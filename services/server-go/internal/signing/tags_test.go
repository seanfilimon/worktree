package signing_test

// v0.7 trust (§15.8): signed tags.

import (
	"context"
	"crypto/ed25519"
	"encoding/hex"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
	"github.com/seanfilimon/worktree/services/server-go/internal/signing"
	"github.com/seanfilimon/worktree/services/server-go/internal/syncsvc"
)

// Cross-language tag vector, asserted identically by the Rust SDK
// (crates/worktree-sdk/src/signing.rs, tests::tag_cross_language_vector);
// same seed (and so key and fingerprint) as the snapshot vector.
const (
	tagVectorPayload = "0a0676312e302e3012260a2430663866616435622d643963622d343639662d61" +
		"3136352d3730383637373238393530651a1652656c6561736520312e302e300a" +
		"0a5369676e65642e221e416461204c6f76656c616365203c616461406578616d" +
		"706c652e636f6d3e2a0b08a5ebdcca0610959aef3a"
	tagVectorSignature = "0dc28f052fa81fe86ff9ad4e67f8a282e4aa97987665958ce5b4cc5d3a3b828a" +
		"fbad99d3824577adca5acd0732d9e3f39eb48a3f6e25c93664ccdf9179d93a0a"
)

var vectorTime = time.Date(2026, 1, 2, 3, 4, 5, 123456789, time.UTC)

func vectorTagRecord() *syncpb.TagRecord {
	return &syncpb.TagRecord{
		Name:         "v1.0.0",
		Target:       &syncpb.SnapshotId{Value: "0f8fad5b-d9cb-469f-a165-70867728950e"},
		Message:      proto.String("Release 1.0.0\n\nSigned."),
		Tagger:       "Ada Lovelace <ada@example.com>",
		CreatedAt:    timestamppb.New(vectorTime),
		Signature:    []byte{1, 2, 3},
		SigningKeyId: "stale",
	}
}

// vectorTag is the record as the server stores it (the authenticated
// pusher as Tagger, the signed value as SignedTagger).
func vectorTag() *domain.Tag {
	return &domain.Tag{
		TreeID:       "server-tree",
		Name:         "v1.0.0",
		Target:       "0f8fad5b-d9cb-469f-a165-70867728950e",
		Message:      "Release 1.0.0\n\nSigned.",
		Tagger:       "ada",
		SignedTagger: "Ada Lovelace <ada@example.com>",
		CreatedAt:    vectorTime,
		Signature:    []byte{9},
		SigningKeyID: "stale",
		Verified:     true,
	}
}

func TestTagCrossLanguageVector(t *testing.T) {
	priv := ed25519.NewKeyFromSeed(vectorSeed())
	fromRec, err := signing.TagPayloadFromRecord(vectorTagRecord())
	must(t, err)
	if got := hex.EncodeToString(fromRec); got != tagVectorPayload {
		t.Fatalf("payload from record = %s", got)
	}
	fromTag, err := signing.TagPayload(vectorTag())
	must(t, err)
	if got := hex.EncodeToString(fromTag); got != tagVectorPayload {
		t.Fatalf("payload from tag = %s", got)
	}
	if got := hex.EncodeToString(ed25519.Sign(priv, fromTag)); got != tagVectorSignature {
		t.Fatalf("signature = %s", got)
	}
	if vectorTagRecord().GetSigningKeyId() != "stale" {
		t.Fatal("record mutated")
	}
}

func TestTagPayloadEmptyMessageIsAbsent(t *testing.T) {
	rec := vectorTagRecord()
	rec.Message = proto.String("")
	a, err := signing.TagPayloadFromRecord(rec)
	must(t, err)
	rec.Message = nil
	b, err := signing.TagPayloadFromRecord(rec)
	must(t, err)
	tag := vectorTag()
	tag.Message = ""
	c, err := signing.TagPayload(tag)
	must(t, err)
	if hex.EncodeToString(a) != hex.EncodeToString(b) || hex.EncodeToString(b) != hex.EncodeToString(c) {
		t.Fatal("empty message encodings differ")
	}
	// Without a SignedTagger the stored tagger is covered.
	tag = vectorTag()
	tag.SignedTagger = ""
	if signing.SignedTagger(tag) != "ada" {
		t.Fatal("SignedTagger fallback")
	}
}

