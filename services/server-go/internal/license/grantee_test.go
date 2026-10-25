package license

import (
	"context"
	"errors"
	"slices"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

var gNow = time.Date(2026, 6, 1, 12, 0, 0, 0, time.UTC)

// granteeWorld: tree acme/app; olga's personal tenant "olga" and org
// "partner" (olga is a member); pete's personal tenant "pete"; the service
// account bot of "partner".
type granteeWorld struct {
	c                *Checker
	store            *meta.Memory
	tree             *domain.Tree
	olga, pete, bot  *domain.Principal
	anonymous, owner *domain.Principal
}

func newGranteeWorld(t *testing.T, rules ...domain.LicenseRule) *granteeWorld {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	mustNil := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
	}
	for _, tn := range []domain.Tenant{
		{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg},
		{ID: "t-partner", Slug: "partner", Name: "Partner", Kind: domain.TenantOrg},
		{ID: "t-olga", Slug: "olga", Name: "olga", Kind: domain.TenantUser},
		{ID: "t-pete", Slug: "pete", Name: "pete", Kind: domain.TenantUser},
	} {
		tn := tn
		tn.CreatedAt = gNow
		mustNil(store.CreateTenant(ctx, &tn))
	}
	mustNil(store.PutMembership(ctx, &domain.Membership{TenantID: "t-partner", AccountID: "a-olga", Role: domain.RoleViewer, CreatedAt: gNow}))
	tree := &domain.Tree{ID: "tree-app", TenantID: "t-acme", Name: "app", DefaultBranch: "main", Visibility: domain.VisibilityPublic, CreatedAt: gNow, UpdatedAt: gNow}
	mustNil(store.CreateTree(ctx, tree))
	for i := range rules {
		rules[i].TreeID = tree.ID
		mustNil(store.PutLicenseRule(ctx, &rules[i]))
	}
	c := New(store)
	c.now = func() time.Time { return gNow }
	return &granteeWorld{
		c:     c,
		store: store,
		tree:  tree,
		olga:  &domain.Principal{AccountID: "a-olga", Username: "olga", TenantID: "t-olga"},
		pete:  &domain.Principal{AccountID: "a-pete", Username: "pete", TenantID: "t-pete"},
		bot:   &domain.Principal{Username: "sa:partner/bot", TenantID: "t-partner", Kind: domain.PrincipalServiceAccount, ServiceAccountID: "sa-bot"},
		owner: &domain.Principal{AccountID: "a-acme", Username: "acme", TenantID: "t-acme"},
	}
}

func at(d time.Duration) *time.Time {
	v := gNow.Add(d)
	return &v
}

func allowed(err error) bool { return err == nil }

func TestGranteeRules(t *testing.T) {
	w := newGranteeWorld(t,
		domain.LicenseRule{PathGlob: "**", SPDX: "LicenseRef-Proprietary"},
		domain.LicenseRule{PathGlob: "src/**", SPDX: "LicenseRef-Proprietary", Grant: GrantModify, Grantee: "partner"},
		domain.LicenseRule{PathGlob: "docs", SPDX: "LicenseRef-Proprietary", Grant: GrantReadOnly, Grantee: "pete"},
		domain.LicenseRule{PathGlob: "ghost/**", SPDX: "MIT", Grant: GrantRedistribute, Grantee: "no-such-tenant"},
	)
	ctx := context.Background()
	cases := []struct {
		name  string
		p     *domain.Principal
		op    Operation
		path  string
		allow bool
	}{
		{"member of grantee reads", w.olga, OpRead, "src/main.go", true},
		{"member of grantee pushes", w.olga, OpWrite, "src/main.go", true},
		{"modify grant forbids export", w.olga, OpExport, "src/main.go", false},
		{"grant does not cover other paths", w.olga, OpRead, "README.md", false},
		{"service account of grantee", w.bot, OpWrite, "src/x/y.go", true},
		{"personal tenant grantee", w.pete, OpRead, "docs/guide.md", true},
		{"personal grantee read-only", w.pete, OpWrite, "docs/guide.md", false},
		{"other tenant's grant does not apply", w.pete, OpRead, "src/main.go", false},
		{"anonymous gets the general rule", w.anonymous, OpRead, "src/main.go", false},
		{"unknown grantee matches nobody", w.olga, OpRead, "ghost/a", false},
		{"owner unaffected", w.owner, OpExport, "README.md", true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := w.c.Check(ctx, tc.p, w.tree, tc.op, []string{tc.path})
			if got := allowed(err); got != tc.allow {
				t.Fatalf("Check(%s %s) = %v, want allowed=%v", tc.op, tc.path, err, tc.allow)
			}
			var d *domain.PushDenied
			if err != nil && !errors.As(err, &d) {
				t.Fatalf("Check error %v is not *PushDenied", err)
			}
		})
	}
}

func TestGranteeRuleMoreSpecificWithinTenant(t *testing.T) {
	// A grantee "**" redistribute grant overrides a more specific general
	// restriction: grantee rules take precedence for their tenant.
	w := newGranteeWorld(t,
		domain.LicenseRule{PathGlob: "secret/**", SPDX: "LicenseRef-Proprietary"},
		domain.LicenseRule{PathGlob: "**", SPDX: "LicenseRef-Proprietary", Grant: GrantRedistribute, Grantee: "partner"},
		domain.LicenseRule{PathGlob: "secret/keys", SPDX: "LicenseRef-Proprietary", Grant: GrantReadOnly, Grantee: "partner"},
	)
	ctx := context.Background()
	if err := w.c.CheckExport(ctx, w.olga, w.tree, []string{"secret/a.txt"}); err != nil {
		t.Fatalf("grantee ** grant: %v", err)
	}
	// Within the grantee's rules the most specific wins.
	if err := w.c.CheckPush(ctx, w.olga, w.tree, []string{"secret/keys/k1"}); err == nil {
		t.Fatal("more specific read-only grantee rule was not applied")
	}
	if err := w.c.CheckRead(ctx, w.pete, w.tree, []string{"secret/a.txt"}); err == nil {
		t.Fatal("non-grantee was not restricted")
	}
}

