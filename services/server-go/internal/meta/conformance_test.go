package meta

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"slices"
	"strconv"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// storeFactory returns a fresh, empty store. It must register cleanup.
type storeFactory func(t *testing.T) domain.MetaStore

// base is a fixed reference instant for deterministic timestamps.
var base = time.Date(2026, 3, 14, 15, 9, 26, 535897932, time.UTC)

func at(sec int) time.Time { return base.Add(time.Duration(sec) * time.Second) }

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

func wantErr(t *testing.T, err, target error) {
	t.Helper()
	if !errors.Is(err, target) {
		t.Fatalf("err = %v, want errors.Is(err, %v)", err, target)
	}
}

func wantEqual[T any](t *testing.T, what string, got, want T) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("%s:\n got  %+v\n want %+v", what, got, want)
	}
}

// runConformance runs the full MetaStore contract suite against stores
// produced by newStore.
func runConformance(t *testing.T, newStore storeFactory) {
	tests := []struct {
		name string
		fn   func(t *testing.T, s domain.MetaStore)
	}{
		{"Tenants", testTenants},
		{"Accounts", testAccounts},
		{"Memberships", testMemberships},
		{"Tokens", testTokens},
		{"Trees", testTrees},
		{"DeleteTreeCascades", testDeleteTreeCascades},
		{"Branches", testBranches},
		{"CompareAndSwap", testCAS},
		{"Snapshots", testSnapshots},
		{"Tags", testTags},
		{"Staged", testStaged},
		{"Protections", testProtections},
		{"MergeRequests", testMergeRequests},
		{"Audit", testAudit},
		{"LicenseRules", testLicenseRules},
		{"AccessConfig", testAccessConfig},
		{"InvalidArguments", testInvalidArguments},
		{"EmptyListsNonNil", testEmptyListsNonNil},
		{"NoAliasing", testNoAliasing},
		{"CanceledContext", testCanceledContext},
		{"Close", testClose},
		{"ConcurrentCASOneWinner", testConcurrentCASOneWinner},
		{"ConcurrentCASChain", testConcurrentCASChain},
		{"ConcurrentMRNumbers", testConcurrentMRNumbers},
		{"ConcurrentUniqueSlug", testConcurrentUniqueSlug},
		{"ConcurrentMixed", testConcurrentMixed},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			tc.fn(t, newStore(t))
		})
	}
	t.Run("History", func(t *testing.T) { runHistoryConformance(t, newStore) })
}

