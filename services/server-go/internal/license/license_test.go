package license

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// fakeStore implements only the MetaStore methods the checker uses.
type fakeStore struct {
	domain.MetaStore

	rules       map[string][]domain.LicenseRule // tree id -> rules
	members     map[string]string               // tenantID+"/"+accountID -> role
	rulesErr    error
	memberErr   error
	memberCalls atomic.Int32
	rulesCalls  atomic.Int32
}

func (s *fakeStore) ListLicenseRules(_ context.Context, treeID string) ([]domain.LicenseRule, error) {
	s.rulesCalls.Add(1)
	if s.rulesErr != nil {
		return nil, s.rulesErr
	}
	rs := s.rules[treeID]
	if rs == nil {
		rs = []domain.LicenseRule{}
	}
	return rs, nil
}

func (s *fakeStore) GetMembership(_ context.Context, tenantID, accountID string) (*domain.Membership, error) {
	s.memberCalls.Add(1)
	if s.memberErr != nil {
		return nil, s.memberErr
	}
	role, ok := s.members[tenantID+"/"+accountID]
	if !ok {
		return nil, fmt.Errorf("membership: %w", domain.ErrNotFound)
	}
	return &domain.Membership{TenantID: tenantID, AccountID: accountID, Role: role}, nil
}

var (
	tree = &domain.Tree{ID: "tree1", TenantID: "acme", Name: "app", Visibility: domain.VisibilityPublic}

	owner    = &domain.Principal{AccountID: "u-owner", Username: "acme", TenantID: "acme"}
	member   = &domain.Principal{AccountID: "u-member", Username: "mia", TenantID: "t-mia"}
	viewer   = &domain.Principal{AccountID: "u-viewer", Username: "vic", TenantID: "t-vic"}
	admin    = &domain.Principal{AccountID: "u-admin", Username: "root", TenantID: "t-root", IsAdmin: true}
	outsider = &domain.Principal{AccountID: "u-out", Username: "olga", TenantID: "partner"}
)

func newStore(rules ...domain.LicenseRule) *fakeStore {
	for i := range rules {
		rules[i].TreeID = tree.ID
	}
	return &fakeStore{
		rules: map[string][]domain.LicenseRule{tree.ID: rules},
		members: map[string]string{
			"acme/u-member": domain.RoleDeveloper,
			"acme/u-viewer": domain.RoleViewer,
		},
	}
}

func check(c *Checker, p *domain.Principal, op Operation, paths ...string) error {
	return c.Check(context.Background(), p, tree, op, paths)
}