func TestExpiredRules(t *testing.T) {
	w := newGranteeWorld(t,
		domain.LicenseRule{PathGlob: "**", SPDX: "LicenseRef-Proprietary"},
		domain.LicenseRule{PathGlob: "src/**", SPDX: "LicenseRef-Proprietary", Grant: GrantModify, Grantee: "partner", ExpiresAt: at(-time.Second)},
		domain.LicenseRule{PathGlob: "lib/**", SPDX: "LicenseRef-Proprietary", Grant: GrantModify, Grantee: "partner", ExpiresAt: at(time.Hour)},
		domain.LicenseRule{PathGlob: "open/**", SPDX: "LicenseRef-Proprietary", ExpiresAt: at(0)}, // expires exactly now
	)
	ctx := context.Background()
	if err := w.c.CheckRead(ctx, w.olga, w.tree, []string{"src/a.go"}); err == nil {
		t.Fatal("expired grant still applies")
	}
	if err := w.c.CheckPush(ctx, w.olga, w.tree, []string{"lib/a.go"}); err != nil {
		t.Fatalf("live grant: %v", err)
	}
	// An expired restriction lifts: open/** falls back to the general **
	// rule (still proprietary here).
	if err := w.c.CheckRead(ctx, w.pete, w.tree, []string{"open/a"}); err == nil {
		t.Fatal("fallback rule not applied")
	}

	// When every rule has expired the tree is unrestricted.
	w2 := newGranteeWorld(t, domain.LicenseRule{PathGlob: "**", SPDX: "LicenseRef-Proprietary", ExpiresAt: at(-time.Minute)})
	if err := w2.c.CheckExport(ctx, w2.pete, w2.tree, []string{"a", "b/c"}); err != nil {
		t.Fatalf("expired-only rules: %v", err)
	}
	// The clock moves: the grant on lib/** lapses an hour later.
	w.c.now = func() time.Time { return gNow.Add(2 * time.Hour) }
	if err := w.c.CheckPush(ctx, w.olga, w.tree, []string{"lib/a.go"}); err == nil {
		t.Fatal("grant applies after its expiry")
	}
}

func TestUnreadablePaths(t *testing.T) {
	w := newGranteeWorld(t,
		domain.LicenseRule{PathGlob: "secret/**", SPDX: "LicenseRef-Proprietary"},
		domain.LicenseRule{PathGlob: "vendor/**", SPDX: "MIT", Grant: GrantReadOnly},
		domain.LicenseRule{PathGlob: "secret/shared", SPDX: "LicenseRef-Proprietary", Grant: GrantReadOnly, Grantee: "partner"},
	)
	ctx := context.Background()
	paths := []string{"README.md", "secret/a", "vendor/x", "secret/shared/b", "../escape", "secret/c/d"}

	got, err := w.c.UnreadablePaths(ctx, w.pete, w.tree, paths)
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{"secret/a", "secret/shared/b", "../escape", "secret/c/d"}; !slices.Equal(got, want) {
		t.Fatalf("pete: %v, want %v", got, want)
	}
	got, err = w.c.UnreadablePaths(ctx, w.olga, w.tree, paths)
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{"secret/a", "../escape", "secret/c/d"}; !slices.Equal(got, want) {
		t.Fatalf("olga: %v, want %v", got, want)
	}
	if got, err = w.c.UnreadablePaths(ctx, w.owner, w.tree, paths); err != nil || got != nil {
		t.Fatalf("owner: %v, %v", got, err)
	}
	if got, err = w.c.UnreadablePaths(ctx, w.pete, w.tree, nil); err != nil || got != nil {
		t.Fatalf("no paths: %v, %v", got, err)
	}
	if _, err = w.c.UnreadablePaths(ctx, w.pete, nil, paths); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("nil tree: %v", err)
	}
}

func TestValidateGranteeAndExpiry(t *testing.T) {
	base := domain.LicenseRule{PathGlob: "src/**", SPDX: "MIT", Grant: GrantModify}
	ok := []domain.LicenseRule{
		base,
		{PathGlob: "src/**", SPDX: "MIT", Grant: GrantModify, Grantee: "partner"},
		{PathGlob: "src/**", SPDX: "MIT", Grant: GrantModify, Grantee: "a1-b2", ExpiresAt: at(time.Hour)},
	}
	for _, r := range ok {
		if err := Validate(r); err != nil {
			t.Errorf("Validate(%+v) = %v", r, err)
		}
	}
	var zero time.Time
	bad := []domain.LicenseRule{
		{PathGlob: "src/**", SPDX: "MIT", Grantee: "Partner"},
		{PathGlob: "src/**", SPDX: "MIT", Grantee: "-x"},
		{PathGlob: "src/**", SPDX: "MIT", Grantee: "a b"},
		{PathGlob: "src/**", SPDX: "MIT", ExpiresAt: &zero},
	}
	for _, r := range bad {
		if err := Validate(r); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("Validate(%+v) = %v, want ErrInvalid", r, err)
		}
	}
}
