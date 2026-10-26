// Package release manages releases and release assets of trees
// (docs/ENGINEERING_PLAN.md §13.5).
//
// A release is a named, annotated version of a tree that points at a tag
// (created on demand). Its assets are arbitrary files whose bytes live in
// the content-addressed object store; the release document only records
// their names, sizes, hashes and media types.
//
// Storage (§13.14): collection "releases", key DocKey(treeID, tag), value
// domain.Release. Authorization is the caller's job (the HTTP layer checks
// tree:read, release:create and release:delete).
package release

import (
	"context"
	"crypto/rand"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"mime"
	"slices"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Coll is the document collection owned by this package.
const Coll = "releases"

// Limits.
const (
	MaxAssetSize = 64 << 20 // bytes per asset
	MaxAssets    = 100      // per release
	MaxName      = 200      // release name, bytes
	MaxNotes     = 100000   // release notes, bytes
	MaxAssetName = 255      // bytes
)

// EventReleaseUpdated is published when a release or its assets change
// (domain has constants for created/deleted only).
const EventReleaseUpdated = "release.updated"

// Deps are the collaborators of the release package. Store is required;
// Objects is required for assets; the others may be nil.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Service is the release component.
type Service struct {
	d   Deps
	now func() time.Time
}

// New returns a Service.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d, now: func() time.Time { return time.Now().UTC() }}
}

// Error is a domain error with a client-facing message; errors.Is matches
// Kind.
type Error struct {
	Kind error
	Msg  string
}

func (e *Error) Error() string { return e.Msg }
func (e *Error) Unwrap() error { return e.Kind }

func errorf(kind error, format string, args ...any) error {
	return &Error{Kind: kind, Msg: fmt.Sprintf(format, args...)}
}

// Key is the document key of the release of tag in treeID.
func Key(treeID, tag string) string { return domain.DocKey(treeID, tag) }

// ---- event subscriber -------------------------------------------------------

// Start subscribes to tree.deleted and removes a deleted tree's releases
// in a background goroutine until ctx is cancelled. Without a Bus it does
// nothing.
func (s *Service) Start(ctx context.Context) {
	if s.d.Bus == nil {
		return
	}
	ch, cancel := s.d.Bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTreeDeleted}})
	go func() {
		defer cancel()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-ch:
				if !ok {
					return
				}
				if err := s.HandleEvent(context.WithoutCancel(ctx), e); err != nil {
					s.d.Logger.WarnContext(ctx, "release: handling event failed", "type", e.Type, "tree", e.TreeID, "err", err)
				}
			}
		}
	}()
}

// HandleEvent processes one bus event synchronously.
func (s *Service) HandleEvent(ctx context.Context, e domain.Event) error {
	if e.Type == domain.EventTreeDeleted && e.TreeID != "" {
		return s.DeleteTree(ctx, e.TreeID)
	}
	return nil
}

// DeleteTree removes every release document of treeID. Asset bytes stay
// in the object store (content-addressed, possibly shared) for garbage
// collection.
func (s *Service) DeleteTree(ctx context.Context, treeID string) error {
	_, err := s.d.Store.DeleteDocs(ctx, Coll, meta.DocPrefix(treeID))
	return err
}

// AssetHashes returns the object hashes of every release asset of every
// tree (roots for object-store garbage collection).
func AssetHashes(ctx context.Context, store domain.DocStore) ([]hash.Hash, error) {
	rels, err := meta.ListDocsT[domain.Release](ctx, store, Coll, "")
	if err != nil {
		return nil, err
	}
	var out []hash.Hash
	for _, r := range rels {
		for _, a := range r.Assets {
			out = append(out, a.Hash)
		}
	}
	return out, nil
}

// ---- queries ------------------------------------------------------------------