// TestGrantMatrix covers every grant (and grant-less license) for every
// kind of principal and operation, through both the interface methods and
// CheckExport.
func TestGrantMatrix(t *testing.T) {
	type allowed struct{ read, write, export bool }
	all := allowed{true, true, true}
	none := allowed{}

	tests := []struct {
		name      string
		spdx      string
		grant     string
		nonMember allowed
	}{
		{"read-only", "MIT", GrantReadOnly, allowed{read: true}},
		{"modify", "MIT", GrantModify, allowed{read: true, write: true}},
		{"redistribute", "MIT", GrantRedistribute, all},
		{"open license without grant", "Apache-2.0", "", all},
		{"open expression without grant", "MIT OR Apache-2.0", "", all},
		{"Proprietary without grant", "Proprietary", "", none},
		{"LicenseRef-Proprietary without grant", "LicenseRef-Proprietary", "", none},
		{"proprietary AND open without grant", "MIT AND LicenseRef-Proprietary", "", none},
		{"proprietary with read-only", "LicenseRef-Proprietary", GrantReadOnly, allowed{read: true}},
		{"proprietary with modify", "Proprietary", GrantModify, allowed{read: true, write: true}},
		{"proprietary with redistribute", "Proprietary", GrantRedistribute, all},
		{"unknown grant fails closed", "MIT", "everything", none},
		{"mis-cased grant fails closed", "MIT", "Read-Only", none},
	}
	principals := []struct {
		name       string
		p          *domain.Principal
		privileged bool
	}{
		{"owner", owner, true},
		{"member", member, true},
		{"viewer member", viewer, true},
		{"server admin", admin, true},
		{"outsider", outsider, false},
		{"anonymous", nil, false},
	}
	const path = "services/billing/src/pricing.rs"

	for _, tt := range tests {
		for _, pr := range principals {
			t.Run(tt.name+"/"+pr.name, func(t *testing.T) {
				c := New(newStore(domain.LicenseRule{PathGlob: "services/billing/**", SPDX: tt.spdx, Grant: tt.grant}))
				want := tt.nonMember
				if pr.privileged {
					want = all
				}
				ctx := context.Background()
				results := []struct {
					op   Operation
					err  error
					want bool
				}{
					{OpRead, c.CheckRead(ctx, pr.p, tree, []string{path}), want.read},
					{OpWrite, c.CheckPush(ctx, pr.p, tree, []string{path}), want.write},
					{OpExport, c.CheckExport(ctx, pr.p, tree, []string{path}), want.export},
				}
				for _, r := range results {
					if r.want {
						if r.err != nil {
							t.Errorf("%s: unexpected error %v", r.op, r.err)
						}
						continue
					}
					var pd *domain.PushDenied
					if !errors.As(r.err, &pd) {
						t.Errorf("%s: err = %v, want *PushDenied", r.op, r.err)
						continue
					}
					if pd.Kind != KindLicenseViolation || pd.Path != path || pd.License != tt.spdx {
						t.Errorf("%s: PushDenied = %+v", r.op, pd)
					}
					if !strings.Contains(pd.Reason, "required grant: "+r.op.requiredGrant()) {
						t.Errorf("%s: reason %q lacks required grant", r.op, pd.Reason)
					}
				}
				// Permits agrees with the non-member column.
				rule := domain.LicenseRule{SPDX: tt.spdx, Grant: tt.grant}
				if Permits(rule, OpRead) != tt.nonMember.read || Permits(rule, OpWrite) != tt.nonMember.write ||
					Permits(rule, OpExport) != tt.nonMember.export {
					t.Errorf("Permits disagrees with matrix for %+v", rule)
				}
			})
		}
	}
}

func TestNoRulesAllowsEveryone(t *testing.T) {
	c := New(newStore())
	for _, p := range []*domain.Principal{nil, outsider} {
		for _, op := range []Operation{OpRead, OpWrite, OpExport} {
			if err := check(c, p, op, "anything/at/all.rs"); err != nil {
				t.Errorf("op %s principal %v: %v", op, p, err)
			}
		}
	}
}

func TestUnmatchedPathsAreUnrestricted(t *testing.T) {
	c := New(newStore(domain.LicenseRule{PathGlob: "services/billing", SPDX: "Proprietary"}))
	if err := check(c, outsider, OpWrite, "src/main.rs", "services/billing-engine/x.rs", "README.md"); err != nil {
		t.Fatalf("unexpected denial: %v", err)
	}
}

func TestFirstViolationReportedInInputOrder(t *testing.T) {
	c := New(newStore(
		domain.LicenseRule{PathGlob: "a", SPDX: "Proprietary"},
		domain.LicenseRule{PathGlob: "b", SPDX: "LicenseRef-Proprietary"},
	))
	err := check(c, outsider, OpRead, "ok.txt", "b/1", "a/1")
	var pd *domain.PushDenied
	if !errors.As(err, &pd) || pd.Path != "b/1" || pd.License != "LicenseRef-Proprietary" {
		t.Fatalf("err = %v, want denial for b/1", err)
	}
	if !strings.Contains(pd.Error(), "license_violation") {
		t.Errorf("Error() = %q", pd.Error())
	}
}

