package httpapi

import (
	"context"
	"errors"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
)

// ---- fakes specific to the management endpoints ----
//
// They wrap the shared fakes (fakes_test.go) instead of adding methods to
// them, so other test files can extend the shared fakes independently.

// manageStore adds the MetaStore methods the management endpoints need.
type manageStore struct{ *fakeStore }

func (s *manageStore) UpdateTenant(_ context.Context, t *domain.Tenant) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.tenants[t.ID]; !ok {
		return nf("tenant")
	}
	s.tenants[t.ID] = *t
	return nil
}

func (s *manageStore) CompareAndSwapBranchTip(_ context.Context, tree, name, oldTip, newTip string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	k := [2]string{tree, name}
	cur, exists := s.branches[k]
	if cur.Tip != oldTip {
		return fmt.Errorf("branch tip is %q: %w", cur.Tip, domain.ErrConflict)
	}
	now := time.Now().UTC()
	if !exists {
		cur = domain.Branch{TreeID: tree, Name: name, CreatedAt: now}
	}
	cur.Tip, cur.UpdatedAt = newTip, now
	s.branches[k] = cur
	return nil
}

func (s *manageStore) DeleteBranch(_ context.Context, tree, name string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	k := [2]string{tree, name}
	if _, ok := s.branches[k]; !ok {
		return nf("branch")
	}
	delete(s.branches, k)
	return nil
}

func (s *manageStore) PutTag(_ context.Context, t *domain.Tag) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	k := [2]string{t.TreeID, t.Name}
	if _, ok := s.tags[k]; ok {
		return fmt.Errorf("tag: %w", domain.ErrAlreadyExists)
	}
	s.tags[k] = *t
	return nil
}

func (s *manageStore) GetTag(_ context.Context, tree, name string) (*domain.Tag, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.tags[[2]string{tree, name}]
	if !ok {
		return nil, nf("tag")
	}
	return &t, nil
}

func (s *manageStore) DeleteTag(_ context.Context, tree, name string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	k := [2]string{tree, name}
	if _, ok := s.tags[k]; !ok {
		return nf("tag")
	}
	delete(s.tags, k)
	return nil
}

// manageAuth makes SetPassword effective and implements accountCreator.
type manageAuth struct {
	*fakeAuth
	created int // CreateAccount calls
}

func (f *manageAuth) SetPassword(ctx context.Context, accountID, password string) error {
	if utf8.RuneCountInString(password) < 8 {
		return fmt.Errorf("password too short: %w", domain.ErrInvalid)
	}
	a, err := f.store.GetAccount(ctx, accountID)
	if err != nil {
		return err
	}
	f.mu.Lock()
	f.passwords[a.Username] = password
	f.mu.Unlock()
	return nil
}

func (f *manageAuth) password(username string) string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.passwords[username]
}

func (f *manageAuth) CreateAccount(ctx context.Context, username, email, password string, isAdmin bool) (*domain.Account, error) {
	f.mu.Lock()
	f.created++
	f.mu.Unlock()
	accts, _ := f.store.ListAccounts(ctx)
	for _, a := range accts {
		if a.Username == username || strings.EqualFold(a.Email, email) {
			return nil, fmt.Errorf("account: %w", domain.ErrAlreadyExists)
		}
	}
	t := &domain.Tenant{ID: newID(), Slug: username, Name: username, Kind: domain.TenantUser, Plan: "free", CreatedAt: time.Now()}
	if err := f.store.CreateTenant(ctx, t); err != nil {
		return nil, err
	}
	a := &domain.Account{ID: newID(), Username: username, Email: email, TenantID: t.ID, IsAdmin: isAdmin, CreatedAt: time.Now()}
	if err := f.store.CreateAccount(ctx, a); err != nil {
		return nil, err
	}
	if err := f.store.PutMembership(ctx, &domain.Membership{TenantID: t.ID, AccountID: a.ID, Role: domain.RoleOwner}); err != nil {
		return nil, err
	}
	f.mu.Lock()
	f.passwords[username] = password
	f.mu.Unlock()
	return a, nil
}

// manageReview enforces branch deletion rules like internal/review: the
// branch:delete permission, the default branch and NoDelete protections.
type manageReview struct{ *fakeReview }

func (r *manageReview) CheckDeleteBranch(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string) error {
	if err := r.authz.Authorize(ctx, p, domain.ActBranchDelete, tree.TenantID, tree); err != nil {
		return err
	}
	if branch == tree.DefaultBranch {
		return &domain.PushDenied{Kind: "branch_protection", Reason: "default branch cannot be deleted"}
	}
	ps, _ := r.store.ListProtections(ctx, tree.ID)
	for _, pr := range ps {
		if pr.Pattern == branch && pr.NoDelete {
			return &domain.PushDenied{Kind: "branch_protection", Reason: "branch is protected"}
		}
	}
	return nil
}

// recordingBus remembers every published event.
type recordingBus struct {
	*fakeBus
	mu        sync.Mutex
	published []domain.Event
}

func (b *recordingBus) Publish(e domain.Event) {
	b.mu.Lock()
	b.published = append(b.published, e)
	b.mu.Unlock()
	b.fakeBus.Publish(e)
}