func testTenants(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	acme := &domain.Tenant{Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, Plan: "pro"}
	must(t, s.CreateTenant(ctx, acme))
	if acme.ID == "" || acme.CreatedAt.IsZero() {
		t.Fatalf("CreateTenant did not fill ID/CreatedAt: %+v", acme)
	}
	bob := &domain.Tenant{ID: "t-bob", Slug: "bob", Name: "Bob", Kind: domain.TenantUser, CreatedAt: at(1)}
	must(t, s.CreateTenant(ctx, bob))
	if bob.ID != "t-bob" || !bob.CreatedAt.Equal(at(1)) {
		t.Fatalf("CreateTenant overwrote caller values: %+v", bob)
	}

	wantErr(t, s.CreateTenant(ctx, &domain.Tenant{Slug: "acme"}), domain.ErrAlreadyExists)
	wantErr(t, s.CreateTenant(ctx, &domain.Tenant{ID: "t-bob", Slug: "other"}), domain.ErrAlreadyExists)

	got, err := s.GetTenant(ctx, acme.ID)
	must(t, err)
	wantEqual(t, "GetTenant", *got, *acme)
	got, err = s.GetTenantBySlug(ctx, "bob")
	must(t, err)
	wantEqual(t, "GetTenantBySlug", *got, *bob)
	_, err = s.GetTenant(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetTenantBySlug(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)

	must(t, s.CreateTenant(ctx, &domain.Tenant{Slug: "aardvark"}))
	list, err := s.ListTenants(ctx)
	must(t, err)
	slugs := []string{}
	for _, tn := range list {
		slugs = append(slugs, tn.Slug)
	}
	wantEqual(t, "tenant order", slugs, []string{"aardvark", "acme", "bob"})

	// Update: rename slug, keep CreatedAt when zero.
	upd := &domain.Tenant{ID: bob.ID, Slug: "robert", Name: "Robert", Kind: domain.TenantUser, Plan: "enterprise"}
	must(t, s.UpdateTenant(ctx, upd))
	got, err = s.GetTenantBySlug(ctx, "robert")
	must(t, err)
	if got.Name != "Robert" || got.Plan != "enterprise" || !got.CreatedAt.Equal(at(1)) {
		t.Fatalf("UpdateTenant result: %+v", got)
	}
	_, err = s.GetTenantBySlug(ctx, "bob")
	wantErr(t, err, domain.ErrNotFound)
	// The old slug is free again.
	must(t, s.CreateTenant(ctx, &domain.Tenant{Slug: "bob"}))
	// Same slug as itself is fine; another tenant's slug is not.
	must(t, s.UpdateTenant(ctx, got))
	got.Slug = "acme"
	wantErr(t, s.UpdateTenant(ctx, got), domain.ErrAlreadyExists)
	wantErr(t, s.UpdateTenant(ctx, &domain.Tenant{ID: "missing", Slug: "x"}), domain.ErrNotFound)
}

func testAccounts(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	alice := &domain.Account{Username: "alice", Email: "a@example.com", DisplayName: "Alice",
		PasswordHash: "$argon2id$secret", TenantID: "t-alice", IsAdmin: true}
	must(t, s.CreateAccount(ctx, alice))
	if alice.ID == "" || alice.CreatedAt.IsZero() {
		t.Fatalf("CreateAccount did not fill ID/CreatedAt: %+v", alice)
	}
	wantErr(t, s.CreateAccount(ctx, &domain.Account{Username: "alice"}), domain.ErrAlreadyExists)
	wantErr(t, s.CreateAccount(ctx, &domain.Account{ID: alice.ID, Username: "alice2"}), domain.ErrAlreadyExists)

	got, err := s.GetAccount(ctx, alice.ID)
	must(t, err)
	wantEqual(t, "GetAccount", *got, *alice)
	if got.PasswordHash != "$argon2id$secret" {
		t.Fatalf("PasswordHash not persisted: %q", got.PasswordHash)
	}
	got, err = s.GetAccountByUsername(ctx, "alice")
	must(t, err)
	wantEqual(t, "GetAccountByUsername", *got, *alice)
	_, err = s.GetAccount(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetAccountByUsername(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)

	must(t, s.CreateAccount(ctx, &domain.Account{Username: "zed"}))
	must(t, s.CreateAccount(ctx, &domain.Account{Username: "bob"}))
	list, err := s.ListAccounts(ctx)
	must(t, err)
	names := []string{}
	for _, a := range list {
		names = append(names, a.Username)
	}
	wantEqual(t, "account order", names, []string{"alice", "bob", "zed"})
	if list[0].PasswordHash == "" {
		t.Fatal("ListAccounts dropped PasswordHash")
	}

	got.Disabled = true
	got.PasswordHash = "new-hash"
	got.Username = "alicia"
	must(t, s.UpdateAccount(ctx, got))
	got2, err := s.GetAccountByUsername(ctx, "alicia")
	must(t, err)
	if !got2.Disabled || got2.PasswordHash != "new-hash" || got2.ID != alice.ID {
		t.Fatalf("UpdateAccount result: %+v", got2)
	}
	_, err = s.GetAccountByUsername(ctx, "alice")
	wantErr(t, err, domain.ErrNotFound)
	got2.Username = "bob"
	wantErr(t, s.UpdateAccount(ctx, got2), domain.ErrAlreadyExists)
	wantErr(t, s.UpdateAccount(ctx, &domain.Account{ID: "missing", Username: "m"}), domain.ErrNotFound)
}

func testMemberships(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	m1 := &domain.Membership{TenantID: "t1", AccountID: "a2", Role: domain.RoleOwner}
	must(t, s.PutMembership(ctx, m1))
	if m1.CreatedAt.IsZero() {
		t.Fatal("PutMembership did not fill CreatedAt")
	}
	must(t, s.PutMembership(ctx, &domain.Membership{TenantID: "t1", AccountID: "a1", Role: domain.RoleViewer, CreatedAt: at(5)}))
	must(t, s.PutMembership(ctx, &domain.Membership{TenantID: "t2", AccountID: "a2", Role: domain.RoleDeveloper, CreatedAt: at(6)}))

	got, err := s.GetMembership(ctx, "t1", "a2")
	must(t, err)
	wantEqual(t, "GetMembership", *got, *m1)
	_, err = s.GetMembership(ctx, "t1", "a3")
	wantErr(t, err, domain.ErrNotFound)

	// Upsert keeps the original CreatedAt.
	must(t, s.PutMembership(ctx, &domain.Membership{TenantID: "t1", AccountID: "a2", Role: domain.RoleAdmin}))
	got, err = s.GetMembership(ctx, "t1", "a2")
	must(t, err)
	if got.Role != domain.RoleAdmin || !got.CreatedAt.Equal(m1.CreatedAt) {
		t.Fatalf("upsert: %+v (want role admin, created %v)", got, m1.CreatedAt)
	}

	members, err := s.ListTenantMembers(ctx, "t1")
	must(t, err)
	if len(members) != 2 || members[0].AccountID != "a1" || members[1].AccountID != "a2" {
		t.Fatalf("ListTenantMembers: %+v", members)
	}
	ms, err := s.ListAccountMemberships(ctx, "a2")
	must(t, err)
	if len(ms) != 2 || ms[0].TenantID != "t1" || ms[1].TenantID != "t2" || ms[1].Role != domain.RoleDeveloper {
		t.Fatalf("ListAccountMemberships: %+v", ms)
	}

	must(t, s.DeleteMembership(ctx, "t1", "a2"))
	wantErr(t, s.DeleteMembership(ctx, "t1", "a2"), domain.ErrNotFound)
	_, err = s.GetMembership(ctx, "t1", "a2")
	wantErr(t, err, domain.ErrNotFound)
	ms, err = s.ListAccountMemberships(ctx, "a2")
	must(t, err)
	if len(ms) != 1 || ms[0].TenantID != "t2" {
		t.Fatalf("after delete ListAccountMemberships: %+v", ms)
	}
	members, err = s.ListTenantMembers(ctx, "t1")
	must(t, err)
	if len(members) != 1 || members[0].AccountID != "a1" {
		t.Fatalf("after delete ListTenantMembers: %+v", members)
	}
}

func testTokens(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	exp := at(3600)
	t1 := &domain.APIToken{AccountID: "a1", Name: "laptop", TokenHash: "h1", CreatedAt: at(2), ExpiresAt: &exp}
	must(t, s.CreateToken(ctx, t1))
	if t1.ID == "" {
		t.Fatal("CreateToken did not fill ID")
	}
	t2 := &domain.APIToken{AccountID: "a1", Name: "ci", TokenHash: "h2", CreatedAt: at(1)}
	must(t, s.CreateToken(ctx, t2))
	must(t, s.CreateToken(ctx, &domain.APIToken{AccountID: "a2", Name: "other", TokenHash: "h3"}))
	wantErr(t, s.CreateToken(ctx, &domain.APIToken{AccountID: "a9", TokenHash: "h1"}), domain.ErrAlreadyExists)
	wantErr(t, s.CreateToken(ctx, &domain.APIToken{ID: t1.ID, AccountID: "a9", TokenHash: "h9"}), domain.ErrAlreadyExists)

	got, err := s.GetTokenByHash(ctx, "h1")
	must(t, err)
	wantEqual(t, "GetTokenByHash", *got, *t1)
	if got.TokenHash != "h1" || got.ExpiresAt == nil || !got.ExpiresAt.Equal(exp) {
		t.Fatalf("token fields not persisted: %+v", got)
	}
	_, err = s.GetTokenByHash(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)

	list, err := s.ListTokens(ctx, "a1")
	must(t, err)
	if len(list) != 2 || list[0].ID != t2.ID || list[1].ID != t1.ID {
		t.Fatalf("ListTokens order (by created_at): %+v", list)
	}

	used := time.Date(2026, 5, 1, 12, 0, 0, 0, time.FixedZone("x", 3600))
	must(t, s.TouchToken(ctx, t1.ID, used))
	got, err = s.GetTokenByHash(ctx, "h1")
	must(t, err)
	if got.LastUsedAt == nil || !got.LastUsedAt.Equal(used) || got.LastUsedAt.Location() != time.UTC {
		t.Fatalf("TouchToken: LastUsedAt = %v", got.LastUsedAt)
	}
	wantErr(t, s.TouchToken(ctx, "missing", used), domain.ErrNotFound)

	must(t, s.DeleteToken(ctx, t1.ID))
	wantErr(t, s.DeleteToken(ctx, t1.ID), domain.ErrNotFound)
	_, err = s.GetTokenByHash(ctx, "h1")
	wantErr(t, err, domain.ErrNotFound)
	list, err = s.ListTokens(ctx, "a1")
	must(t, err)
	if len(list) != 1 || list[0].ID != t2.ID {
		t.Fatalf("ListTokens after delete: %+v", list)
	}
	// The hash can be reused after deletion.
	must(t, s.CreateToken(ctx, &domain.APIToken{AccountID: "a1", TokenHash: "h1"}))
}

func testTrees(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	tr := &domain.Tree{TenantID: "t1", Name: "web", Description: "site", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	must(t, s.CreateTree(ctx, tr))
	if tr.ID == "" || tr.CreatedAt.IsZero() || !tr.UpdatedAt.Equal(tr.CreatedAt) {
		t.Fatalf("CreateTree defaults: %+v", tr)
	}
	wantErr(t, s.CreateTree(ctx, &domain.Tree{TenantID: "t1", Name: "web"}), domain.ErrAlreadyExists)
	wantErr(t, s.CreateTree(ctx, &domain.Tree{ID: tr.ID, TenantID: "t9", Name: "x"}), domain.ErrAlreadyExists)
	must(t, s.CreateTree(ctx, &domain.Tree{TenantID: "t2", Name: "web"}))
	must(t, s.CreateTree(ctx, &domain.Tree{TenantID: "t1", Name: "api"}))
	must(t, s.CreateTree(ctx, &domain.Tree{TenantID: "t2", Name: "zeta"}))

	got, err := s.GetTree(ctx, tr.ID)
	must(t, err)
	wantEqual(t, "GetTree", *got, *tr)
	got, err = s.GetTreeByName(ctx, "t1", "web")
	must(t, err)
	wantEqual(t, "GetTreeByName", *got, *tr)
	_, err = s.GetTree(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetTreeByName(ctx, "t3", "web")
	wantErr(t, err, domain.ErrNotFound)

	names := func(ts []domain.Tree) []string {
		out := []string{}
		for _, x := range ts {
			out = append(out, x.TenantID+"/"+x.Name)
		}
		return out
	}
	list, err := s.ListTrees(ctx, "t1")
	must(t, err)
	wantEqual(t, "ListTrees(t1)", names(list), []string{"t1/api", "t1/web"})
	list, err = s.ListTrees(ctx, "")
	must(t, err)
	wantEqual(t, "ListTrees(all)", names(list), []string{"t1/api", "t1/web", "t2/web", "t2/zeta"})

	// Update: rename, UpdatedAt advances, CreatedAt preserved.
	upd := *tr
	upd.Name = "website"
	upd.Visibility = domain.VisibilityPublic
	upd.CreatedAt = time.Time{}
	time.Sleep(2 * time.Millisecond)
	must(t, s.UpdateTree(ctx, &upd))
	got, err = s.GetTreeByName(ctx, "t1", "website")
	must(t, err)
	if got.Visibility != domain.VisibilityPublic || !got.CreatedAt.Equal(tr.CreatedAt) || !got.UpdatedAt.After(tr.UpdatedAt) {
		t.Fatalf("UpdateTree result: %+v (orig %+v)", got, tr)
	}
	wantEqual(t, "UpdateTree writes back", *got, upd)
	_, err = s.GetTreeByName(ctx, "t1", "web")
	wantErr(t, err, domain.ErrNotFound)
	upd.Name = "api"
	wantErr(t, s.UpdateTree(ctx, &upd), domain.ErrAlreadyExists)
	wantErr(t, s.UpdateTree(ctx, &domain.Tree{ID: "missing", TenantID: "t1", Name: "m"}), domain.ErrNotFound)

	must(t, s.DeleteTree(ctx, tr.ID))
	wantErr(t, s.DeleteTree(ctx, tr.ID), domain.ErrNotFound)
	_, err = s.GetTree(ctx, tr.ID)
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetTreeByName(ctx, "t1", "website")
	wantErr(t, err, domain.ErrNotFound)
	// The name is free again.
	must(t, s.CreateTree(ctx, &domain.Tree{TenantID: "t1", Name: "website"}))
}

func testDeleteTreeCascades(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	doomed := &domain.Tree{TenantID: "t1", Name: "doomed"}
	keep := &domain.Tree{TenantID: "t1", Name: "keep"}
	must(t, s.CreateTree(ctx, doomed))
	must(t, s.CreateTree(ctx, keep))
	for _, tr := range []*domain.Tree{doomed, keep} {
		must(t, s.CompareAndSwapBranchTip(ctx, tr.ID, "main", "", "s-"+tr.Name))
		must(t, s.PutSnapshot(ctx, &domain.Snapshot{ID: "s-" + tr.Name, TreeID: tr.ID}))
		must(t, s.PutSnapshot(ctx, &domain.Snapshot{ID: "s2-" + tr.Name, TreeID: tr.ID}))
		must(t, s.PutTag(ctx, &domain.Tag{TreeID: tr.ID, Name: "v1", Target: "s-" + tr.Name}))
		must(t, s.PutStaged(ctx, &domain.StagedSnapshot{TreeID: tr.ID, AccountID: "a1", BranchName: "main"}))
		must(t, s.PutProtection(ctx, &domain.BranchProtection{TreeID: tr.ID, Pattern: "main"}))
		must(t, s.CreateMergeRequest(ctx, &domain.MergeRequest{TreeID: tr.ID, Title: "x"}))
		must(t, s.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: tr.ID, PathGlob: "**", SPDX: "MIT"}))
	}
	must(t, s.DeleteTree(ctx, doomed.ID))

	if n, err := s.CountSnapshots(ctx, doomed.ID); err != nil || n != 0 {
		t.Fatalf("CountSnapshots(doomed) = %d, %v", n, err)
	}
	if n, err := s.CountSnapshots(ctx, ""); err != nil || n != 2 {
		t.Fatalf("CountSnapshots(all) = %d, %v; want 2", n, err)
	}
	if ok, _ := s.HasSnapshot(ctx, "s-doomed"); ok {
		t.Fatal("snapshot of deleted tree survived")
	}
	if n, err := s.CountStaged(ctx); err != nil || n != 1 {
		t.Fatalf("CountStaged = %d, %v; want 1", n, err)
	}
	checkEmpty := func(name string, n int, err error) {
		t.Helper()
		must(t, err)
		if n != 0 {
			t.Fatalf("%s of deleted tree: %d entries remain", name, n)
		}
	}
	br, err := s.ListBranches(ctx, doomed.ID)
	checkEmpty("branches", len(br), err)
	tg, err := s.ListTags(ctx, doomed.ID)
	checkEmpty("tags", len(tg), err)
	st, err := s.ListStaged(ctx, doomed.ID, "")
	checkEmpty("staged", len(st), err)
	pr, err := s.ListProtections(ctx, doomed.ID)
	checkEmpty("protections", len(pr), err)
	mr, err := s.ListMergeRequests(ctx, doomed.ID, "")
	checkEmpty("merge requests", len(mr), err)
	lr, err := s.ListLicenseRules(ctx, doomed.ID)
	checkEmpty("license rules", len(lr), err)

	// The surviving tree is untouched.
	for name, n := range map[string]func() (int, error){
		"branches":    func() (int, error) { x, err := s.ListBranches(ctx, keep.ID); return len(x), err },
		"tags":        func() (int, error) { x, err := s.ListTags(ctx, keep.ID); return len(x), err },
		"staged":      func() (int, error) { x, err := s.ListStaged(ctx, keep.ID, ""); return len(x), err },
		"protections": func() (int, error) { x, err := s.ListProtections(ctx, keep.ID); return len(x), err },
		"mrs":         func() (int, error) { x, err := s.ListMergeRequests(ctx, keep.ID, ""); return len(x), err },
		"licenses":    func() (int, error) { x, err := s.ListLicenseRules(ctx, keep.ID); return len(x), err },
		"snapshots":   func() (int, error) { return s.CountSnapshots(ctx, keep.ID) },
	} {
		got, err := n()
		must(t, err)
		want := 1
		if name == "snapshots" {
			want = 2
		}
		if got != want {
			t.Fatalf("surviving tree %s = %d, want %d", name, got, want)
		}
	}
	// MR numbering continues for the surviving tree.
	next := &domain.MergeRequest{TreeID: keep.ID}
	must(t, s.CreateMergeRequest(ctx, next))
	if next.Number != 2 {
		t.Fatalf("next MR number = %d, want 2", next.Number)
	}
}

func testBranches(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	b := &domain.Branch{TreeID: "tr", Name: "main", Tip: "s1", Protected: true, CreatedAt: at(1)}
	must(t, s.PutBranch(ctx, b))
	if !b.CreatedAt.Equal(at(1)) || b.UpdatedAt.IsZero() {
		t.Fatalf("PutBranch timestamps: %+v", b)
	}
	got, err := s.GetBranch(ctx, "tr", "main")
	must(t, err)
	wantEqual(t, "GetBranch", *got, *b)
	_, err = s.GetBranch(ctx, "tr", "dev")
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetBranch(ctx, "other", "main")
	wantErr(t, err, domain.ErrNotFound)

	// Replace without CreatedAt keeps the stored one.
	must(t, s.PutBranch(ctx, &domain.Branch{TreeID: "tr", Name: "main", Tip: "s2"}))
	got, err = s.GetBranch(ctx, "tr", "main")
	must(t, err)
	if got.Tip != "s2" || got.Protected || !got.CreatedAt.Equal(at(1)) {
		t.Fatalf("PutBranch replace: %+v", got)
	}

	for _, n := range []string{"release/1.0", "dev", "feature/x"} {
		must(t, s.PutBranch(ctx, &domain.Branch{TreeID: "tr", Name: n}))
	}
	must(t, s.PutBranch(ctx, &domain.Branch{TreeID: "tr2", Name: "aaa"}))
	list, err := s.ListBranches(ctx, "tr")
	must(t, err)
	names := []string{}
	for _, x := range list {
		names = append(names, x.Name)
	}
	wantEqual(t, "branch order", names, []string{"dev", "feature/x", "main", "release/1.0"})

	must(t, s.DeleteBranch(ctx, "tr", "dev"))
	wantErr(t, s.DeleteBranch(ctx, "tr", "dev"), domain.ErrNotFound)
	_, err = s.GetBranch(ctx, "tr", "dev")
	wantErr(t, err, domain.ErrNotFound)
}

func testCAS(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	// "" creates a missing branch.
	must(t, s.CompareAndSwapBranchTip(ctx, "tr", "main", "", "s1"))
	got, err := s.GetBranch(ctx, "tr", "main")
	must(t, err)
	if got.Tip != "s1" || got.TreeID != "tr" || got.Name != "main" || got.CreatedAt.IsZero() || got.UpdatedAt.IsZero() {
		t.Fatalf("CAS-created branch: %+v", got)
	}
	created := got.CreatedAt

	// "" no longer matches.
	wantErr(t, s.CompareAndSwapBranchTip(ctx, "tr", "main", "", "s2"), domain.ErrConflict)
	// Wrong old tip.
	wantErr(t, s.CompareAndSwapBranchTip(ctx, "tr", "main", "s0", "s2"), domain.ErrConflict)
	// Right old tip.
	must(t, s.CompareAndSwapBranchTip(ctx, "tr", "main", "s1", "s2"))
	got, err = s.GetBranch(ctx, "tr", "main")
	must(t, err)
	if got.Tip != "s2" || !got.CreatedAt.Equal(created) {
		t.Fatalf("after CAS: %+v", got)
	}
	// A non-empty old tip never matches a missing branch.
	wantErr(t, s.CompareAndSwapBranchTip(ctx, "tr", "nope", "s1", "s2"), domain.ErrConflict)
	if _, err := s.GetBranch(ctx, "tr", "nope"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("failed CAS created a branch: %v", err)
	}
	// "" matches an existing empty branch, and Protected is preserved.
	must(t, s.PutBranch(ctx, &domain.Branch{TreeID: "tr", Name: "empty", Protected: true}))
	must(t, s.CompareAndSwapBranchTip(ctx, "tr", "empty", "", "s9"))
	got, err = s.GetBranch(ctx, "tr", "empty")
	must(t, err)
	if got.Tip != "s9" || !got.Protected {
		t.Fatalf("CAS on empty branch: %+v", got)
	}
	// Swapping back to empty is allowed.
	must(t, s.CompareAndSwapBranchTip(ctx, "tr", "empty", "s9", ""))
	must(t, s.CompareAndSwapBranchTip(ctx, "tr", "empty", "", "s10"))
}

func testSnapshots(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	mh := hash.Sum([]byte("manifest"))
	s1 := &domain.Snapshot{TreeID: "tr", BranchName: "main", ManifestHash: mh, Author: "alice",
		Message: "first", CreatedAt: at(1), PushedBy: "a1", PushedAt: at(2)}
	must(t, s.PutSnapshot(ctx, s1))
	if s1.ID == "" {
		t.Fatal("PutSnapshot did not fill ID")
	}
	got, err := s.GetSnapshot(ctx, s1.ID)
	must(t, err)
	want := *s1
	want.Parents = []string{} // nil parents are stored as empty
	wantEqual(t, "GetSnapshot", *got, want)
	if got.Parents == nil {
		t.Fatal("Parents is nil")
	}

	s2 := &domain.Snapshot{ID: "s2", TreeID: "tr", Parents: []string{s1.ID}, ManifestHash: mh, Message: "second", CreatedAt: at(3), AutoGenerated: true}
	must(t, s.PutSnapshot(ctx, s2))
	// Identical re-put is a no-op, even with different push metadata.
	again := *s2
	again.Parents = slices.Clone(s2.Parents)
	again.BranchName = "feature"
	again.PushedBy = "someone-else"
	again.PushedAt = at(99)
	again.CreatedAt = at(3).In(time.FixedZone("y", -7200)) // same instant
	must(t, s.PutSnapshot(ctx, &again))
	got, err = s.GetSnapshot(ctx, "s2")
	must(t, err)
	wantEqual(t, "snapshot unchanged by re-put", *got, *s2)

	// Different content conflicts.
	for name, mut := range map[string]func(*domain.Snapshot){
		"message":  func(x *domain.Snapshot) { x.Message = "changed" },
		"parents":  func(x *domain.Snapshot) { x.Parents = nil },
		"manifest": func(x *domain.Snapshot) { x.ManifestHash = hash.Sum([]byte("other")) },
		"created":  func(x *domain.Snapshot) { x.CreatedAt = at(4) },
		"tree":     func(x *domain.Snapshot) { x.TreeID = "other" },
		"author":   func(x *domain.Snapshot) { x.Author = "mallory" },
		"auto":     func(x *domain.Snapshot) { x.AutoGenerated = false },
	} {
		c := *s2
		c.Parents = slices.Clone(s2.Parents)
		mut(&c)
		if err := s.PutSnapshot(ctx, &c); !errors.Is(err, domain.ErrConflict) {
			t.Fatalf("re-put with different %s: err = %v, want ErrConflict", name, err)
		}
	}

	if ok, err := s.HasSnapshot(ctx, "s2"); err != nil || !ok {
		t.Fatalf("HasSnapshot(s2) = %v, %v", ok, err)
	}
	if ok, err := s.HasSnapshot(ctx, "nope"); err != nil || ok {
		t.Fatalf("HasSnapshot(nope) = %v, %v", ok, err)
	}
	_, err = s.GetSnapshot(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)

	must(t, s.PutSnapshot(ctx, &domain.Snapshot{ID: "o1", TreeID: "other"}))
	for tree, want := range map[string]int{"tr": 2, "other": 1, "": 3, "none": 0} {
		n, err := s.CountSnapshots(ctx, tree)
		must(t, err)
		if n != want {
			t.Fatalf("CountSnapshots(%q) = %d, want %d", tree, n, want)
		}
	}
}

func testTags(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	v1 := &domain.Tag{TreeID: "tr", Name: "v1.0", Target: "s1", Message: "release", Tagger: "alice"}
	must(t, s.PutTag(ctx, v1))
	if v1.CreatedAt.IsZero() {
		t.Fatal("PutTag did not fill CreatedAt")
	}
	wantErr(t, s.PutTag(ctx, &domain.Tag{TreeID: "tr", Name: "v1.0", Target: "s2"}), domain.ErrAlreadyExists)
	must(t, s.PutTag(ctx, &domain.Tag{TreeID: "tr", Name: "v0.9", Target: "s0"}))
	must(t, s.PutTag(ctx, &domain.Tag{TreeID: "other", Name: "v1.0", Target: "x"}))

	got, err := s.GetTag(ctx, "tr", "v1.0")
	must(t, err)
	wantEqual(t, "GetTag", *got, *v1)
	_, err = s.GetTag(ctx, "tr", "v2")
	wantErr(t, err, domain.ErrNotFound)

	list, err := s.ListTags(ctx, "tr")
	must(t, err)
	if len(list) != 2 || list[0].Name != "v0.9" || list[1].Name != "v1.0" {
		t.Fatalf("ListTags: %+v", list)
	}
	must(t, s.DeleteTag(ctx, "tr", "v1.0"))
	wantErr(t, s.DeleteTag(ctx, "tr", "v1.0"), domain.ErrNotFound)
	// Name reusable after delete.
	must(t, s.PutTag(ctx, &domain.Tag{TreeID: "tr", Name: "v1.0", Target: "s3"}))
}

func testStaged(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	st := &domain.StagedSnapshot{SnapshotID: "w1", TreeID: "tr", TenantID: "t1", AccountID: "a1",
		Author: "alice", BranchName: "main", FilesChanged: []string{"a.txt"}, FilesAdded: 1, Message: "wip", StagedAt: at(10)}
	must(t, s.PutStaged(ctx, st))
	// Replace for the same (tree, account, branch).
	st2 := *st
	st2.SnapshotID = "w2"
	st2.FilesChanged = []string{"a.txt", "b.txt"}
	st2.StagedAt = at(20)
	must(t, s.PutStaged(ctx, &st2))
	must(t, s.PutStaged(ctx, &domain.StagedSnapshot{SnapshotID: "w3", TreeID: "tr", AccountID: "a2", BranchName: "main", StagedAt: at(30)}))
	must(t, s.PutStaged(ctx, &domain.StagedSnapshot{SnapshotID: "w4", TreeID: "tr", AccountID: "a1", BranchName: "dev", StagedAt: at(5)}))
	must(t, s.PutStaged(ctx, &domain.StagedSnapshot{SnapshotID: "w5", TreeID: "other", AccountID: "a1", BranchName: "main"}))

	ids := func(ss []domain.StagedSnapshot) []string {
		out := []string{}
		for _, x := range ss {
			out = append(out, x.SnapshotID)
		}
		return out
	}
	all, err := s.ListStaged(ctx, "tr", "")
	must(t, err)
	wantEqual(t, "ListStaged(all)", ids(all), []string{"w3", "w2", "w4"})
	wantEqual(t, "replaced entry", all[1], st2)
	if all[0].FilesChanged == nil {
		t.Fatal("FilesChanged is nil")
	}
	mainOnly, err := s.ListStaged(ctx, "tr", "main")
	must(t, err)
	wantEqual(t, "ListStaged(main)", ids(mainOnly), []string{"w3", "w2"})

	if n, err := s.CountStaged(ctx); err != nil || n != 4 {
		t.Fatalf("CountStaged = %d, %v; want 4", n, err)
	}
	must(t, s.DeleteStaged(ctx, "tr", "a1", "main"))
	must(t, s.DeleteStaged(ctx, "tr", "a1", "main")) // idempotent
	must(t, s.DeleteStaged(ctx, "nope", "nope", "nope"))
	all, err = s.ListStaged(ctx, "tr", "")
	must(t, err)
	wantEqual(t, "after delete", ids(all), []string{"w3", "w4"})
	if n, err := s.CountStaged(ctx); err != nil || n != 3 {
		t.Fatalf("CountStaged = %d, %v; want 3", n, err)
	}
}

func testProtections(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	p := &domain.BranchProtection{TreeID: "tr", Pattern: "release/*", NoDirectPush: true, RequiredApprovals: 2, AllowedPushers: []string{"alice"}}
	must(t, s.PutProtection(ctx, p))
	must(t, s.PutProtection(ctx, &domain.BranchProtection{TreeID: "tr", Pattern: "main", NoDelete: true}))
	must(t, s.PutProtection(ctx, &domain.BranchProtection{TreeID: "other", Pattern: "main"}))
	list, err := s.ListProtections(ctx, "tr")
	must(t, err)
	if len(list) != 2 || list[0].Pattern != "main" || list[1].Pattern != "release/*" {
		t.Fatalf("ListProtections: %+v", list)
	}
	wantEqual(t, "protection", list[1], *p)
	if list[0].AllowedPushers == nil {
		t.Fatal("AllowedPushers is nil")
	}
	// Upsert.
	must(t, s.PutProtection(ctx, &domain.BranchProtection{TreeID: "tr", Pattern: "main", RequiredApprovals: 1}))
	list, err = s.ListProtections(ctx, "tr")
	must(t, err)
	if len(list) != 2 || list[0].RequiredApprovals != 1 || list[0].NoDelete {
		t.Fatalf("after upsert: %+v", list)
	}
	must(t, s.DeleteProtection(ctx, "tr", "main"))
	wantErr(t, s.DeleteProtection(ctx, "tr", "main"), domain.ErrNotFound)
	list, err = s.ListProtections(ctx, "tr")
	must(t, err)
	if len(list) != 1 {
		t.Fatalf("after delete: %+v", list)
	}
}

func testMergeRequests(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	var mrs []*domain.MergeRequest
	for i := 0; i < 3; i++ {
		mr := &domain.MergeRequest{TreeID: "tr", Title: "mr" + strconv.Itoa(i), SourceBranch: "f", TargetBranch: "main", AuthorID: "a1", Author: "alice", Number: 99}
		must(t, s.CreateMergeRequest(ctx, mr))
		if mr.Number != i+1 || mr.ID == "" || mr.State != domain.MRStateOpen || mr.CreatedAt.IsZero() || mr.Approvals == nil {
			t.Fatalf("CreateMergeRequest #%d defaults: %+v", i, mr)
		}
		mrs = append(mrs, mr)
	}
	other := &domain.MergeRequest{TreeID: "other", Title: "x"}
	must(t, s.CreateMergeRequest(ctx, other))
	if other.Number != 1 {
		t.Fatalf("per-tree numbering: got %d, want 1", other.Number)
	}

	got, err := s.GetMergeRequest(ctx, "tr", 2)
	must(t, err)
	wantEqual(t, "GetMergeRequest", *got, *mrs[1])
	for _, n := range []int{0, -1, 4} {
		_, err = s.GetMergeRequest(ctx, "tr", n)
		wantErr(t, err, domain.ErrNotFound)
	}

	// Update: merge #2.
	merged := at(50)
	got.State = domain.MRStateMerged
	got.MergedAt = &merged
	got.MergedSnapshot = "s9"
	got.Approvals = append(got.Approvals, domain.Approval{AccountID: "a2", Username: "bob", At: at(40)})
	got.CreatedAt = time.Time{}
	time.Sleep(2 * time.Millisecond)
	must(t, s.UpdateMergeRequest(ctx, got))
	if !got.UpdatedAt.After(mrs[1].UpdatedAt) || !got.CreatedAt.Equal(mrs[1].CreatedAt) {
		t.Fatalf("UpdateMergeRequest timestamps: %+v", got)
	}
	got2, err := s.GetMergeRequest(ctx, "tr", 2)
	must(t, err)
	wantEqual(t, "updated MR", *got2, *got)

	// Close #1.
	c := *mrs[0]
	c.State = domain.MRStateClosed
	must(t, s.UpdateMergeRequest(ctx, &c))

	nums := func(list []domain.MergeRequest) []int {
		out := []int{}
		for _, m := range list {
			out = append(out, m.Number)
		}
		return out
	}
	list, err := s.ListMergeRequests(ctx, "tr", "")
	must(t, err)
	wantEqual(t, "ListMergeRequests(all)", nums(list), []int{3, 2, 1})
	for state, want := range map[string][]int{
		domain.MRStateOpen: {3}, domain.MRStateMerged: {2}, domain.MRStateClosed: {1}, "bogus": {},
	} {
		list, err := s.ListMergeRequests(ctx, "tr", state)
		must(t, err)
		wantEqual(t, "ListMergeRequests("+state+")", nums(list), want)
	}

	wantErr(t, s.UpdateMergeRequest(ctx, &domain.MergeRequest{TreeID: "tr", Number: 42}), domain.ErrNotFound)
	bad := *mrs[2]
	bad.ID = "different"
	wantErr(t, s.UpdateMergeRequest(ctx, &bad), domain.ErrInvalid)

	// Numbers keep increasing.
	next := &domain.MergeRequest{TreeID: "tr"}
	must(t, s.CreateMergeRequest(ctx, next))
	if next.Number != 4 {
		t.Fatalf("next number = %d, want 4", next.Number)
	}
}

func testAudit(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	e1 := &domain.AuditEvent{TenantID: "t1", ActorID: "a1", Actor: "alice", Action: "tree.create",
		ResourceType: "tree", ResourceID: "tr1", Details: map[string]string{"name": "web"}, RemoteAddr: "127.0.0.1", At: at(10)}
	must(t, s.AppendAudit(ctx, e1))
	if e1.ID == "" {
		t.Fatal("AppendAudit did not fill ID")
	}
	must(t, s.AppendAudit(ctx, &domain.AuditEvent{ID: "e2", TenantID: "t1", ActorID: "a2", Action: "branch.push", At: at(30)}))
	// Out-of-order timestamp lands in time order.
	must(t, s.AppendAudit(ctx, &domain.AuditEvent{ID: "e3", TenantID: "t2", ActorID: "a1", Action: "branch.push", At: at(20)}))
	// Same instant as e2: later insert is listed first.
	must(t, s.AppendAudit(ctx, &domain.AuditEvent{ID: "e4", TenantID: "t1", ActorID: "a1", Action: "auth.login", At: at(30)}))
	e5 := &domain.AuditEvent{ID: "e5", Action: "system.start"}
	must(t, s.AppendAudit(ctx, e5))
	if e5.At.IsZero() {
		t.Fatal("AppendAudit did not fill At")
	}

	ids := func(es []domain.AuditEvent) []string {
		out := []string{}
		for _, e := range es {
			out = append(out, e.ID)
		}
		return out
	}
	all, err := s.ListAudit(ctx, domain.AuditFilter{})
	must(t, err)
	wantEqual(t, "ListAudit(all)", ids(all), []string{"e5", "e4", "e2", "e3", e1.ID})
	wantEqual(t, "audit event", all[4], *e1)

	cases := []struct {
		name string
		f    domain.AuditFilter
		want []string
	}{
		{"tenant", domain.AuditFilter{TenantID: "t1"}, []string{"e4", "e2", e1.ID}},
		{"actor", domain.AuditFilter{ActorID: "a1"}, []string{"e4", "e3", e1.ID}},
		{"action", domain.AuditFilter{Action: "branch.push"}, []string{"e2", "e3"}},
		{"since inclusive", domain.AuditFilter{Since: at(20)}, []string{"e5", "e4", "e2", "e3"}},
		{"since+tenant", domain.AuditFilter{TenantID: "t1", Since: at(20)}, []string{"e4", "e2"}},
		{"limit", domain.AuditFilter{Limit: 2}, []string{"e5", "e4"}},
		{"limit+filter", domain.AuditFilter{ActorID: "a1", Limit: 2}, []string{"e4", "e3"}},
		{"no match", domain.AuditFilter{TenantID: "t9"}, []string{}},
	}
	for _, tc := range cases {
		got, err := s.ListAudit(ctx, tc.f)
		must(t, err)
		wantEqual(t, "ListAudit("+tc.name+")", ids(got), tc.want)
	}

	// Default limit is 100.
	for i := 0; i < 110; i++ {
		must(t, s.AppendAudit(ctx, &domain.AuditEvent{TenantID: "bulk", Action: "x", At: at(1000 + i)}))
	}
	got, err := s.ListAudit(ctx, domain.AuditFilter{TenantID: "bulk"})
	must(t, err)
	if len(got) != defaultAuditLimit {
		t.Fatalf("default limit: got %d events, want %d", len(got), defaultAuditLimit)
	}
	if !got[0].At.Equal(at(1109)) || !got[99].At.Equal(at(1010)) {
		t.Fatalf("default-limit window: first %v last %v", got[0].At, got[99].At)
	}
	got, err = s.ListAudit(ctx, domain.AuditFilter{TenantID: "bulk", Limit: 1000})
	must(t, err)
	if len(got) != 110 {
		t.Fatalf("explicit limit: got %d, want 110", len(got))
	}
}

func testLicenseRules(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	r := &domain.LicenseRule{TreeID: "tr", PathGlob: "vendor/**", SPDX: "Apache-2.0", Grant: "read-only"}
	must(t, s.PutLicenseRule(ctx, r))
	must(t, s.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: "tr", PathGlob: "**", SPDX: "MIT", Grant: "modify"}))
	must(t, s.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: "other", PathGlob: "**", SPDX: "MIT"}))
	list, err := s.ListLicenseRules(ctx, "tr")
	must(t, err)
	if len(list) != 2 || list[0].PathGlob != "**" || list[1] != *r {
		t.Fatalf("ListLicenseRules: %+v", list)
	}
	must(t, s.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: "tr", PathGlob: "**", SPDX: "GPL-3.0-only", Grant: "redistribute"}))
	list, err = s.ListLicenseRules(ctx, "tr")
	must(t, err)
	if len(list) != 2 || list[0].SPDX != "GPL-3.0-only" {
		t.Fatalf("after upsert: %+v", list)
	}
	must(t, s.DeleteLicenseRule(ctx, "tr", "**"))
	wantErr(t, s.DeleteLicenseRule(ctx, "tr", "**"), domain.ErrNotFound)
	list, err = s.ListLicenseRules(ctx, "tr")
	must(t, err)
	if len(list) != 1 || list[0] != *r {
		t.Fatalf("after delete: %+v", list)
	}
}