func signTag(t *testing.T, tag *domain.Tag, k ed25519.PrivateKey, keyID string) {
	t.Helper()
	tag.Signature, tag.SigningKeyID = nil, keyID
	payload, err := signing.TagPayload(tag)
	must(t, err)
	tag.Signature = ed25519.Sign(k, payload)
}

func newTag(treeID, name string) *domain.Tag {
	return &domain.Tag{
		TreeID: treeID, Name: name, Target: uuid.NewString(), Message: "m",
		Tagger: "alice", SignedTagger: "Alice <alice@example.com>",
		CreatedAt: time.Date(2026, 3, 4, 5, 6, 7, 8, time.UTC),
	}
}

func TestCheckTagSignature(t *testing.T) {
	ctx := context.Background()
	f := newFixture(t)
	must(t, f.store.CreateAccount(ctx, &domain.Account{ID: alice.AccountID, Username: "alice", Email: "alice@example.com"}))
	k := newKey(t, 11)
	key, err := f.svc.Register(ctx, alice, "laptop", "", pubOf(k))
	must(t, err)

	// Unsigned without protection: accepted, not verified, no signed tagger.
	tag := newTag(f.tree.ID, "v1")
	tag.Verified, tag.Signer = true, "mallory" // clients cannot claim these
	must(t, f.svc.CheckTagSignature(ctx, alice, f.tree, tag))
	if tag.Verified || tag.Signer != "" || tag.SignedTagger != "" {
		t.Fatalf("unsigned tag = %+v", tag)
	}

	// Signed by id and by fingerprint: verified, signer is the key owner.
	for _, id := range []string{key.ID, key.Fingerprint} {
		tag := newTag(f.tree.ID, "v2")
		signTag(t, tag, k, id)
		must(t, f.svc.CheckTagSignature(ctx, alice, f.tree, tag))
		if !tag.Verified || tag.Signer != "alice" {
			t.Fatalf("signed tag = %+v", tag)
		}
	}

	// Tampering, unknown keys, truncation and key ids without signature.
	bad := []func(*domain.Tag){
		func(tag *domain.Tag) { tag.Message = "rewritten" },
		func(tag *domain.Tag) { tag.Target = uuid.NewString() },
		func(tag *domain.Tag) { tag.SignedTagger = "Mallory" },
		func(tag *domain.Tag) { tag.CreatedAt = tag.CreatedAt.Add(time.Nanosecond) },
		func(tag *domain.Tag) { tag.SigningKeyID = "nope" },
		func(tag *domain.Tag) { tag.Signature = tag.Signature[:5] },
		func(tag *domain.Tag) { tag.Signature = nil },
	}
	for i, tamper := range bad {
		tag := newTag(f.tree.ID, "v3")
		signTag(t, tag, k, key.ID)
		tamper(tag)
		var denied *domain.PushDenied
		err := f.svc.CheckTagSignature(ctx, alice, f.tree, tag)
		if !errors.As(err, &denied) || denied.Kind != signing.KindSignatureRequired || !strings.Contains(denied.Reason, "invalid signature") {
			t.Fatalf("case %d: err = %v", i, err)
		}
		if tag.Verified {
			t.Fatalf("case %d: verified", i)
		}
	}
	if n := f.audit.count(signing.AuditTagSignatureInvalid); n != len(bad) {
		t.Fatalf("audited %d, want %d", n, len(bad))
	}

	// Revoked keys no longer verify.
	_, err = f.svc.Revoke(ctx, alice.AccountID, key.ID)
	must(t, err)
	tag = newTag(f.tree.ID, "v4")
	signTag(t, tag, k, key.ID)
	var denied *domain.PushDenied
	if err := f.svc.CheckTagSignature(ctx, alice, f.tree, tag); !errors.As(err, &denied) {
		t.Fatalf("revoked: %v", err)
	}
}