func (b *recordingBus) types() []string {
	b.mu.Lock()
	defer b.mu.Unlock()
	var out []string
	for _, e := range b.published {
		out = append(out, e.Type)
	}
	return out
}

// newManageFixture is newFixture with the management fakes wired in.
func newManageFixture(t testing.TB, mutate ...func(*Deps)) *fixture {
	t.Helper()
	wire := func(d *Deps) {
		d.Store = &manageStore{fakeStore: d.Store.(*fakeStore)}
		d.Auth = &manageAuth{fakeAuth: d.Auth.(*fakeAuth)}
		d.Review = &manageReview{fakeReview: d.Review.(*fakeReview)}
		d.Bus = &recordingBus{fakeBus: d.Bus.(*fakeBus)}
	}
	return newFixture(t, append([]func(*Deps){wire}, mutate...)...)
}

func (f *fixture) manageAuth() *manageAuth { return f.deps.Auth.(*manageAuth) }

func (f *fixture) recBus() *recordingBus { return f.deps.Bus.(*recordingBus) }

// runManageCases is runCases over newManageFixture.
func runManageCases(t *testing.T, cases []endpointCase) {
	t.Helper()
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			f := newManageFixture(t)
			if tc.setup != nil {
				tc.setup(t, f)
			}
			rec := f.do(t, tc.method, tc.path, tc.as, tc.body)
			if rec.Code != tc.status {
				t.Fatalf("%s %s: status = %d, want %d; body: %s", tc.method, tc.path, rec.Code, tc.status, rec.Body.String())
			}
			if tc.code != "" {
				if got := errCode(rec); got != tc.code {
					t.Fatalf("error code = %q, want %q; body: %s", got, tc.code, rec.Body.String())
				}
			}
			if rec.Code >= 400 && errCode(rec) == "" {
				t.Fatalf("error response without envelope: %s", rec.Body.String())
			}
			if tc.check != nil {
				tc.check(t, f, rec)
			}
		})
	}
}

func hasEvent(t *testing.T, f *fixture, typ string) {
	t.Helper()
	if !slices.Contains(f.recBus().types(), typ) {
		t.Fatalf("event %q not published; have %v", typ, f.recBus().types())
	}
}

func noSideEffects(t *testing.T, f *fixture, _ *httptest.ResponseRecorder) {
	t.Helper()
	// ops (§13.3): 403s are audited as access.denied by design.
	if got := nonDenialActions(f); len(got) != 0 {
		t.Fatalf("unexpected audit events %v", got)
	}
	if got := f.recBus().types(); len(got) != 0 {
		t.Fatalf("unexpected events %v", got)
	}
}

// addSnapshot stores a web snapshot with the given parent and points
// branch at it.
func addSnapshot(t *testing.T, f *fixture, id, parent, branch string) {
	t.Helper()
	f.store.mu.Lock()
	f.store.snapshots[id] = domain.Snapshot{ID: id, TreeID: f.web.ID, BranchName: branch, Parents: []string{parent}, ManifestHash: f.s1.ManifestHash, CreatedAt: t0.Add(2 * time.Hour)}
	f.store.mu.Unlock()
	must(t, f.store.PutBranch(context.Background(), &domain.Branch{TreeID: f.web.ID, Name: branch, Tip: id, CreatedAt: t0, UpdatedAt: t0}))
}

// addToken stores an API token with raw value raw for username and makes
// it authenticate as that user.
func addToken(t *testing.T, f *fixture, username, raw string) domain.APIToken {
	t.Helper()
	p := f.users[username]
	tok := domain.APIToken{ID: "tok-" + raw, AccountID: p.AccountID, Name: raw, TokenHash: hashToken(raw), CreatedAt: t0}
	must(t, f.store.CreateToken(context.Background(), &tok))
	f.tokens[raw] = p
	return tok
}

func tokenIDs(f *fixture, accountID string) []string {
	toks, _ := f.store.ListTokens(context.Background(), accountID)
	var ids []string
	for _, t := range toks {
		ids = append(ids, t.ID)
	}
	slices.Sort(ids)
	return ids
}

func branchTip(want string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
	return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
		t.Helper()
		b := decode[domain.Branch](t, rec)
		stored, err := f.store.GetBranch(context.Background(), b.TreeID, b.Name)
		if err != nil {
			t.Fatalf("branch not stored: %v", err)
		}
		if b.Tip != want || stored.Tip != want {
			t.Fatalf("tip = %q (stored %q), want %q", b.Tip, stored.Tip, want)
		}
	}
}

// ---- branches ----