func testAccessConfig(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	_, err := s.GetAccessConfig(ctx, "t1", "roles")
	wantErr(t, err, domain.ErrNotFound)
	doc := []byte(`[roles]` + "\n" + `x = 1`)
	must(t, s.PutAccessConfig(ctx, "t1", "roles", doc))
	doc[0] = '#' // caller mutation must not leak in
	got, err := s.GetAccessConfig(ctx, "t1", "roles")
	must(t, err)
	if string(got) != "[roles]\nx = 1" {
		t.Fatalf("GetAccessConfig = %q", got)
	}
	got[0] = '#' // returned slice mutation must not leak in
	got, err = s.GetAccessConfig(ctx, "t1", "roles")
	must(t, err)
	if got[0] != '[' {
		t.Fatal("returned access config aliases store state")
	}
	must(t, s.PutAccessConfig(ctx, "t1", "roles", []byte("v2")))
	got, err = s.GetAccessConfig(ctx, "t1", "roles")
	must(t, err)
	if string(got) != "v2" {
		t.Fatalf("replace: %q", got)
	}
	// Empty documents are stored and distinguishable from missing ones.
	must(t, s.PutAccessConfig(ctx, "t1", "policies", nil))
	got, err = s.GetAccessConfig(ctx, "t1", "policies")
	must(t, err)
	if got == nil || len(got) != 0 {
		t.Fatalf("empty access config = %#v, want empty non-nil", got)
	}
	_, err = s.GetAccessConfig(ctx, "t2", "roles")
	wantErr(t, err, domain.ErrNotFound)
}