// List returns the releases of treeID, newest first. Drafts are included
// only when withDrafts is set.
func (s *Service) List(ctx context.Context, treeID string, withDrafts bool) ([]domain.Release, error) {
	all, err := meta.ListDocsT[domain.Release](ctx, s.d.Store, Coll, meta.DocPrefix(treeID))
	if err != nil {
		return nil, err
	}
	out := make([]domain.Release, 0, len(all))
	for _, r := range all {
		if r.Draft && !withDrafts {
			continue
		}
		out = append(out, normalize(r))
	}
	s.decorate(ctx, treeID, out)
	slices.SortStableFunc(out, func(a, b domain.Release) int {
		if c := b.CreatedAt.Compare(a.CreatedAt); c != 0 {
			return c
		}
		return strings.Compare(b.Tag, a.Tag)
	})
	return out, nil
}

func normalize(r domain.Release) domain.Release {
	if r.Assets == nil {
		r.Assets = []domain.ReleaseAsset{}
	}
	return r
}

// Get returns the release of tag. Drafts are found only with withDrafts.
func (s *Service) Get(ctx context.Context, treeID, tag string, withDrafts bool) (*domain.Release, error) {
	r, err := meta.GetDocT[domain.Release](ctx, s.d.Store, Coll, Key(treeID, tag))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, errorf(domain.ErrNotFound, "release %q not found", tag)
		}
		return nil, err
	}
	if r.Draft && !withDrafts {
		return nil, errorf(domain.ErrNotFound, "release %q not found", tag)
	}
	one := []domain.Release{normalize(*r)}
	s.decorate(ctx, treeID, one)
	return &one[0], nil
}

// ---- create / update / delete --------------------------------------------------

// CreateInput describes a new release. SnapshotID is a resolved snapshot
// id of the tree ("" = the tag must exist).
type CreateInput struct {
	Tag        string
	Name       string
	Notes      string
	Draft      bool
	Prerelease bool
	SnapshotID string

	// CheckTag, when set, vets a tag the release creates before it is
	// stored (v0.7 trust: signature and RequireSigned checks).
	CheckTag func(*domain.Tag) error
}

func validateText(field, v string, max int) error {
	if len(v) > max {
		return errorf(domain.ErrInvalid, "%s must be at most %d bytes", field, max)
	}
	if !utf8.ValidString(v) {
		return errorf(domain.ErrInvalid, "%s must be valid UTF-8", field)
	}
	return nil
}

// Create adds a release for in.Tag. When the tag does not exist it is
// created at in.SnapshotID (required then); when it exists, SnapshotID
// (if given) must equal its target. tenant is the tree's tenant.
func (s *Service) Create(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree, in CreateInput) (*domain.Release, error) {
	if strings.TrimSpace(in.Tag) == "" {
		return nil, errorf(domain.ErrInvalid, "tag is required")
	}
	if err := validateText("name", in.Name, MaxName); err != nil {
		return nil, err
	}
	if err := validateText("notes", in.Notes, MaxNotes); err != nil {
		return nil, err
	}
	// Refuse before touching the tag when the release already exists.
	var existing domain.Release
	switch err := s.d.Store.GetDoc(ctx, Coll, Key(tree.ID, in.Tag), &existing); {
	case err == nil:
		return nil, errorf(domain.ErrAlreadyExists, "release %q already exists", in.Tag)
	case !errors.Is(err, domain.ErrNotFound):
		return nil, err
	}

	tag, err := s.d.Store.GetTag(ctx, tree.ID, in.Tag)
	switch {
	case err == nil:
		if in.SnapshotID != "" && in.SnapshotID != tag.Target {
			return nil, errorf(domain.ErrConflict, "tag %q already exists at %s", in.Tag, tag.Target)
		}
	case errors.Is(err, domain.ErrNotFound):
		if in.SnapshotID == "" {
			return nil, errorf(domain.ErrInvalid, "tag %q does not exist; give a snapshot to create it at", in.Tag)
		}
		tag = &domain.Tag{TreeID: tree.ID, Name: in.Tag, Target: in.SnapshotID,
			Message: "Release " + in.Tag, Tagger: username(p), CreatedAt: s.now()}
		// v0.7 trust (§15.8): RequireSigned tag protections refuse the
		// (unsigned) tag a release would create.
		if in.CheckTag != nil {
			if err := in.CheckTag(tag); err != nil {
				return nil, err
			}
		}
		if err := s.d.Store.PutTag(ctx, tag); err != nil {
			if errors.Is(err, domain.ErrAlreadyExists) {
				return nil, errorf(domain.ErrConflict, "tag %q was created concurrently", in.Tag)
			}
			return nil, err
		}
		s.publish(domain.EventTagPushed, tree, p, map[string]any{"tag": tag.Name, "target": tag.Target})
		s.audit(ctx, p, tenant, "tag.create", "tag", tree.ID+"/"+tag.Name,
			map[string]string{"tag": tag.Name, "target": tag.Target, "via": "release"})
	default:
		return nil, err
	}

	now := s.now()
	r := domain.Release{
		ID:         newID(),
		TreeID:     tree.ID,
		Tag:        in.Tag,
		SnapshotID: tag.Target,
		Name:       strings.TrimSpace(in.Name),
		Notes:      in.Notes,
		Draft:      in.Draft,
		Prerelease: in.Prerelease,
		Assets:     []domain.ReleaseAsset{},
		CreatedBy:  accountID(p),
		Author:     username(p),
		CreatedAt:  now,
	}
	if r.Name == "" {
		r.Name = in.Tag
	}
	if !r.Draft {
		r.PublishedAt = &now
	}
	if err := s.d.Store.CreateDoc(ctx, Coll, Key(tree.ID, in.Tag), r); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return nil, errorf(domain.ErrAlreadyExists, "release %q already exists", in.Tag)
		}
		return nil, err
	}
	r.Verified, r.SigningKeyID, r.Signer = tag.Verified, tag.SigningKeyID, tag.Signer // v0.7 trust: computed, not stored
	s.publish(domain.EventReleaseCreated, tree, p, &r)
	s.audit(ctx, p, tenant, "release.create", "release", tree.ID+"/"+r.Tag, map[string]string{
		"tag": r.Tag, "snapshot": r.SnapshotID, "draft": fmt.Sprint(r.Draft), "prerelease": fmt.Sprint(r.Prerelease),
	})
	return &r, nil
}