func TestManageCreateBranch(t *testing.T) {
	const path = "/api/v1/trees/acme/web/branches"
	cases := []endpointCase{
		{name: "default from", method: "POST", path: path, as: "alice", body: map[string]string{"name": "release/1.0"}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				branchTip("snap-2")(t, f, rec)
				if b := decode[domain.Branch](t, rec); b.Name != "release/1.0" || b.TreeID != f.web.ID || b.CreatedAt.IsZero() {
					t.Fatalf("branch = %+v", b)
				}
				hasAudit(t, f, "branch.create")
				hasEvent(t, f, domain.EventBranchCreated)
				ev := f.recBus().published[0]
				if ev.TenantID != f.acme.ID || ev.TreeID != f.web.ID || ev.Actor != "alice" {
					t.Fatalf("event = %+v", ev)
				}
			}},
		{name: "from tag", method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "v1"}, status: 201, check: branchTip("snap-1")},
		{name: "from branch", method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "feature"}, status: 201, check: branchTip("snap-2")},
		{name: "from full id", method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "snap-1"}, status: 201, check: branchTip("snap-1")},
		{name: "from unreachable full id", method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "snap-orphan"}, status: 201, check: branchTip("snap-orphan")},
		{name: "from unique prefix", setup: func(t *testing.T, f *fixture) { addSnapshot(t, f, "c0ffee42", "snap-2", "feature") },
			method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "c0ff"}, status: 201, check: branchTip("c0ffee42")},
		{name: "prefix of ancestor", setup: func(t *testing.T, f *fixture) { addSnapshot(t, f, "c0ffee42", "snap-1", "feature") },
			method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "snap-1"}, status: 201, check: branchTip("snap-1")},
		{name: "ambiguous prefix", method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "snap"}, status: 400, code: "bad_request", check: noSideEffects},
		{name: "short prefix", setup: func(t *testing.T, f *fixture) { addSnapshot(t, f, "c0ffee42", "snap-2", "feature") },
			method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "c0f"}, status: 404, code: "not_found"},
		{name: "prefix of unreachable", method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "snap-orp"}, status: 404, code: "not_found"},
		{name: "foreign snapshot", method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "snap-foreign"}, status: 404, code: "not_found"},
		{name: "unknown rev", method: "POST", path: path, as: "bob", body: map[string]string{"name": "hotfix", "from": "nope"}, status: 404, code: "not_found", check: noSideEffects},
		{name: "exists", method: "POST", path: path, as: "alice", body: map[string]string{"name": "feature"}, status: 409, code: "conflict", check: noSideEffects},
		{name: "empty default branch", method: "POST", path: "/api/v1/trees/acme/docs/branches", as: "alice", body: map[string]string{"name": "dev"}, status: 201, check: branchTip("")},
		{name: "viewer", method: "POST", path: path, as: "carol", body: map[string]string{"name": "x"}, status: 403, code: "forbidden", check: noSideEffects},
		{name: "non-member", method: "POST", path: path, as: "dave", body: map[string]string{"name": "x"}, status: 403, code: "forbidden"},
		{name: "anonymous", method: "POST", path: path, body: map[string]string{"name": "x"}, status: 401, code: "unauthenticated"},
		{name: "anonymous public tree", method: "POST", path: "/api/v1/trees/acme/docs/branches", body: map[string]string{"name": "x"}, status: 401, code: "unauthenticated"},
		{name: "unknown tree", method: "POST", path: "/api/v1/trees/acme/nope/branches", as: "alice", body: map[string]string{"name": "x"}, status: 404, code: "not_found"},
		{name: "unknown field", method: "POST", path: path, as: "alice", body: map[string]string{"name": "x", "tip": "y"}, status: 400, code: "bad_request"},
	}
	for _, bad := range []string{"", "has space", "a..b", "-x", "/x", "x/", "a//b", "x.lock", "x.", "a~b", "tab\tx", strings.Repeat("b", 201)} {
		cases = append(cases, endpointCase{name: "invalid name " + fmt.Sprintf("%.20q", bad), method: "POST", path: path, as: "alice",
			body: map[string]string{"name": bad}, status: 400, code: "bad_request", check: noSideEffects})
	}
	cases = append(cases, endpointCase{name: "max length name", method: "POST", path: path, as: "alice",
		body: map[string]string{"name": strings.Repeat("b", 200)}, status: 201})
	runManageCases(t, cases)
}

func TestManageCreateBranchRace(t *testing.T) {
	// The branch appears between the existence check and the CAS: the
	// CAS conflict must surface as 409.
	f := newManageFixture(t, func(d *Deps) {
		d.Store = &raceStore{manageStore: d.Store.(*manageStore)}
	})
	rec := f.do(t, "POST", "/api/v1/trees/acme/web/branches", "alice", map[string]string{"name": "racy"})
	if rec.Code != 409 || errCode(rec) != "conflict" {
		t.Fatalf("status = %d, body %s", rec.Code, rec.Body.String())
	}
}

// raceStore creates the branch concurrently right before the CAS.
type raceStore struct{ *manageStore }

func (s *raceStore) CompareAndSwapBranchTip(ctx context.Context, tree, name, oldTip, newTip string) error {
	if err := s.manageStore.CompareAndSwapBranchTip(ctx, tree, name, "", "snap-1"); err != nil {
		return err
	}
	return s.manageStore.CompareAndSwapBranchTip(ctx, tree, name, oldTip, newTip)
}