func TestMostSpecificRuleWinsInChecks(t *testing.T) {
	c := New(newStore(
		domain.LicenseRule{PathGlob: "**", SPDX: "MIT", Grant: GrantRedistribute},
		domain.LicenseRule{PathGlob: "services/billing/**", SPDX: "Proprietary"},
		domain.LicenseRule{PathGlob: "services/billing/src/api.rs", SPDX: "Proprietary", Grant: GrantRedistribute},
		domain.LicenseRule{PathGlob: "services/billing/docs", SPDX: "Proprietary", Grant: GrantReadOnly},
	))
	tests := []struct {
		path string
		op   Operation
		ok   bool
	}{
		{"src/main.rs", OpExport, true},
		{"services/billing/src/pricing.rs", OpRead, false},
		{"services/billing/src/api.rs", OpExport, true},
		{"services/billing/docs/intro.md", OpRead, true},
		{"services/billing/docs/intro.md", OpWrite, false},
		{"services/other.rs", OpWrite, true},
	}
	for _, tt := range tests {
		err := check(c, outsider, tt.op, tt.path)
		if (err == nil) != tt.ok {
			t.Errorf("%s %s: err = %v, want ok=%v", tt.op, tt.path, err, tt.ok)
		}
	}
}

func TestWindowsAndOddPathsNormalized(t *testing.T) {
	c := New(newStore(domain.LicenseRule{PathGlob: "vendor/**", SPDX: "Proprietary"}))
	for _, p := range []string{`vendor\sdk\a.go`, "/vendor/a", "./vendor/a", "x/../vendor/a"} {
		if err := check(c, outsider, OpRead, p); err == nil {
			t.Errorf("%q: expected denial", p)
		}
	}
}

func TestInvalidPathDenied(t *testing.T) {
	c := New(newStore(domain.LicenseRule{PathGlob: "secret", SPDX: "Proprietary"}))
	for _, p := range []string{"../secret/x", "..", "", "/"} {
		var pd *domain.PushDenied
		if err := check(c, outsider, OpRead, p); !errors.As(err, &pd) || !strings.Contains(pd.Reason, "invalid path") {
			t.Errorf("%q: err = %v, want invalid path denial", p, err)
		}
	}
	// Privileged principals are never path-checked.
	if err := check(c, owner, OpRead, "../x"); err != nil {
		t.Errorf("owner: %v", err)
	}
}

func TestFastPathsAvoidStore(t *testing.T) {
	s := newStore(domain.LicenseRule{PathGlob: "**", SPDX: "Proprietary"})
	c := New(s)
	for _, p := range []*domain.Principal{owner, admin} {
		if err := check(c, p, OpExport, "a"); err != nil {
			t.Fatal(err)
		}
	}
	if err := check(c, outsider, OpRead); err != nil { // no paths
		t.Fatal(err)
	}
	if s.rulesCalls.Load() != 0 || s.memberCalls.Load() != 0 {
		t.Errorf("store consulted: rules=%d members=%d", s.rulesCalls.Load(), s.memberCalls.Load())
	}
	// Membership is only looked up when rules exist.
	empty := newStore()
	if err := check(New(empty), outsider, OpRead, "a"); err != nil {
		t.Fatal(err)
	}
	if empty.memberCalls.Load() != 0 {
		t.Error("membership looked up for a tree without rules")
	}
	// Anonymous callers never trigger a membership lookup.
	_ = check(c, nil, OpRead, "a")
	if s.memberCalls.Load() != 0 {
		t.Error("membership looked up for anonymous")
	}
}

func TestStoreErrorsFailClosed(t *testing.T) {
	boom := errors.New("bolt: database closed")
	tests := []struct {
		name  string
		setup func(*fakeStore)
	}{
		{"rules", func(s *fakeStore) { s.rulesErr = boom }},
		{"membership", func(s *fakeStore) { s.memberErr = boom }},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := newStore(domain.LicenseRule{PathGlob: "**", SPDX: "MIT", Grant: GrantRedistribute})
			tt.setup(s)
			err := check(New(s), outsider, OpRead, "a")
			var pd *domain.PushDenied
			if err == nil || errors.As(err, &pd) || !errors.Is(err, boom) {
				t.Fatalf("err = %v, want wrapped store error", err)
			}
		})
	}
	// ErrNotFound from ListLicenseRules means "no rules".
	s := newStore()
	s.rulesErr = fmt.Errorf("tree: %w", domain.ErrNotFound)
	if err := check(New(s), outsider, OpRead, "a"); err != nil {
		t.Fatalf("ErrNotFound rules: %v", err)
	}
}