func TestTagRequireSigned(t *testing.T) {
	ctx := context.Background()
	f := newFixture(t)
	k := newKey(t, 12)
	key, err := f.svc.Register(ctx, alice, "laptop", "", pubOf(k))
	must(t, err)
	put := func(tp domain.TagProtection) {
		tp.TreeID = f.tree.ID
		must(t, f.store.PutDoc(ctx, review.TagProtectionCollection, domain.DocKey(f.tree.ID, tp.Pattern), &tp))
	}
	put(domain.TagProtection{Pattern: "v*", RequireSigned: true})
	put(domain.TagProtection{Pattern: "nightly-*", NoDelete: true})

	if p, err := f.svc.TagRequiresSigned(ctx, f.tree.ID, "v1.0"); err != nil || p != "v*" {
		t.Fatalf("v1.0: %q %v", p, err)
	}
	if p, err := f.svc.TagRequiresSigned(ctx, f.tree.ID, "nightly-1"); err != nil || p != "" {
		t.Fatalf("nightly: %q %v", p, err)
	}

	var denied *domain.PushDenied
	err = f.svc.CheckTagSignature(ctx, alice, f.tree, newTag(f.tree.ID, "v1.0"))
	if !errors.As(err, &denied) || !strings.Contains(denied.Reason, "must be signed") {
		t.Fatalf("unsigned under require_signed: %v", err)
	}
	must(t, f.svc.CheckTagSignature(ctx, alice, f.tree, newTag(f.tree.ID, "nightly-1")))
	signed := newTag(f.tree.ID, "v1.0")
	signTag(t, signed, k, key.ID)
	must(t, f.svc.CheckTagSignature(ctx, alice, f.tree, signed))
	if !signed.Verified {
		t.Fatal("signed tag not verified")
	}
}

// ---- end to end through syncsvc.PushTag / ListTags ---------------------