func TestManageDeleteBranch(t *testing.T) {
	const path = "/api/v1/trees/acme/web/branches/"
	gone := func(name string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			if _, err := f.store.GetBranch(context.Background(), f.web.ID, name); !errors.Is(err, domain.ErrNotFound) {
				t.Fatalf("branch %q still exists (%v)", name, err)
			}
			hasAudit(t, f, "branch.delete")
			hasEvent(t, f, eventBranchDeleted)
		}
	}
	nested := func(t *testing.T, f *fixture) {
		must(t, f.store.PutBranch(context.Background(), &domain.Branch{TreeID: f.web.ID, Name: "release/1.0", Tip: "snap-1"}))
	}
	keep := func(t *testing.T, f *fixture) {
		must(t, f.store.PutBranch(context.Background(), &domain.Branch{TreeID: f.web.ID, Name: "keep", Tip: "snap-1"}))
		must(t, f.store.PutProtection(context.Background(), &domain.BranchProtection{TreeID: f.web.ID, Pattern: "keep", NoDelete: true}))
	}
	runManageCases(t, []endpointCase{
		{name: "owner", method: "DELETE", path: path + "feature", as: "alice", status: 204, check: gone("feature")},
		{name: "nested name", setup: nested, method: "DELETE", path: path + "release/1.0", as: "mallory", status: 204, check: gone("release/1.0")},
		{name: "escaped slash", setup: nested, method: "DELETE", path: path + "release%2F1.0", as: "mallory", status: 204, check: gone("release/1.0")},
		{name: "default branch", method: "DELETE", path: path + "main", as: "alice", status: 403, code: "branch_protection", check: noSideEffects},
		{name: "protected", setup: keep, method: "DELETE", path: path + "keep", as: "alice", status: 403, code: "branch_protection", check: noSideEffects},
		{name: "developer", method: "DELETE", path: path + "feature", as: "bob", status: 403, code: "forbidden", check: noSideEffects},
		{name: "anonymous", method: "DELETE", path: path + "feature", status: 401, code: "unauthenticated"},
		{name: "unknown", method: "DELETE", path: path + "nope", as: "alice", status: 404, code: "not_found", check: noSideEffects},
		{name: "unknown tree", method: "DELETE", path: "/api/v1/trees/acme/nope/branches/x", as: "alice", status: 404, code: "not_found"},
	})
}

// ---- tags ----

func TestManageCreateTag(t *testing.T) {
	const path = "/api/v1/trees/acme/web/tags"
	tagTarget := func(want string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			tag := decode[domain.Tag](t, rec)
			stored, ok := f.store.tags[[2]string{f.web.ID, tag.Name}]
			if !ok || tag.Target != want || stored.Target != want {
				t.Fatalf("tag = %+v (stored %+v, %v), want target %q", tag, stored, ok, want)
			}
		}
	}
	runManageCases(t, []endpointCase{
		{name: "developer from branch", method: "POST", path: path, as: "bob", body: map[string]string{"name": "v2", "target": "main", "message": "release 2"}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				tagTarget("snap-2")(t, f, rec)
				tag := decode[domain.Tag](t, rec)
				if tag.Tagger != "bob" || tag.Message != "release 2" || tag.TreeID != f.web.ID || tag.CreatedAt.IsZero() {
					t.Fatalf("tag = %+v", tag)
				}
				hasAudit(t, f, "tag.create")
				hasEvent(t, f, domain.EventTagPushed)
			}},
		{name: "from tag", method: "POST", path: path, as: "bob", body: map[string]string{"name": "v1-again", "target": "v1"}, status: 201, check: tagTarget("snap-1")},
		{name: "from id", method: "POST", path: path, as: "bob", body: map[string]string{"name": "rel/v0", "target": "snap-1"}, status: 201, check: tagTarget("snap-1")},
		{name: "exists", method: "POST", path: path, as: "bob", body: map[string]string{"name": "v1", "target": "main"}, status: 409, code: "conflict", check: noSideEffects},
		{name: "missing target", method: "POST", path: path, as: "bob", body: map[string]string{"name": "v2"}, status: 400, code: "bad_request"},
		{name: "unknown target", method: "POST", path: path, as: "bob", body: map[string]string{"name": "v2", "target": "nope"}, status: 404, code: "not_found"},
		{name: "empty branch target", setup: func(t *testing.T, f *fixture) {
			must(t, f.store.PutBranch(context.Background(), &domain.Branch{TreeID: f.web.ID, Name: "empty"}))
		}, method: "POST", path: path, as: "bob", body: map[string]string{"name": "v2", "target": "empty"}, status: 400, code: "bad_request"},
		{name: "invalid name", method: "POST", path: path, as: "bob", body: map[string]string{"name": "v 2", "target": "main"}, status: 400, code: "bad_request"},
		{name: "lock name", method: "POST", path: path, as: "bob", body: map[string]string{"name": "v2.lock", "target": "main"}, status: 400, code: "bad_request"},
		{name: "long message", method: "POST", path: path, as: "bob", body: map[string]string{"name": "v2", "target": "main", "message": strings.Repeat("m", maxTagMessage+1)}, status: 400, code: "bad_request"},
		{name: "viewer", method: "POST", path: path, as: "carol", body: map[string]string{"name": "v2", "target": "main"}, status: 403, code: "forbidden", check: noSideEffects},
		{name: "anonymous", method: "POST", path: path, body: map[string]string{"name": "v2", "target": "main"}, status: 401, code: "unauthenticated"},
	})
}

