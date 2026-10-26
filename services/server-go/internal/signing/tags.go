package signing

// v0.7 trust (docs/ENGINEERING_PLAN.md §15.8): signed tags.
//
// A tag signature is an Ed25519 signature over the deterministic protobuf
// encoding of the tag's TagRecord with signature and signing_key_id
// cleared and an empty message encoded as absent — byte-for-byte what the
// Rust SDK's signing::tag_signed_payload produces (both sides pin a shared
// test vector). The record's tagger is the value the client signed; the
// server keeps the authenticated creator in Tag.Tagger and the signed
// value in Tag.SignedTagger.

import (
	"context"
	"crypto/ed25519"
	"errors"
	"fmt"

	"github.com/google/uuid"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// AuditTagSignatureInvalid is audited when a tag carries a signature that
// does not verify, or an unsigned tag matches a RequireSigned protection.
const AuditTagSignatureInvalid = "tag.signature_invalid"

// SignedTagger is the tagger a signature of t covers: SignedTagger when
// set, else Tagger.
func SignedTagger(t *domain.Tag) string {
	if t.SignedTagger != "" {
		return t.SignedTagger
	}
	return t.Tagger
}

// TagPayload returns the bytes a signature of t covers (the record syncsvc
// sends for t, signature fields cleared).
func TagPayload(t *domain.Tag) ([]byte, error) {
	rec := &syncpb.TagRecord{
		Name:   t.Name,
		Target: &syncpb.SnapshotId{Value: t.Target},
		Tagger: SignedTagger(t),
	}
	if t.Message != "" {
		rec.Message = proto.String(t.Message)
	}
	if !t.CreatedAt.IsZero() {
		rec.CreatedAt = timestamppb.New(t.CreatedAt)
	}
	return marshalTagPayload(rec)
}

// TagPayloadFromRecord returns the bytes a signature of rec covers.
func TagPayloadFromRecord(rec *syncpb.TagRecord) ([]byte, error) {
	c := proto.Clone(rec).(*syncpb.TagRecord)
	c.Signature = nil
	c.SigningKeyId = ""
	if c.GetMessage() == "" {
		c.Message = nil
	}
	return marshalTagPayload(c)
}

func marshalTagPayload(rec *syncpb.TagRecord) ([]byte, error) {
	b, err := proto.MarshalOptions{Deterministic: true}.Marshal(rec)
	if err != nil {
		return nil, fmt.Errorf("signing: encode tag payload: %w", err)
	}
	return b, nil
}

// VerifyTag checks the signature of t against its registered, unrevoked
// key and returns that key. Failures wrap errBadSignature with a reason.
func (s *Service) VerifyTag(ctx context.Context, t *domain.Tag) (*domain.SigningKey, error) {
	if len(t.Signature) == 0 {
		return nil, fmt.Errorf("%w: tag is not signed", errBadSignature)
	}
	key, err := s.Resolve(ctx, t.SigningKeyID)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, fmt.Errorf("%w: unknown signing key %q", errBadSignature, t.SigningKeyID)
	}
	if err != nil {
		return nil, err
	}
	if key.RevokedAt != nil {
		return nil, fmt.Errorf("%w: signing key %s is revoked", errBadSignature, key.ID)
	}
	if len(key.PublicKey) != ed25519.PublicKeySize || len(t.Signature) != ed25519.SignatureSize {
		return nil, fmt.Errorf("%w: malformed key or signature", errBadSignature)
	}
	payload, err := TagPayload(t)
	if err != nil {
		return nil, err
	}
	if !ed25519.Verify(ed25519.PublicKey(key.PublicKey), payload, t.Signature) {
		return nil, fmt.Errorf("%w: signature does not match key %s", errBadSignature, key.ID)
	}
	return key, nil
}

// TagRequiresSigned returns the pattern of the first tag protection of
// treeID matching name that requires signed tags ("" when none does).
func (s *Service) TagRequiresSigned(ctx context.Context, treeID, name string) (string, error) {
	rules, err := meta.ListDocsT[domain.TagProtection](ctx, s.d.Store, review.TagProtectionCollection, meta.DocPrefix(treeID))
	if err != nil {
		return "", fmt.Errorf("signing: list tag protections: %w", err)
	}
	for _, r := range rules {
		if r.RequireSigned && review.MatchPattern(r.Pattern, name) {
			return r.Pattern, nil
		}
	}
	return "", nil
}

// CheckTagSignature verifies a tag about to be stored in tree: a tag carrying a
// signature (or key id) must verify against a registered, unrevoked key
// and is then marked Verified with the key's account as Signer; an
// unsigned tag matching a RequireSigned tag protection is refused. Both
// refusals are *domain.PushDenied{Kind: "signature_required"} and are
// audited. Clients cannot set Verified or Signer themselves.
func (s *Service) CheckTagSignature(ctx context.Context, p *domain.Principal, tree *domain.Tree, t *domain.Tag) error {
	t.Verified, t.Signer = false, ""
	if len(t.Signature) == 0 && t.SigningKeyID == "" {
		t.SignedTagger = ""
		pattern, err := s.TagRequiresSigned(ctx, tree.ID, t.Name)
		if err != nil {
			return err
		}
		if pattern != "" {
			s.auditTag(ctx, p, tree, t, "unsigned")
			return &domain.PushDenied{
				Kind:   KindSignatureRequired,
				Reason: fmt.Sprintf("tag '%s' is protected by '%s' and must be signed (wt tag %s --sign)", t.Name, pattern, t.Name),
			}
		}
		return nil
	}
	if t.SignedTagger == "" {
		t.SignedTagger = t.Tagger
	}
	key, err := s.VerifyTag(ctx, t)
	if err != nil {
		if !IsBadSignature(err) {
			return err
		}
		s.auditTag(ctx, p, tree, t, err.Error())
		return &domain.PushDenied{
			Kind:   KindSignatureRequired,
			Reason: fmt.Sprintf("tag '%s' has an invalid signature (%s)", t.Name, err),
		}
	}
	t.Verified = true
	t.Signer = key.AccountID
	if acct, err := s.d.Store.GetAccount(ctx, key.AccountID); err == nil && acct.Username != "" {
		t.Signer = acct.Username
	}
	s.touch(ctx, domain.DocKey(key.AccountID, key.ID))
	return nil
}

func (s *Service) auditTag(ctx context.Context, p *domain.Principal, tree *domain.Tree, t *domain.Tag, reason string) {
	if s.d.Audit == nil {
		return
	}
	e := domain.AuditEvent{
		ID:           uuid.NewString(),
		TenantID:     tree.TenantID,
		Actor:        "anonymous",
		Action:       AuditTagSignatureInvalid,
		ResourceType: "tag",
		ResourceID:   tree.ID + "/" + t.Name,
		Details: map[string]string{
			"tree":   tree.Name,
			"tag":    t.Name,
			"target": t.Target,
			"key_id": t.SigningKeyID,
			"reason": reason,
		},
		At: s.now(),
	}
	if p != nil {
		e.ActorID, e.Actor = p.AccountID, p.Username
	}
	s.d.Audit.Record(context.WithoutCancel(ctx), e)
}