// Patch is a partial release update; nil fields are unchanged.
type Patch struct {
	Name       *string `json:"name"`
	Notes      *string `json:"notes"`
	Draft      *bool   `json:"draft"`
	Prerelease *bool   `json:"prerelease"`
}

// Update applies patch to the release of tag. Publishing a draft
// (draft=false) sets PublishedAt.
func (s *Service) Update(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree, tag string, patch Patch) (*domain.Release, error) {
	if patch.Name != nil {
		if err := validateText("name", *patch.Name, MaxName); err != nil {
			return nil, err
		}
	}
	if patch.Notes != nil {
		if err := validateText("notes", *patch.Notes, MaxNotes); err != nil {
			return nil, err
		}
	}
	details := map[string]string{"tag": tag}
	return s.mutate(ctx, p, tenant, tree, tag, "release.update", details, func(r *domain.Release, now time.Time) (bool, error) {
		changed := false
		if patch.Name != nil {
			name := strings.TrimSpace(*patch.Name)
			if name == "" {
				name = r.Tag
			}
			if name != r.Name {
				r.Name, changed = name, true
				details["name"] = name
			}
		}
		if patch.Notes != nil && *patch.Notes != r.Notes {
			r.Notes, changed = *patch.Notes, true
			details["notes"] = "changed"
		}
		if patch.Prerelease != nil && *patch.Prerelease != r.Prerelease {
			r.Prerelease, changed = *patch.Prerelease, true
			details["prerelease"] = fmt.Sprint(r.Prerelease)
		}
		if patch.Draft != nil && *patch.Draft != r.Draft {
			r.Draft, changed = *patch.Draft, true
			details["draft"] = fmt.Sprint(r.Draft)
			if !r.Draft && r.PublishedAt == nil {
				r.PublishedAt = &now
			}
		}
		return changed, nil
	})
}