func TestManageDeleteTag(t *testing.T) {
	const path = "/api/v1/trees/acme/web/tags/"
	gone := func(name string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			if _, ok := f.store.tags[[2]string{f.web.ID, name}]; ok {
				t.Fatalf("tag %q still exists", name)
			}
			hasAudit(t, f, "tag.delete")
			hasEvent(t, f, eventTagDeleted)
		}
	}
	runManageCases(t, []endpointCase{
		{name: "owner", method: "DELETE", path: path + "v1", as: "alice", status: 204, check: gone("v1")},
		{name: "nested", setup: func(t *testing.T, f *fixture) {
			f.store.tags[[2]string{f.web.ID, "rel/v1"}] = domain.Tag{TreeID: f.web.ID, Name: "rel/v1", Target: "snap-1"}
		}, method: "DELETE", path: path + "rel/v1", as: "mallory", status: 204, check: gone("rel/v1")},
		{name: "developer", method: "DELETE", path: path + "v1", as: "bob", status: 403, code: "forbidden", check: noSideEffects},
		{name: "anonymous", method: "DELETE", path: path + "v1", status: 401, code: "unauthenticated"},
		{name: "unknown", method: "DELETE", path: path + "v9", as: "alice", status: 404, code: "not_found", check: noSideEffects},
	})
}

// ---- tenants ----

func TestManagePatchTenant(t *testing.T) {
	const path = "/api/v1/tenants/acme"
	tenantIs := func(name, plan string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			got := decode[domain.Tenant](t, rec)
			stored := f.store.tenants[f.acme.ID]
			if got.Name != name || got.Plan != plan || stored.Name != name || stored.Plan != plan || got.Slug != "acme" {
				t.Fatalf("tenant = %+v (stored %+v), want name %q plan %q", got, stored, name, plan)
			}
			hasAudit(t, f, "tenant.update")
		}
	}
	runManageCases(t, []endpointCase{
		{name: "rename by tenant admin", method: "PATCH", path: path, as: "mallory", body: map[string]string{"name": "  Acme Corp "}, status: 200, check: tenantIs("Acme Corp", "free")},
		{name: "rename unicode 100", method: "PATCH", path: path, as: "alice", body: map[string]string{"name": strings.Repeat("é", 100)}, status: 200, check: tenantIs(strings.Repeat("é", 100), "free")},
		{name: "plan by server admin", method: "PATCH", path: path, as: "admin", body: map[string]string{"plan": "enterprise"}, status: 200, check: tenantIs("Acme", "enterprise")},
		{name: "both by server admin", method: "PATCH", path: "/api/v1/tenants/tenant-acme", as: "admin", body: map[string]string{"name": "A", "plan": "pro"}, status: 200, check: tenantIs("A", "pro")},
		{name: "plan by owner", method: "PATCH", path: path, as: "alice", body: map[string]string{"plan": "pro"}, status: 403, code: "forbidden", check: noSideEffects},
		{name: "invalid plan", method: "PATCH", path: path, as: "admin", body: map[string]string{"plan": "gold"}, status: 400, code: "bad_request"},
		{name: "empty name", method: "PATCH", path: path, as: "alice", body: map[string]string{"name": "  "}, status: 400, code: "bad_request"},
		{name: "long name", method: "PATCH", path: path, as: "alice", body: map[string]string{"name": strings.Repeat("n", 101)}, status: 400, code: "bad_request"},
		{name: "empty body", method: "PATCH", path: path, as: "alice", body: map[string]string{}, status: 400, code: "bad_request"},
		{name: "developer", method: "PATCH", path: path, as: "bob", body: map[string]string{"name": "X"}, status: 403, code: "forbidden", check: noSideEffects},
		{name: "anonymous", method: "PATCH", path: path, body: map[string]string{"name": "X"}, status: 401, code: "unauthenticated"},
		{name: "unknown tenant", method: "PATCH", path: "/api/v1/tenants/nope", as: "admin", body: map[string]string{"name": "X"}, status: 404, code: "not_found"},
	})
}

// ---- passwords ----

func TestManageChangePassword(t *testing.T) {
	const path = "/api/v1/auth/password"
	seed := func(t *testing.T, f *fixture) {
		addToken(t, f, "alice", "alice-current")
		addToken(t, f, "alice", "alice-other")
		addToken(t, f, "bob", "bob-token")
	}
	runManageCases(t, []endpointCase{
		{name: "success", setup: seed, method: "POST", path: path, as: "alice-current",
			body: map[string]string{"current_password": "alice-password", "new_password": "a brand new password"}, status: 204,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if got := f.manageAuth().password("alice"); got != "a brand new password" {
					t.Fatalf("password = %q", got)
				}
				// Only the token of this request survives (the session token
				// issued while verifying the password is gone too).
				if got := tokenIDs(f, "acct-alice"); !slices.Equal(got, []string{"tok-alice-current"}) {
					t.Fatalf("alice tokens = %v", got)
				}
				if got := tokenIDs(f, "acct-bob"); !slices.Equal(got, []string{"tok-bob-token"}) {
					t.Fatalf("bob tokens = %v", got)
				}
				hasAudit(t, f, "auth.password_change")
			}},
		{name: "wrong current", setup: seed, method: "POST", path: path, as: "alice-current",
			body: map[string]string{"current_password": "guess", "new_password": "a brand new password"}, status: 403, code: "forbidden",
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if got := f.manageAuth().password("alice"); got != "alice-password" {
					t.Fatalf("password changed to %q", got)
				}
				if got := tokenIDs(f, "acct-alice"); len(got) != 2 {
					t.Fatalf("alice tokens = %v", got)
				}
				hasAudit(t, f, "auth.password_change_failed")
			}},
		{name: "weak new password", setup: seed, method: "POST", path: path, as: "alice-current",
			body: map[string]string{"current_password": "alice-password", "new_password": "short"}, status: 400, code: "bad_request",
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if got := tokenIDs(f, "acct-alice"); len(got) != 2 {
					t.Fatalf("alice tokens = %v", got)
				}
			}},
		{name: "missing current", method: "POST", path: path, as: "alice", body: map[string]string{"new_password": "a brand new password"}, status: 400, code: "bad_request"},
		{name: "anonymous", method: "POST", path: path, body: map[string]string{"current_password": "x", "new_password": "a brand new password"}, status: 401, code: "unauthenticated"},
	})
}