func testInvalidArguments(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	nul := "bad\x00name"
	calls := map[string]error{
		"nil tenant":          s.CreateTenant(ctx, nil),
		"empty slug":          s.CreateTenant(ctx, &domain.Tenant{}),
		"nul slug":            s.CreateTenant(ctx, &domain.Tenant{Slug: nul}),
		"nul tenant id":       s.CreateTenant(ctx, &domain.Tenant{ID: nul, Slug: "ok"}),
		"update nil tenant":   s.UpdateTenant(ctx, nil),
		"nil account":         s.CreateAccount(ctx, nil),
		"empty username":      s.CreateAccount(ctx, &domain.Account{}),
		"update nil account":  s.UpdateAccount(ctx, nil),
		"nil membership":      s.PutMembership(ctx, nil),
		"membership no acct":  s.PutMembership(ctx, &domain.Membership{TenantID: "t"}),
		"nil token":           s.CreateToken(ctx, nil),
		"token no hash":       s.CreateToken(ctx, &domain.APIToken{AccountID: "a"}),
		"token no account":    s.CreateToken(ctx, &domain.APIToken{TokenHash: "h"}),
		"nil tree":            s.CreateTree(ctx, nil),
		"tree no tenant":      s.CreateTree(ctx, &domain.Tree{Name: "x"}),
		"tree nul name":       s.CreateTree(ctx, &domain.Tree{TenantID: "t", Name: nul}),
		"update nil tree":     s.UpdateTree(ctx, nil),
		"nil branch":          s.PutBranch(ctx, nil),
		"branch no name":      s.PutBranch(ctx, &domain.Branch{TreeID: "t"}),
		"cas no tree":         s.CompareAndSwapBranchTip(ctx, "", "main", "", "x"),
		"cas nul name":        s.CompareAndSwapBranchTip(ctx, "t", nul, "", "x"),
		"nil snapshot":        s.PutSnapshot(ctx, nil),
		"snapshot no tree":    s.PutSnapshot(ctx, &domain.Snapshot{ID: "x"}),
		"nil tag":             s.PutTag(ctx, nil),
		"tag no name":         s.PutTag(ctx, &domain.Tag{TreeID: "t"}),
		"nil staged":          s.PutStaged(ctx, nil),
		"staged no branch":    s.PutStaged(ctx, &domain.StagedSnapshot{TreeID: "t", AccountID: "a"}),
		"nil protection":      s.PutProtection(ctx, nil),
		"protection no patt":  s.PutProtection(ctx, &domain.BranchProtection{TreeID: "t"}),
		"nil mr":              s.CreateMergeRequest(ctx, nil),
		"mr no tree":          s.CreateMergeRequest(ctx, &domain.MergeRequest{}),
		"update nil mr":       s.UpdateMergeRequest(ctx, nil),
		"nil audit":           s.AppendAudit(ctx, nil),
		"nil license":         s.PutLicenseRule(ctx, nil),
		"license no glob":     s.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: "t"}),
		"access no kind":      s.PutAccessConfig(ctx, "t", "", nil),
		"access nul tenant":   s.PutAccessConfig(ctx, nul, "roles", nil),
		"update tree no name": s.UpdateTree(ctx, &domain.Tree{ID: "x", TenantID: "t"}),
	}
	for name, err := range calls {
		if !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("%s: err = %v, want ErrInvalid", name, err)
		}
	}
	// Rejected arguments are not mutated.
	tn := &domain.Tenant{}
	_ = s.CreateTenant(ctx, tn)
	if tn.ID != "" || !tn.CreatedAt.IsZero() {
		t.Fatalf("rejected tenant was mutated: %+v", tn)
	}
	// And nothing was written.
	if list, _ := s.ListTenants(ctx); len(list) != 0 {
		t.Fatalf("invalid calls wrote tenants: %+v", list)
	}
	if n, _ := s.CountSnapshots(ctx, ""); n != 0 {
		t.Fatalf("invalid calls wrote %d snapshots", n)
	}
}