func TestMalformedStoredRuleFailsClosed(t *testing.T) {
	c := New(newStore(domain.LicenseRule{PathGlob: "src/[", SPDX: "MIT"}))
	err := check(c, outsider, OpRead, "docs/a")
	var pd *domain.PushDenied
	if err == nil || errors.As(err, &pd) || !strings.Contains(err.Error(), "invalid license rule") {
		t.Fatalf("err = %v", err)
	}
}

func TestArgumentErrors(t *testing.T) {
	c := New(newStore())
	if err := c.CheckRead(context.Background(), outsider, nil, []string{"a"}); !errors.Is(err, domain.ErrInvalid) {
		t.Errorf("nil tree: %v", err)
	}
	if err := check(c, outsider, Operation("delete"), "a"); !errors.Is(err, domain.ErrInvalid) {
		t.Errorf("unknown op: %v", err)
	}
	if err := New(nil).CheckPush(context.Background(), outsider, tree, []string{"a"}); err == nil {
		t.Error("nil store: want error")
	}
}

func TestPersonalTenantRequiresMatch(t *testing.T) {
	// A principal with an empty personal tenant must not be treated as the
	// owner of a tree with an empty tenant id.
	orphan := &domain.Tree{ID: tree.ID, TenantID: ""}
	c := New(newStore(domain.LicenseRule{PathGlob: "**", SPDX: "Proprietary"}))
	p := &domain.Principal{AccountID: "x"}
	if err := c.CheckRead(context.Background(), p, orphan, []string{"a"}); err == nil {
		t.Fatal("expected denial")
	}
}

func TestValidate(t *testing.T) {
	tests := []struct {
		name string
		rule domain.LicenseRule
		ok   bool
	}{
		{"simple", domain.LicenseRule{PathGlob: "vendor/**", SPDX: "Apache-2.0", Grant: GrantRedistribute}, true},
		{"no grant", domain.LicenseRule{PathGlob: "src", SPDX: "MIT"}, true},
		{"read-only proprietary", domain.LicenseRule{PathGlob: "services/billing/", SPDX: "LicenseRef-Proprietary", Grant: GrantReadOnly}, true},
		{"modify", domain.LicenseRule{PathGlob: "**/*.rs", SPDX: "MIT OR Apache-2.0", Grant: GrantModify}, true},
		{"with exception", domain.LicenseRule{PathGlob: "lib/?/*", SPDX: "GPL-2.0-only WITH Classpath-exception-2.0"}, true},
		{"proprietary keyword", domain.LicenseRule{PathGlob: "x", SPDX: "Proprietary"}, true},
		{"class", domain.LicenseRule{PathGlob: "src/[a-c]*.go", SPDX: "MIT"}, true},
		{"empty glob", domain.LicenseRule{PathGlob: "", SPDX: "MIT"}, false},
		{"blank glob", domain.LicenseRule{PathGlob: "  ", SPDX: "MIT"}, false},
		{"dotdot glob", domain.LicenseRule{PathGlob: "a/../b", SPDX: "MIT"}, false},
		{"bad class", domain.LicenseRule{PathGlob: "src/[a-", SPDX: "MIT"}, false},
		{"nul", domain.LicenseRule{PathGlob: "a\x00b", SPDX: "MIT"}, false},
		{"too long", domain.LicenseRule{PathGlob: strings.Repeat("a", maxGlobLen+1), SPDX: "MIT"}, false},
		{"unknown grant", domain.LicenseRule{PathGlob: "a", SPDX: "MIT", Grant: "write"}, false},
		{"mis-cased grant", domain.LicenseRule{PathGlob: "a", SPDX: "MIT", Grant: "Modify"}, false},
		{"empty spdx", domain.LicenseRule{PathGlob: "a", SPDX: ""}, false},
		{"bad spdx", domain.LicenseRule{PathGlob: "a", SPDX: "MIT or Apache-2.0"}, false},
		{"bad ref", domain.LicenseRule{PathGlob: "a", SPDX: "LicenseRef-"}, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := Validate(tt.rule)
			if tt.ok && err != nil {
				t.Fatalf("Validate = %v, want nil", err)
			}
			if !tt.ok && !errors.Is(err, domain.ErrInvalid) {
				t.Fatalf("Validate = %v, want ErrInvalid", err)
			}
		})
	}
}