func TestManageAdminSetPassword(t *testing.T) {
	const path = "/api/v1/admin/accounts/bob/password"
	seed := func(t *testing.T, f *fixture) {
		addToken(t, f, "bob", "bob-1")
		addToken(t, f, "bob", "bob-2")
		addToken(t, f, "admin", "admin-current")
		addToken(t, f, "admin", "admin-other")
	}
	runManageCases(t, []endpointCase{
		{name: "reset other", setup: seed, method: "POST", path: path, as: "admin-current", body: map[string]string{"password": "reset password 1"}, status: 204,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if got := f.manageAuth().password("bob"); got != "reset password 1" {
					t.Fatalf("password = %q", got)
				}
				if got := tokenIDs(f, "acct-bob"); len(got) != 0 {
					t.Fatalf("bob tokens = %v", got)
				}
				if got := tokenIDs(f, "acct-admin"); len(got) != 2 {
					t.Fatalf("admin tokens = %v", got)
				}
				hasAudit(t, f, "account.password_reset")
			}},
		{name: "reset self keeps current token", setup: seed, method: "POST", path: "/api/v1/admin/accounts/admin/password", as: "admin-current",
			body: map[string]string{"password": "reset password 1"}, status: 204,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if got := tokenIDs(f, "acct-admin"); !slices.Equal(got, []string{"tok-admin-current"}) {
					t.Fatalf("admin tokens = %v", got)
				}
			}},
		{name: "weak", setup: seed, method: "POST", path: path, as: "admin", body: map[string]string{"password": "short"}, status: 400, code: "bad_request",
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if got := tokenIDs(f, "acct-bob"); len(got) != 2 {
					t.Fatalf("bob tokens = %v", got)
				}
			}},
		{name: "unknown account", method: "POST", path: "/api/v1/admin/accounts/zed/password", as: "admin", body: map[string]string{"password": "reset password 1"}, status: 404, code: "not_found"},
		{name: "non-admin", method: "POST", path: path, as: "alice", body: map[string]string{"password": "reset password 1"}, status: 403, code: "forbidden", check: noSideEffects},
		{name: "anonymous", method: "POST", path: path, body: map[string]string{"password": "reset password 1"}, status: 401, code: "unauthenticated"},
	})
}

// ---- admin accounts ----

func TestManageAdminCreateAccount(t *testing.T) {
	const path = "/api/v1/admin/accounts"
	body := func(username, email, password string, admin bool) map[string]any {
		return map[string]any{"username": username, "email": email, "password": password, "is_admin": admin}
	}
	created := func(username string, admin bool) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			got := decode[domain.Account](t, rec)
			stored, err := f.store.GetAccountByUsername(context.Background(), username)
			if err != nil || got.Username != username || got.IsAdmin != admin || stored.IsAdmin != admin || got.ID != stored.ID {
				t.Fatalf("account = %+v, stored %+v (%v)", got, stored, err)
			}
			if raw := decode[map[string]any](t, rec); raw["password_hash"] != nil || raw["PasswordHash"] != nil {
				t.Fatalf("password hash leaked: %v", raw)
			}
			if f.manageAuth().created != 1 {
				t.Fatalf("CreateAccount calls = %d", f.manageAuth().created)
			}
			hasAudit(t, f, "account.create")
		}
	}
	closed := func(t *testing.T, f *fixture) { f.auth.regOpen = false }
	runManageCases(t, []endpointCase{
		{name: "create", setup: closed, method: "POST", path: path, as: "admin", body: body(" Erin ", "erin@example.com", "erin password", false), status: 201, check: created("erin", false)},
		{name: "create admin", setup: closed, method: "POST", path: path, as: "admin", body: body("root2", "root2@example.com", "root password", true), status: 201, check: created("root2", true)},
		{name: "duplicate username", method: "POST", path: path, as: "admin", body: body("bob", "bob2@example.com", "bob password", false), status: 409, code: "conflict"},
		{name: "duplicate email", method: "POST", path: path, as: "admin", body: body("bobby", "BOB@example.com", "bob password", false), status: 409, code: "conflict"},
		{name: "invalid username", method: "POST", path: path, as: "admin", body: body("x", "x@example.com", "x password", false), status: 400, code: "bad_request"},
		{name: "reserved username", method: "POST", path: path, as: "admin", body: body("system", "s@example.com", "s password", false), status: 400, code: "bad_request"},
		{name: "invalid email", method: "POST", path: path, as: "admin", body: body("erin", "erin", "erin password", false), status: 400, code: "bad_request"},
		{name: "weak password", method: "POST", path: path, as: "admin", body: body("erin", "erin@example.com", "short", false), status: 400, code: "bad_request"},
		{name: "non-admin", method: "POST", path: path, as: "alice", body: body("erin", "erin@example.com", "erin password", false), status: 403, code: "forbidden", check: noSideEffects},
		{name: "anonymous", method: "POST", path: path, body: body("erin", "erin@example.com", "erin password", false), status: 401, code: "unauthenticated"},
	})
}