func testEmptyListsNonNil(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	check := func(name string, v any, err error) {
		t.Helper()
		must(t, err)
		rv := reflect.ValueOf(v)
		if rv.IsNil() || rv.Len() != 0 {
			t.Errorf("%s = %#v, want empty non-nil slice", name, v)
		}
	}
	v1, err := s.ListTenants(ctx)
	check("ListTenants", v1, err)
	v2, err := s.ListAccounts(ctx)
	check("ListAccounts", v2, err)
	v3, err := s.ListTenantMembers(ctx, "t")
	check("ListTenantMembers", v3, err)
	v4, err := s.ListAccountMemberships(ctx, "a")
	check("ListAccountMemberships", v4, err)
	v5, err := s.ListTokens(ctx, "a")
	check("ListTokens", v5, err)
	v6, err := s.ListTrees(ctx, "")
	check("ListTrees(all)", v6, err)
	v7, err := s.ListTrees(ctx, "t")
	check("ListTrees", v7, err)
	v8, err := s.ListBranches(ctx, "tr")
	check("ListBranches", v8, err)
	v9, err := s.ListTags(ctx, "tr")
	check("ListTags", v9, err)
	v10, err := s.ListStaged(ctx, "tr", "")
	check("ListStaged", v10, err)
	v11, err := s.ListProtections(ctx, "tr")
	check("ListProtections", v11, err)
	v12, err := s.ListMergeRequests(ctx, "tr", "")
	check("ListMergeRequests", v12, err)
	v13, err := s.ListAudit(ctx, domain.AuditFilter{})
	check("ListAudit", v13, err)
	v14, err := s.ListLicenseRules(ctx, "tr")
	check("ListLicenseRules", v14, err)
	for name, f := range map[string]func() (int, error){
		"CountSnapshots": func() (int, error) { return s.CountSnapshots(ctx, "") },
		"CountStaged":    func() (int, error) { return s.CountStaged(ctx) },
	} {
		n, err := f()
		must(t, err)
		if n != 0 {
			t.Errorf("%s = %d on empty store", name, n)
		}
	}
}