func TestPushTagVerifiesSignatures(t *testing.T) {
	f := newFixture(t)
	ctx := context.Background()
	objects := objstore.New(t.TempDir())
	syncSvc := syncsvc.New(syncsvc.Deps{
		Store:      f.store,
		Objects:    objects,
		History:    meta.NewHistory(f.store),
		Authz:      allowAll{},
		Audit:      f.audit,
		PushChecks: []domain.PushChecker{f.svc},
	})
	k := newKey(t, 13)
	key, err := f.svc.Register(ctx, alice, "laptop", "", pubOf(k))
	must(t, err)

	mdata, err := proto.MarshalOptions{Deterministic: true}.Marshal(&syncpb.ManifestRecord{})
	must(t, err)
	mh := hash.Sum(mdata)
	must(t, objects.Put(ctx, mh, mdata))
	snapID := uuid.NewString()
	resp, err := syncSvc.Push(iam.WithPrincipal(ctx, alice), &syncpb.PushRequest{
		TreeId:     &syncpb.TreeId{Value: f.tree.ID},
		BranchName: "main",
		NewTip:     &syncpb.SnapshotId{Value: snapID},
		Snapshots: []*syncpb.SnapshotRecord{{
			Id: &syncpb.SnapshotId{Value: snapID}, TreeId: &syncpb.TreeId{Value: f.tree.ID}, BranchName: "main",
			ManifestHash: &syncpb.ContentHash{Value: mh[:]}, Author: "alice", Message: "m",
			CreatedAt: timestamppb.New(time.Date(2026, 5, 1, 0, 0, 0, 0, time.UTC)),
		}},
	})
	must(t, err)
	if !resp.GetAccepted() {
		t.Fatalf("push rejected: %v", resp.GetRejectionReason())
	}

	record := func(name string) *syncpb.TagRecord {
		return &syncpb.TagRecord{
			Name: name, Target: &syncpb.SnapshotId{Value: snapID}, Message: proto.String("release " + name),
			Tagger: "Alice <alice@example.com>", CreatedAt: timestamppb.New(time.Date(2026, 5, 2, 0, 0, 0, 42, time.UTC)),
		}
	}
	sign := func(rec *syncpb.TagRecord, keyID string) {
		payload, err := signing.TagPayloadFromRecord(rec)
		must(t, err)
		rec.Signature = ed25519.Sign(k, payload)
		rec.SigningKeyId = keyID
	}
	pushTag := func(rec *syncpb.TagRecord) *syncpb.PushTagResponse {
		t.Helper()
		resp, err := syncSvc.PushTag(iam.WithPrincipal(ctx, alice), &syncpb.PushTagRequest{
			TreeId: &syncpb.TreeId{Value: f.tree.ID}, Tag: rec,
		})
		must(t, err)
		return resp
	}

	signed := record("v1.0.0")
	sign(signed, key.Fingerprint)
	if resp := pushTag(signed); !resp.GetAccepted() {
		t.Fatalf("signed tag refused: %s", resp.GetError())
	}
	stored, err := f.store.GetTag(ctx, f.tree.ID, "v1.0.0")
	must(t, err)
	if !stored.Verified || stored.Tagger != "alice" || stored.SignedTagger != "Alice <alice@example.com>" || stored.SigningKeyID != key.Fingerprint {
		t.Fatalf("stored = %+v", stored)
	}
	// The listed record verifies against the same payload.
	list, err := syncSvc.ListTags(iam.WithPrincipal(ctx, alice), &syncpb.ListTagsRequest{TreeId: &syncpb.TreeId{Value: f.tree.ID}})
	must(t, err)
	got := list.GetTags()[0]
	payload, err := signing.TagPayloadFromRecord(got)
	must(t, err)
	if !ed25519.Verify(pubOf(k), payload, got.GetSignature()) || got.GetTagger() != "Alice <alice@example.com>" {
		t.Fatalf("listed record does not verify: %+v", got)
	}

	tampered := record("v1.0.1")
	sign(tampered, key.ID)
	tampered.Message = proto.String("changed")
	if resp := pushTag(tampered); resp.GetAccepted() || !strings.Contains(resp.GetError(), "invalid signature") {
		t.Fatalf("tampered tag: %v %s", resp.GetAccepted(), resp.GetError())
	}
	if _, err := f.store.GetTag(ctx, f.tree.ID, "v1.0.1"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("tampered tag stored: %v", err)
	}

	// Unsigned tags are fine until a RequireSigned protection matches.
	if resp := pushTag(record("plain")); !resp.GetAccepted() {
		t.Fatalf("unsigned tag refused: %s", resp.GetError())
	}
	if s, _ := f.store.GetTag(ctx, f.tree.ID, "plain"); s.Verified || s.SignedTagger != "" {
		t.Fatalf("plain = %+v", s)
	}
	must(t, f.store.PutDoc(ctx, review.TagProtectionCollection, domain.DocKey(f.tree.ID, "v*"),
		&domain.TagProtection{TreeID: f.tree.ID, Pattern: "v*", RequireSigned: true}))
	if resp := pushTag(record("v2.0.0")); resp.GetAccepted() || !strings.Contains(resp.GetError(), "must be signed") {
		t.Fatalf("unsigned v2: %v %s", resp.GetAccepted(), resp.GetError())
	}
	v2 := record("v2.0.0")
	sign(v2, key.ID)
	if resp := pushTag(v2); !resp.GetAccepted() {
		t.Fatalf("signed v2 refused: %s", resp.GetError())
	}
}