func TestManageAdminCreateAccountFallback(t *testing.T) {
	// Without accountCreator the handler registers and then promotes.
	f := newManageFixture(t, func(d *Deps) { d.Auth = d.Auth.(*manageAuth).fakeAuth })
	rec := f.do(t, "POST", "/api/v1/admin/accounts", "admin",
		map[string]any{"username": "erin", "email": "erin@example.com", "password": "erin password", "is_admin": true})
	if rec.Code != 201 {
		t.Fatalf("status = %d, body %s", rec.Code, rec.Body.String())
	}
	stored, err := f.store.GetAccountByUsername(context.Background(), "erin")
	if err != nil || !stored.IsAdmin || !decode[domain.Account](t, rec).IsAdmin {
		t.Fatalf("stored = %+v, %v", stored, err)
	}
}

// ---- system ----

func TestManageAdminSystem(t *testing.T) {
	dir := t.TempDir()
	must(t, os.WriteFile(filepath.Join(dir, "meta.db"), make([]byte, 1234), 0o600))
	bus := events.NewBus()
	f := newManageFixture(t, func(d *Deps) {
		d.AllowRegistration = true
		d.DataDir = dir
		d.Bus = bus
	})
	bus.Publish(domain.Event{Type: domain.EventTreeCreated})
	_, cancel := bus.Subscribe(domain.EventFilter{})
	defer cancel()

	rec := f.do(t, "GET", "/api/v1/admin/system", "admin", nil)
	if rec.Code != 200 {
		t.Fatalf("status = %d, body %s", rec.Code, rec.Body.String())
	}
	info := decode[systemInfo](t, rec)
	n, b, _ := f.objs.Stats(context.Background())
	abs, _ := filepath.Abs(dir)
	switch {
	case info.Version != "test-1.0", info.GoVersion != runtime.Version(), info.OS != runtime.GOOS, info.Arch != runtime.GOARCH:
		t.Fatalf("build info = %+v", info)
	case !info.AllowRegistration, info.DataDir != abs:
		t.Fatalf("config = %v %q", info.AllowRegistration, info.DataDir)
	case info.UptimeSeconds < 90, info.StartedAt.IsZero(), info.StartedAt.Location() != time.UTC:
		t.Fatalf("uptime = %d, started %v", info.UptimeSeconds, info.StartedAt)
	case info.NumCPU < 1, info.NumGoroutine < 1, info.Memory.SysBytes == 0, info.Memory.AllocBytes == 0:
		t.Fatalf("runtime = %+v", info)
	case info.Storage != systemStorage{Objects: n, ObjectBytes: b, MetaDBBytes: 1234}:
		t.Fatalf("storage = %+v", info.Storage)
	case info.EventBus.Published != 1, info.EventBus.Subscribers != 1:
		t.Fatalf("event bus = %+v", info.EventBus)
	}
	// Field names are the contract of apps/admin's SystemInfo.
	raw := decode[map[string]any](t, rec)
	for _, k := range []string{"version", "go_version", "os", "arch", "started_at", "uptime_seconds", "allow_registration",
		"data_dir", "num_cpu", "num_goroutine", "memory", "storage", "event_bus"} {
		if _, ok := raw[k]; !ok {
			t.Errorf("missing field %q", k)
		}
	}
	for obj, keys := range map[string][]string{
		"memory":    {"alloc_bytes", "sys_bytes", "heap_inuse_bytes", "gc_runs"},
		"storage":   {"objects", "object_bytes", "meta_db_bytes"},
		"event_bus": {"published", "delivered", "dropped", "subscribers"},
	} {
		m, _ := raw[obj].(map[string]any)
		for _, k := range keys {
			if _, ok := m[k]; !ok {
				t.Errorf("missing field %s.%s", obj, k)
			}
		}
	}
}

func TestManageAdminSystemDefaults(t *testing.T) {
	// No data dir, a bus without stats: zero values, still 200.
	runManageCases(t, []endpointCase{
		{name: "defaults", method: "GET", path: "/api/v1/admin/system", as: "admin", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				info := decode[systemInfo](t, rec)
				if info.DataDir != "" || info.Storage.MetaDBBytes != 0 || info.EventBus != (events.Stats{}) || info.AllowRegistration {
					t.Fatalf("info = %+v", info)
				}
			}},
		{name: "non-admin", method: "GET", path: "/api/v1/admin/system", as: "alice", status: 403, code: "forbidden"},
		{name: "anonymous", method: "GET", path: "/api/v1/admin/system", status: 401, code: "unauthenticated"},
	})
}