func testNoAliasing(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	// Mutating the argument after the call must not change the store.
	snap := &domain.Snapshot{ID: "s1", TreeID: "tr", Parents: []string{"p1", "p2"}}
	must(t, s.PutSnapshot(ctx, snap))
	snap.Parents[0] = "MUTATED"
	got, err := s.GetSnapshot(ctx, "s1")
	must(t, err)
	if got.Parents[0] != "p1" {
		t.Fatal("PutSnapshot aliases caller's Parents")
	}
	// Mutating a returned value must not change the store.
	got.Parents[1] = "MUTATED"
	got, err = s.GetSnapshot(ctx, "s1")
	must(t, err)
	if got.Parents[1] != "p2" {
		t.Fatal("GetSnapshot returns aliased Parents")
	}

	st := &domain.StagedSnapshot{TreeID: "tr", AccountID: "a", BranchName: "main", FilesChanged: []string{"x"}}
	must(t, s.PutStaged(ctx, st))
	st.FilesChanged[0] = "MUTATED"
	list, err := s.ListStaged(ctx, "tr", "")
	must(t, err)
	if list[0].FilesChanged[0] != "x" {
		t.Fatal("PutStaged aliases FilesChanged")
	}
	list[0].FilesChanged[0] = "MUTATED"
	list, _ = s.ListStaged(ctx, "tr", "")
	if list[0].FilesChanged[0] != "x" {
		t.Fatal("ListStaged returns aliased FilesChanged")
	}

	p := &domain.BranchProtection{TreeID: "tr", Pattern: "main", AllowedPushers: []string{"alice"}}
	must(t, s.PutProtection(ctx, p))
	p.AllowedPushers[0] = "MUTATED"
	ps, _ := s.ListProtections(ctx, "tr")
	if ps[0].AllowedPushers[0] != "alice" {
		t.Fatal("PutProtection aliases AllowedPushers")
	}
	ps[0].AllowedPushers[0] = "MUTATED"
	ps, _ = s.ListProtections(ctx, "tr")
	if ps[0].AllowedPushers[0] != "alice" {
		t.Fatal("ListProtections returns aliased AllowedPushers")
	}

	mr := &domain.MergeRequest{TreeID: "tr", Approvals: []domain.Approval{{Username: "bob"}}}
	must(t, s.CreateMergeRequest(ctx, mr))
	mr.Approvals[0].Username = "MUTATED"
	gotMR, _ := s.GetMergeRequest(ctx, "tr", 1)
	if gotMR.Approvals[0].Username != "bob" {
		t.Fatal("CreateMergeRequest aliases Approvals")
	}
	gotMR.Approvals[0].Username = "MUTATED"
	mrs, _ := s.ListMergeRequests(ctx, "tr", "")
	if mrs[0].Approvals[0].Username != "bob" {
		t.Fatal("GetMergeRequest returns aliased Approvals")
	}

	exp := at(5)
	tok := &domain.APIToken{AccountID: "a", TokenHash: "h", ExpiresAt: &exp}
	must(t, s.CreateToken(ctx, tok))
	*tok.ExpiresAt = at(500)
	gotTok, _ := s.GetTokenByHash(ctx, "h")
	if !gotTok.ExpiresAt.Equal(at(5)) {
		t.Fatal("CreateToken aliases ExpiresAt")
	}
	*gotTok.ExpiresAt = at(500)
	gotTok, _ = s.GetTokenByHash(ctx, "h")
	if !gotTok.ExpiresAt.Equal(at(5)) {
		t.Fatal("GetTokenByHash returns aliased ExpiresAt")
	}

	ev := &domain.AuditEvent{Action: "x", Details: map[string]string{"k": "v"}}
	must(t, s.AppendAudit(ctx, ev))
	ev.Details["k"] = "MUTATED"
	evs, _ := s.ListAudit(ctx, domain.AuditFilter{})
	if evs[0].Details["k"] != "v" {
		t.Fatal("AppendAudit aliases Details")
	}
	evs[0].Details["k"] = "MUTATED"
	evs, _ = s.ListAudit(ctx, domain.AuditFilter{})
	if evs[0].Details["k"] != "v" {
		t.Fatal("ListAudit returns aliased Details")
	}

	tn := &domain.Tenant{Slug: "acme", Name: "Acme"}
	must(t, s.CreateTenant(ctx, tn))
	gotT, _ := s.GetTenant(ctx, tn.ID)
	gotT.Name = "MUTATED"
	gotT, _ = s.GetTenant(ctx, tn.ID)
	if gotT.Name != "Acme" {
		t.Fatal("GetTenant returns aliased tenant")
	}
}