// Delete removes the release of tag (the tag itself is kept).
func (s *Service) Delete(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree, tag string) error {
	var gone *domain.Release
	err := meta.UpdateDocT(ctx, s.d.Store, Coll, Key(tree.ID, tag), func(cur *domain.Release) (*domain.Release, error) {
		if cur == nil {
			return nil, errorf(domain.ErrNotFound, "release %q not found", tag)
		}
		gone = cur
		return nil, nil
	})
	if err != nil {
		return err
	}
	s.publish(domain.EventReleaseDeleted, tree, p, gone)
	s.audit(ctx, p, tenant, "release.delete", "release", tree.ID+"/"+tag, map[string]string{
		"tag": tag, "assets": fmt.Sprint(len(gone.Assets)),
	})
	return nil
}

// mutate atomically applies fn to the release of tag; changes are saved,
// published (release.updated) and audited as action.
func (s *Service) mutate(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree, tag, action string,
	details map[string]string, fn func(r *domain.Release, now time.Time) (bool, error)) (*domain.Release, error) {
	var out *domain.Release
	changed := false
	err := meta.UpdateDocT(ctx, s.d.Store, Coll, Key(tree.ID, tag), func(cur *domain.Release) (*domain.Release, error) {
		if cur == nil {
			return nil, errorf(domain.ErrNotFound, "release %q not found", tag)
		}
		c, err := fn(cur, s.now())
		if err != nil {
			return nil, err
		}
		changed, out = c, cur
		return cur, nil
	})
	if err != nil {
		return nil, err
	}
	one := []domain.Release{normalize(*out)}
	s.decorate(ctx, tree.ID, one)
	n := one[0]
	if changed {
		s.publish(EventReleaseUpdated, tree, p, &n)
		s.audit(ctx, p, tenant, action, "release", tree.ID+"/"+tag, details)
	}
	return &n, nil
}

// ---- assets ------------------------------------------------------------------------