// ---- search ----

func TestManageSearch(t *testing.T) {
	type hits struct {
		tenants, trees, branches, accounts []string
	}
	expect := func(want hits) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			t.Helper()
			res := decode[searchResults](t, rec)
			var got hits
			for _, x := range res.Tenants {
				got.tenants = append(got.tenants, x.Slug)
			}
			for _, x := range res.Trees {
				got.trees = append(got.trees, x.TenantSlug+"/"+x.Tree.Name)
			}
			for _, x := range res.Branches {
				got.branches = append(got.branches, x.TenantSlug+"/"+x.Tree+"@"+x.Branch.Name)
			}
			for _, x := range res.Accounts {
				got.accounts = append(got.accounts, x.Username)
			}
			if !slices.Equal(got.tenants, want.tenants) || !slices.Equal(got.trees, want.trees) ||
				!slices.Equal(got.branches, want.branches) || !slices.Equal(got.accounts, want.accounts) {
				t.Fatalf("hits = %+v, want %+v", got, want)
			}
			raw := decode[map[string]any](t, rec)
			for _, k := range []string{"tenants", "trees", "branches", "accounts"} {
				if _, ok := raw[k].([]any); !ok {
					t.Fatalf("%s = %v, want a JSON array", k, raw[k])
				}
			}
		}
	}
	describe := func(t *testing.T, f *fixture) {
		web := f.store.trees[f.web.ID]
		web.Description = "The Marketing website"
		must(t, f.store.UpdateTree(context.Background(), &web))
	}
	const path = "/api/v1/search?q="
	runManageCases(t, []endpointCase{
		{name: "tenant by slug", method: "GET", path: path + "acme", as: "admin", status: 200, check: expect(hits{tenants: []string{"acme"}})},
		{name: "tenant by name case-insensitive", method: "GET", path: path + "ACM", as: "bob", status: 200, check: expect(hits{tenants: []string{"acme"}})},
		{name: "tree and branch", method: "GET", path: path + "MAIN", as: "alice", status: 200, check: expect(hits{branches: []string{"acme/web@main"}})},
		{name: "tree by name", method: "GET", path: path + "web", as: "carol", status: 200, check: expect(hits{trees: []string{"acme/web"}})},
		{name: "tree by description", setup: describe, method: "GET", path: path + "marketing", as: "bob", status: 200, check: expect(hits{trees: []string{"acme/web"}})},
		{name: "non-member sees nothing of acme", method: "GET", path: path + "web", as: "dave", status: 200, check: expect(hits{})},
		{name: "non-member own tenant", method: "GET", path: path + "dav", as: "dave", status: 200, check: expect(hits{tenants: []string{"dave"}})},
		{name: "accounts for admins", method: "GET", path: path + "example.com&limit=2", as: "admin", status: 200, check: expect(hits{accounts: []string{"admin", "alice"}})},
		{name: "accounts hidden for others", method: "GET", path: path + "example.com", as: "alice", status: 200, check: expect(hits{})},
		{name: "limit per category", method: "GET", path: path + "a&limit=1", as: "admin", status: 200,
			check: expect(hits{tenants: []string{"acme"}, branches: []string{"acme/web@feature"}, accounts: []string{"admin"}})},
		{name: "limit clamped", method: "GET", path: path + "e&limit=500", as: "admin", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if n := len(decode[searchResults](t, rec).Accounts); n != 6 {
					t.Fatalf("accounts = %d", n)
				}
			}},
		{name: "missing q", method: "GET", path: "/api/v1/search", as: "alice", status: 400, code: "bad_request"},
		{name: "blank q", method: "GET", path: path + "%20%20", as: "alice", status: 400, code: "bad_request"},
		{name: "long q", method: "GET", path: path + strings.Repeat("q", 101), as: "alice", status: 400, code: "bad_request"},
		{name: "bad limit", method: "GET", path: path + "a&limit=0", as: "alice", status: 400, code: "bad_request"},
		{name: "non-numeric limit", method: "GET", path: path + "a&limit=x", as: "alice", status: 400, code: "bad_request"},
		{name: "anonymous", method: "GET", path: path + "a", status: 401, code: "unauthenticated"},
	})
}

func TestValidateRefNameManage(t *testing.T) {
	for _, tc := range []struct {
		name string
		ok   bool
	}{
		{"main", true}, {"feature/x-1", true}, {"release/1.0", true}, {"v1.2.3", true}, {"ünïcode", true},
		{"", false}, {"a b", false}, {"a..b", false}, {"-a", false}, {"/a", false}, {"a/", false}, {"a//b", false},
		{"a.lock", false}, {"a.", false}, {"a:b", false}, {"a\x00b", false}, {strings.Repeat("x", 201), false},
		{string([]byte{0xff}), false},
	} {
		err := validateRefNameManage("tag", tc.name)
		if (err == nil) != tc.ok {
			t.Errorf("validateRefNameManage(%q) = %v, want ok=%v", tc.name, err, tc.ok)
		}
		if err != nil && !strings.Contains(err.Error(), "tag name") {
			t.Errorf("error %q does not name the ref kind", err)
		}
	}
}