func testCanceledContext(t *testing.T, s domain.MetaStore) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	wantErr(t, s.CreateTenant(ctx, &domain.Tenant{Slug: "x"}), context.Canceled)
	_, err := s.GetTenant(ctx, "x")
	wantErr(t, err, context.Canceled)
	_, err = s.ListTrees(ctx, "")
	wantErr(t, err, context.Canceled)
	wantErr(t, s.CompareAndSwapBranchTip(ctx, "tr", "main", "", "s"), context.Canceled)
	_, err = s.ListAudit(ctx, domain.AuditFilter{})
	wantErr(t, err, context.Canceled)
	if list, err := s.ListTenants(context.Background()); err != nil || len(list) != 0 {
		t.Fatalf("canceled create wrote data: %v %v", list, err)
	}
}

func testClose(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	must(t, s.CreateTenant(ctx, &domain.Tenant{Slug: "x"}))
	must(t, s.Close())
	must(t, s.Close()) // idempotent
	wantErr(t, s.CreateTenant(ctx, &domain.Tenant{Slug: "y"}), ErrClosed)
	_, err := s.GetTenantBySlug(ctx, "x")
	wantErr(t, err, ErrClosed)
	_, err = s.ListAudit(ctx, domain.AuditFilter{})
	wantErr(t, err, ErrClosed)
	_, err = NewHistory(s).Log(ctx, "x", 0)
	wantErr(t, err, ErrClosed)
}