// ValidateAssetName checks an asset file name: 1-255 bytes of UTF-8, no
// path separators, control characters, or the names "." and "..".
func ValidateAssetName(name string) error {
	switch {
	case name == "" || len(name) > MaxAssetName:
		return errorf(domain.ErrInvalid, "asset name must be 1-%d bytes", MaxAssetName)
	case !utf8.ValidString(name):
		return errorf(domain.ErrInvalid, "asset name must be valid UTF-8")
	case name == "." || name == "..":
		return errorf(domain.ErrInvalid, "asset name must not be %q", name)
	case strings.ContainsAny(name, `/\`) || strings.ContainsFunc(name, unicode.IsControl):
		return errorf(domain.ErrInvalid, "asset name must not contain '/', '\\' or control characters")
	case strings.TrimSpace(name) != name:
		return errorf(domain.ErrInvalid, "asset name must not start or end with whitespace")
	}
	return nil
}

// contentType returns a valid media type: declared when it parses, else
// guessed from the name's extension, else application/octet-stream.
func contentType(declared, name string) string {
	if declared != "" {
		if mt, params, err := mime.ParseMediaType(declared); err == nil {
			return mime.FormatMediaType(mt, params)
		}
	}
	if i := strings.LastIndexByte(name, '.'); i >= 0 {
		if t := mime.TypeByExtension(name[i:]); t != "" {
			return t
		}
	}
	return "application/octet-stream"
}

// PutAsset stores body (at most MaxAssetSize bytes) as asset name of the
// release of tag, replacing an asset of the same name.
func (s *Service) PutAsset(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree, tag, name, declaredType string, body io.Reader) (*domain.ReleaseAsset, error) {
	if s.d.Objects == nil {
		return nil, errors.New("release: no object store configured")
	}
	if err := ValidateAssetName(name); err != nil {
		return nil, err
	}
	// The release must exist before the upload is read.
	if _, err := s.Get(ctx, tree.ID, tag, true); err != nil {
		return nil, err
	}
	data, err := io.ReadAll(io.LimitReader(body, MaxAssetSize+1))
	if err != nil {
		return nil, err
	}
	if len(data) > MaxAssetSize {
		return nil, &TooLargeError{Limit: MaxAssetSize}
	}
	h := hash.Sum(data)
	if err := s.d.Objects.Put(ctx, h, data); err != nil {
		return nil, fmt.Errorf("release: store asset: %w", err)
	}
	asset := domain.ReleaseAsset{
		Name: name, Size: int64(len(data)), Hash: h,
		ContentType: contentType(declaredType, name), UploadedAt: s.now(),
		SHA256: sha256Hex(data), // v0.7 trust (§15.8)
	}
	details := map[string]string{"tag": tag, "asset": name, "size": fmt.Sprint(asset.Size), "hash": h.String()}
	_, err = s.mutate(ctx, p, tenant, tree, tag, "release.asset.upload", details, func(r *domain.Release, _ time.Time) (bool, error) {
		i := slices.IndexFunc(r.Assets, func(a domain.ReleaseAsset) bool { return a.Name == name })
		if i >= 0 {
			r.Assets[i] = asset
			return true, nil
		}
		if len(r.Assets) >= MaxAssets {
			return false, errorf(domain.ErrConflict, "a release holds at most %d assets", MaxAssets)
		}
		r.Assets = append(r.Assets, asset)
		return true, nil
	})
	if err != nil {
		return nil, err
	}
	return &asset, nil
}

// TooLargeError reports an asset over the size limit.
type TooLargeError struct{ Limit int64 }

func (e *TooLargeError) Error() string {
	return fmt.Sprintf("asset exceeds %d bytes", e.Limit)
}

// Asset returns the metadata and bytes of asset name of the release of tag.
func (s *Service) Asset(ctx context.Context, treeID, tag, name string, withDrafts bool) (*domain.ReleaseAsset, []byte, error) {
	r, err := s.Get(ctx, treeID, tag, withDrafts)
	if err != nil {
		return nil, nil, err
	}
	i := slices.IndexFunc(r.Assets, func(a domain.ReleaseAsset) bool { return a.Name == name })
	if i < 0 {
		return nil, nil, errorf(domain.ErrNotFound, "asset %q of release %q not found", name, tag)
	}
	if s.d.Objects == nil {
		return nil, nil, errors.New("release: no object store configured")
	}
	data, err := s.d.Objects.Get(ctx, r.Assets[i].Hash)
	if err != nil {
		if errors.Is(err, objstore.ErrNotFound) {
			return nil, nil, errorf(domain.ErrNotFound, "content of asset %q is missing", name)
		}
		return nil, nil, err
	}
	a := r.Assets[i]
	return &a, data, nil
}

// DeleteAsset removes asset name from the release of tag.
func (s *Service) DeleteAsset(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree, tag, name string) error {
	details := map[string]string{"tag": tag, "asset": name}
	_, err := s.mutate(ctx, p, tenant, tree, tag, "release.asset.delete", details, func(r *domain.Release, _ time.Time) (bool, error) {
		i := slices.IndexFunc(r.Assets, func(a domain.ReleaseAsset) bool { return a.Name == name })
		if i < 0 {
			return false, errorf(domain.ErrNotFound, "asset %q of release %q not found", name, tag)
		}
		r.Assets = slices.Delete(r.Assets, i, i+1)
		return true, nil
	})
	return err
}

// ---- events and audit -----------------------------------------------------------

func username(p *domain.Principal) string {
	if p == nil {
		return "anonymous"
	}
	return p.Username
}

func accountID(p *domain.Principal) string {
	if p == nil {
		return ""
	}
	return p.AccountID
}

func (s *Service) publish(typ string, tree *domain.Tree, p *domain.Principal, payload any) {
	if s.d.Bus == nil {
		return
	}
	s.d.Bus.Publish(domain.Event{Type: typ, TenantID: tree.TenantID, TreeID: tree.ID, Actor: username(p), Payload: payload, At: s.now()})
}

func (s *Service) audit(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, action, resType, resID string, details map[string]string) {
	if s.d.Audit == nil {
		return
	}
	tenantID := ""
	if tenant != nil {
		tenantID = tenant.ID
	}
	s.d.Audit.Record(context.WithoutCancel(ctx), domain.AuditEvent{
		ID: newID(), TenantID: tenantID, ActorID: accountID(p), Actor: username(p),
		Action: action, ResourceType: resType, ResourceID: resID, Details: details, At: s.now(),
	})
}

// newID returns a random RFC 4122 version 4 UUID string.
func newID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	b[6] = b[6]&0x0f | 0x40
	b[8] = b[8]&0x3f | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}