func testConcurrentCASOneWinner(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	const n = 32
	for round := 0; round < 3; round++ {
		branch := "b" + strconv.Itoa(round)
		var wins atomic.Int32
		var wg sync.WaitGroup
		errs := make(chan error, n)
		start := make(chan struct{})
		for i := 0; i < n; i++ {
			wg.Add(1)
			go func(i int) {
				defer wg.Done()
				<-start
				err := s.CompareAndSwapBranchTip(ctx, "tr", branch, "", fmt.Sprintf("tip-%d", i))
				switch {
				case err == nil:
					wins.Add(1)
				case errors.Is(err, domain.ErrConflict):
				default:
					errs <- err
				}
			}(i)
		}
		close(start)
		wg.Wait()
		close(errs)
		for err := range errs {
			t.Fatalf("unexpected CAS error: %v", err)
		}
		if w := wins.Load(); w != 1 {
			t.Fatalf("round %d: %d CAS winners, want exactly 1", round, w)
		}
	}
}

// testConcurrentCASChain has writers repeatedly advance one branch with
// read-then-CAS; every successful swap must be observed exactly once.
func testConcurrentCASChain(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	const workers, attempts = 8, 25
	var wins atomic.Int32
	var mu sync.Mutex
	seenOld := map[string]bool{}
	var wg sync.WaitGroup
	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			for i := 0; i < attempts; i++ {
				cur := ""
				if b, err := s.GetBranch(ctx, "tr", "main"); err == nil {
					cur = b.Tip
				} else if !errors.Is(err, domain.ErrNotFound) {
					t.Error(err)
					return
				}
				next := fmt.Sprintf("w%d-%d", w, i)
				err := s.CompareAndSwapBranchTip(ctx, "tr", "main", cur, next)
				if err == nil {
					wins.Add(1)
					mu.Lock()
					if seenOld[cur] {
						t.Errorf("two swaps succeeded from the same old tip %q", cur)
					}
					seenOld[cur] = true
					mu.Unlock()
				} else if !errors.Is(err, domain.ErrConflict) {
					t.Error(err)
					return
				}
			}
		}(w)
	}
	wg.Wait()
	if wins.Load() == 0 {
		t.Fatal("no CAS succeeded")
	}
	if int(wins.Load()) != len(seenOld) {
		t.Fatalf("wins %d != distinct old tips %d", wins.Load(), len(seenOld))
	}
}

func testConcurrentMRNumbers(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	const n = 40
	nums := make([]int, n)
	var wg sync.WaitGroup
	for i := 0; i < n; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			mr := &domain.MergeRequest{TreeID: "tr", Title: strconv.Itoa(i)}
			if err := s.CreateMergeRequest(ctx, mr); err != nil {
				t.Error(err)
				return
			}
			nums[i] = mr.Number
		}(i)
	}
	wg.Wait()
	slices.Sort(nums)
	for i, got := range nums {
		if got != i+1 {
			t.Fatalf("MR numbers not 1..%d: %v", n, nums)
		}
	}
}

func testConcurrentUniqueSlug(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	const n = 24
	var wins atomic.Int32
	var wg sync.WaitGroup
	for i := 0; i < n; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			err := s.CreateTenant(ctx, &domain.Tenant{Slug: "contested"})
			if err == nil {
				wins.Add(1)
			} else if !errors.Is(err, domain.ErrAlreadyExists) {
				t.Error(err)
			}
		}()
	}
	wg.Wait()
	if wins.Load() != 1 {
		t.Fatalf("%d tenants created with the same slug", wins.Load())
	}
}

// testConcurrentMixed hammers many methods in parallel; it is mostly
// useful under the race detector.
func testConcurrentMixed(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	var wg sync.WaitGroup
	for w := 0; w < 8; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			for i := 0; i < 20; i++ {
				id := fmt.Sprintf("w%d-%d", w, i)
				if err := s.PutSnapshot(ctx, &domain.Snapshot{ID: id, TreeID: "tr"}); err != nil {
					t.Error(err)
					return
				}
				if err := s.AppendAudit(ctx, &domain.AuditEvent{Action: "x", ActorID: id}); err != nil {
					t.Error(err)
					return
				}
				if err := s.PutStaged(ctx, &domain.StagedSnapshot{TreeID: "tr", AccountID: id, BranchName: "main"}); err != nil {
					t.Error(err)
					return
				}
				if _, err := s.ListStaged(ctx, "tr", "main"); err != nil {
					t.Error(err)
					return
				}
				if _, err := s.ListAudit(ctx, domain.AuditFilter{Limit: 5}); err != nil {
					t.Error(err)
					return
				}
			}
		}(w)
	}
	wg.Wait()
	if n, err := s.CountSnapshots(ctx, "tr"); err != nil || n != 160 {
		t.Fatalf("CountSnapshots = %d, %v; want 160", n, err)
	}
	if n, err := s.CountStaged(ctx); err != nil || n != 160 {
		t.Fatalf("CountStaged = %d, %v; want 160", n, err)
	}
}
