package treeconfig

import (
	"context"
	"errors"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

type recAudit struct {
	mu     sync.Mutex
	events []domain.AuditEvent
}

func (a *recAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.events = append(a.events, e)
}

func (a *recAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, nil
}

func (a *recAudit) actions() []string {
	a.mu.Lock()
	defer a.mu.Unlock()
	var out []string
	for _, e := range a.events {
		out = append(out, e.Action)
	}
	return out
}

type env struct {
	t     *testing.T
	ctx   context.Context
	store *meta.Memory
	objs  objstore.Store
	svc   *Service
	audit *recAudit
	bus   *events.Bus
	acme  *domain.Tenant
	web   *domain.Tree
	users map[string]*domain.Principal
	n     int
}

func newEnv(t *testing.T) *env {
	t.Helper()
	ctx := context.Background()
	st := meta.NewMemory()
	e := &env{t: t, ctx: ctx, store: st, objs: objstore.New(t.TempDir()), audit: &recAudit{}, bus: events.NewBus(), users: map[string]*domain.Principal{}}
	az := authz.New(st, authz.Options{})
	e.svc = New(Deps{Store: st, Objects: e.objs, Authz: az, Audit: e.audit, Bus: e.bus})
	e.svc.SetAccessConfig(accesscfg.New(accesscfg.Deps{Store: st, Authz: az, Audit: e.audit, Bus: e.bus}))
	e.acme = &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg}
	e.must(st.CreateTenant(ctx, e.acme))
	e.web = &domain.Tree{ID: "tree-web", TenantID: e.acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	e.must(st.CreateTree(ctx, e.web))
	for name, role := range map[string]string{"olga": domain.RoleOwner, "adam": domain.RoleAdmin, "dev": domain.RoleDeveloper, "view": domain.RoleViewer} {
		acct := &domain.Account{ID: "acct-" + name, Username: name, Email: name + "@example.com", TenantID: "t-" + name}
		e.must(st.CreateAccount(ctx, acct))
		e.must(st.PutMembership(ctx, &domain.Membership{TenantID: e.acme.ID, AccountID: acct.ID, Role: role}))
		e.users[name] = &domain.Principal{AccountID: acct.ID, Username: name, TenantID: acct.TenantID}
	}
	e.must(st.CreateTeam(ctx, &domain.Team{ID: "team-core", TenantID: e.acme.ID, Slug: "core", Name: "Core"}))
	return e
}

func (e *env) must(err error) {
	e.t.Helper()
	if err != nil {
		e.t.Fatal(err)
	}
}

// snapshot stores a snapshot of files (parent: parent) and returns it.
func (e *env) snapshot(parent string, files map[string]string) *domain.Snapshot {
	e.t.Helper()
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	m := &syncpb.ManifestRecord{}
	for _, p := range paths {
		data := []byte(files[p])
		h := hash.Sum(data)
		e.must(e.objs.Put(e.ctx, h, data))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data))})
	}
	raw, err := proto.Marshal(m)
	e.must(err)
	mh := hash.Sum(raw)
	e.must(e.objs.Put(e.ctx, mh, raw))
	e.n++
	snap := &domain.Snapshot{ID: strings.Repeat("0", 30) + string(rune('a'+e.n/10)) + string(rune('a'+e.n%10)), TreeID: e.web.ID, BranchName: "main", ManifestHash: mh, CreatedAt: time.Now()}
	if parent != "" {
		snap.Parents = []string{parent}
	}
	return snap
}

// changed lists the paths that differ between two file sets.
func changed(before, after map[string]string) []string {
	var out []string
	for p, v := range after {
		if b, ok := before[p]; !ok || b != v {
			out = append(out, p)
		}
	}
	for p := range before {
		if _, ok := after[p]; !ok {
			out = append(out, p)
		}
	}
	sort.Strings(out)
	return out
}

// push simulates syncsvc.Push: the check, then (when accepted) storing the
// snapshot, moving the tip and the branch.pushed event.
func (e *env) push(as, branch string, before, after map[string]string) error {
	e.t.Helper()
	b, err := e.store.GetBranch(e.ctx, e.web.ID, branch)
	oldTip := ""
	if err == nil {
		oldTip = b.Tip
	}
	snap := e.snapshot(oldTip, after)
	plan := &domain.PushPlan{Branch: branch, OldTip: oldTip, NewTip: snap.ID, NewSnapshots: []*domain.Snapshot{snap}, ChangedPaths: changed(before, after)}
	if err := e.svc.CheckPushPlan(e.ctx, e.users[as], e.web, plan); err != nil {
		return err
	}
	e.must(e.store.PutSnapshot(e.ctx, snap))
	e.must(e.store.CompareAndSwapBranchTip(e.ctx, e.web.ID, branch, oldTip, snap.ID))
	return e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventBranchPushed, TenantID: e.acme.ID, TreeID: e.web.ID, Actor: as,
		Payload: map[string]any{"branch": branch, "old_tip": oldTip, "new_tip": snap.ID}})
}

func (e *env) state() *State {
	e.t.Helper()
	st, err := e.svc.Get(e.ctx, e.users["view"], e.web)
	e.must(err)
	return st
}

func (e *env) branch(name string) *domain.Branch {
	e.t.Helper()
	b, err := e.store.GetBranch(e.ctx, e.web.ID, name)
	e.must(err)
	return b
}

func (e *env) protections() map[string]domain.BranchProtection {
	e.t.Helper()
	ps, err := e.store.ListProtections(e.ctx, e.web.ID)
	e.must(err)
	out := map[string]domain.BranchProtection{}
	for _, p := range ps {
		out[p.Pattern] = p
	}
	return out
}

func denied(t *testing.T, err error) *domain.PushDenied {
	t.Helper()
	var d *domain.PushDenied
	if !errors.As(err, &d) {
		t.Fatalf("err = %v, want a *PushDenied", err)
	}
	if d.Kind != DenyKind {
		t.Fatalf("kind = %q", d.Kind)
	}
	return d
}

const branchesV1 = `[[branch]]
name = "main"
description = "Production"
owner = "team:core"

[[branch]]
name = "release"
owner = "user:adam"
`

const protectionsV1 = `[[protection]]
pattern = "main"
no_direct_push = false
required_approvals = 1
no_delete = true

[[protection]]
pattern = "release/*"
no_delete = true
`

func TestPushAppliesBranchesAndProtections(t *testing.T) {
	e := newEnv(t)
	base := map[string]string{"README.md": "# web\n"}
	e.must(e.push("dev", "main", nil, base))
	if st := e.state(); st.Applied != nil || st.LastAttempt != nil {
		t.Fatalf("no config yet: %+v", st)
	}
	// A protection set through the API with a pattern the file also declares.
	e.must(e.store.PutProtection(e.ctx, &domain.BranchProtection{TreeID: e.web.ID, Pattern: "release/*", RequiredApprovals: 3}))
	e.must(e.store.CompareAndSwapBranchTip(e.ctx, e.web.ID, "release", "", ""))

	v1 := map[string]string{"README.md": "# web\n", BranchesFile: branchesV1, ProtectionsFile: protectionsV1}
	e.must(e.push("dev", "main", base, v1))

	main := e.branch("main")
	if main.Description != "Production" || main.Owner != "team:core" || main.Tip == "" {
		t.Errorf("main = %+v", main)
	}
	if r := e.branch("release"); r.Owner != "user:adam" || r.Description != "" {
		t.Errorf("release = %+v", r)
	}
	ps := e.protections()
	if p := ps["main"]; p.RequiredApprovals != 1 || !p.NoDelete {
		t.Errorf("main protection = %+v", p)
	}
	if p := ps["release/*"]; p.RequiredApprovals != 3 || p.NoDelete {
		t.Errorf("the API rule was overwritten: %+v", p)
	}
	st := e.state()
	if st.Applied == nil || st.Applied.SnapshotID != main.Tip || st.LastAttempt == nil || !st.LastAttempt.Accepted {
		t.Fatalf("state = %+v", st)
	}
	if strings.Join(st.Applied.ManagedPatterns, ",") != "main" {
		t.Errorf("managed = %v", st.Applied.ManagedPatterns)
	}
	if got := codes(st.Applied.Warnings); strings.Join(got, ",") != WManagedByAPI {
		t.Errorf("warnings = %v", st.Applied.Warnings)
	}
	if len(st.Applied.Files) != 2 || st.Applied.Files[0].Path != BranchesFile || st.Applied.AppliedBy != "dev" {
		t.Errorf("files = %+v", st.Applied.Files)
	}
	if !strings.Contains(strings.Join(st.Applied.Changes, "\n"), "protection main: create") {
		t.Errorf("changes = %v", st.Applied.Changes)
	}

	// Declared branches are config-managed.
	desc := "manual"
	if _, err := e.svc.UpdateBranch(e.ctx, e.users["adam"], e.web, "main", BranchUpdate{Description: &desc}); !errors.Is(err, ErrConfigManaged) {
		t.Errorf("UpdateBranch(main) = %v", err)
	}

	// v2: release leaves the file, the main protection changes, release/*
	// stays API-managed; protections.toml is removed afterwards.
	v2 := map[string]string{"README.md": "# web\n", BranchesFile: "[[branch]]\nname = \"main\"\ndescription = \"Prod\"\n",
		ProtectionsFile: "[[protection]]\npattern = \"main\"\nrequired_approvals = 2\n"}
	e.must(e.push("dev", "main", v1, v2))
	if r := e.branch("release"); r.Owner != "" {
		t.Errorf("release keeps its owner: %+v", r)
	}
	if m := e.branch("main"); m.Description != "Prod" || m.Owner != "" {
		t.Errorf("main = %+v", m)
	}
	if p := e.protections()["main"]; p.RequiredApprovals != 2 || p.NoDelete {
		t.Errorf("main protection = %+v", p)
	}
	v3 := map[string]string{"README.md": "# web\n", BranchesFile: v2[BranchesFile]}
	e.must(e.push("dev", "main", v2, v3))
	ps = e.protections()
	if _, ok := ps["main"]; ok {
		t.Error("the config-managed protection survived the removal of protections.toml")
	}
	if _, ok := ps["release/*"]; !ok {
		t.Error("the API rule was deleted")
	}
	// A push that changes no config file leaves the state alone.
	v4 := map[string]string{"README.md": "# web 2\n", BranchesFile: v2[BranchesFile]}
	before := e.state().Applied.SnapshotID
	e.must(e.push("dev", "main", v3, v4))
	if e.state().Applied.SnapshotID != before {
		t.Error("an unrelated push re-applied the configuration")
	}
	if acts := strings.Join(e.audit.actions(), ","); strings.Count(acts, "config.apply") != 3 {
		t.Errorf("audit = %s", acts)
	}
}

func TestPushRejectsInvalidConfig(t *testing.T) {
	e := newEnv(t)
	e.must(e.push("dev", "main", nil, map[string]string{"a": "1"}))
	bad := map[string]string{"a": "1", BranchesFile: "[[branch]]\nname = \"main\"\nowner = \"team:nobody\"\n", ProtectionsFile: "[[protection]]\npattern = \"a//b\"\n"}
	d := denied(t, e.push("dev", "main", map[string]string{"a": "1"}, bad))
	if !strings.Contains(d.Reason, ".wt-tree/branches.toml:3:1: E3008 owner team:nobody does not exist") ||
		!strings.Contains(d.Reason, ".wt-tree/protections.toml:2:1: E3005") {
		t.Errorf("reason = %s", d.Reason)
	}
	st := e.state()
	if st.Applied != nil || st.LastAttempt == nil || st.LastAttempt.Accepted || len(st.LastAttempt.Errors) != 2 {
		t.Fatalf("state = %+v", st)
	}
	// Other branches are not checked.
	if err := e.push("dev", "feature", nil, bad); err != nil {
		t.Errorf("push to a feature branch: %v", err)
	}
	if e.branch("feature").Owner != "" {
		t.Error("feature got metadata")
	}
	if !strings.Contains(strings.Join(e.audit.actions(), ","), "config.rejected") {
		t.Errorf("audit = %v", e.audit.actions())
	}
}

const policiesV1 = `[[policy]]
name = "no-bob"
[[policy.statement]]
effect = "deny"
principals = ["user:dev"]
actions = ["sync:push"]
resources = ["wt:branch/acme/web/main"]
`

func TestAccessFilesNeedIAMAdmin(t *testing.T) {
	e := newEnv(t)
	e.must(e.push("adam", "main", nil, map[string]string{"a": "1"}))
	// Notes next to the access document are not access changes.
	e.must(e.push("dev", "main", map[string]string{"a": "1"}, map[string]string{"a": "1", AccessDir + "README.md": "notes"}))
	e.must(e.push("dev", "main", map[string]string{"a": "1", AccessDir + "README.md": "notes"}, map[string]string{"a": "1"}))
	withAccess := map[string]string{"a": "1", PoliciesFile: policiesV1}
	d := denied(t, e.push("dev", "main", map[string]string{"a": "1"}, withAccess))
	if !strings.Contains(d.Reason, "E3010") || !strings.Contains(d.Reason, "iam:admin") {
		t.Errorf("reason = %s", d.Reason)
	}
	// An admin may; the tree's policy is created.
	e.must(e.push("adam", "main", map[string]string{"a": "1"}, withAccess))
	ps, err := e.store.ListPolicies(e.ctx, e.acme.ID)
	e.must(err)
	if len(ps) != 1 || ps[0].Name != "web/no-bob" || ps[0].TreeID != e.web.ID || ps[0].Statements[0].Actions[0] != "branch:push" {
		t.Fatalf("policies = %+v", ps)
	}
	st := e.state()
	if st.Applied == nil || st.Applied.AccessHash == "" || !strings.Contains(strings.Join(st.Applied.Changes, ","), "access policy no-bob: create") {
		t.Errorf("applied = %+v", st.Applied)
	}
	// A semantically invalid document is refused by accesscfg (located).
	bad := map[string]string{"a": "1", PoliciesFile: "[[policy]]\nname = \"x\"\n[[policy.statement]]\neffect = \"allow\"\nprincipals = [\"*\"]\nactions = [\"tree:read\"]\nresources = [\"*\"]\n"}
	d = denied(t, e.push("adam", "main", withAccess, bad))
	if !strings.Contains(d.Reason, ".wt-tree/access/policies.toml:3:1: E3011") {
		t.Errorf("reason = %s", d.Reason)
	}
	// Removing the file removes the tree's policies.
	e.must(e.push("adam", "main", withAccess, map[string]string{"a": "1"}))
	ps, err = e.store.ListPolicies(e.ctx, e.acme.ID)
	e.must(err)
	if len(ps) != 0 {
		t.Errorf("policies = %+v", ps)
	}
	if e.state().Applied.AccessHash != "" {
		t.Error("access hash kept")
	}
}

func TestMergeAppliesWithActor(t *testing.T) {
	e := newEnv(t)
	e.must(e.push("dev", "main", nil, map[string]string{"a": "1"}))
	main := e.branch("main")
	// A server-side merge: no CheckPushPlan, the event names the merger.
	snap := e.snapshot(main.Tip, map[string]string{"a": "1", BranchesFile: "[[branch]]\nname = \"main\"\ndescription = \"merged\"\n"})
	e.must(e.store.PutSnapshot(e.ctx, snap))
	e.must(e.store.CompareAndSwapBranchTip(e.ctx, e.web.ID, "main", main.Tip, snap.ID))
	e.must(e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventBranchPushed, TreeID: e.web.ID, Actor: "olga",
		Payload: map[string]any{"branch": "main", "old_tip": main.Tip, "new_tip": snap.ID, "merge_request": 1}}))
	if b := e.branch("main"); b.Description != "merged" {
		t.Errorf("main = %+v", b)
	}
	if st := e.state(); st.Applied.AppliedBy != "olga" {
		t.Errorf("applied by %q", st.Applied.AppliedBy)
	}
	// An invalid merged configuration is recorded, not applied.
	bad := e.snapshot(snap.ID, map[string]string{"a": "1", BranchesFile: "[[branch]]\nnope = 1\n"})
	e.must(e.store.PutSnapshot(e.ctx, bad))
	e.must(e.store.CompareAndSwapBranchTip(e.ctx, e.web.ID, "main", snap.ID, bad.ID))
	e.must(e.svc.Apply(e.ctx, e.web, "main", snap.ID, bad.ID, "olga"))
	st := e.state()
	if st.Applied.SnapshotID != snap.ID || st.LastAttempt.Accepted || st.LastAttempt.SnapshotID != bad.ID {
		t.Errorf("state = %+v / %+v", st.Applied, st.LastAttempt)
	}
}

func TestNewBranchGetsDeclaredMetadata(t *testing.T) {
	e := newEnv(t)
	files := map[string]string{BranchesFile: "[[branch]]\nname = \"later\"\ndescription = \"comes later\"\n"}
	e.must(e.push("dev", "main", nil, files))
	st := e.state()
	if got := codes(st.Applied.Warnings); strings.Join(got, ",") != WNoSuchBranch {
		t.Errorf("warnings = %v", st.Applied.Warnings)
	}
	e.must(e.push("dev", "later", nil, files))
	if b := e.branch("later"); b.Description != "comes later" {
		t.Errorf("later = %+v", b)
	}
	// Created over REST.
	e.must(e.push("dev", "main", files, map[string]string{BranchesFile: files[BranchesFile] + "\n[[branch]]\nname = \"rest\"\nowner = \"user:dev\"\n"}))
	e.must(e.store.CompareAndSwapBranchTip(e.ctx, e.web.ID, "rest", "", ""))
	e.must(e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventBranchCreated, TreeID: e.web.ID, Payload: domain.Branch{TreeID: e.web.ID, Name: "rest"}}))
	if b := e.branch("rest"); b.Owner != "user:dev" {
		t.Errorf("rest = %+v", b)
	}
}

func TestUpdateBranch(t *testing.T) {
	e := newEnv(t)
	e.must(e.push("dev", "main", nil, map[string]string{"a": "1"}))
	desc, owner, bad, nobody, empty := "Main line", "team:core", "group:x", "user:nobody", ""
	b, err := e.svc.UpdateBranch(e.ctx, e.users["dev"], e.web, "main", BranchUpdate{Description: &desc})
	e.must(err)
	if b.Description != "Main line" || b.Tip == "" {
		t.Errorf("branch = %+v", b)
	}
	if _, err := e.svc.UpdateBranch(e.ctx, e.users["dev"], e.web, "main", BranchUpdate{Owner: &owner}); !errors.Is(err, domain.ErrForbidden) {
		t.Errorf("developer sets owner: %v", err)
	}
	if _, err := e.svc.UpdateBranch(e.ctx, e.users["view"], e.web, "main", BranchUpdate{Description: &desc}); !errors.Is(err, domain.ErrForbidden) {
		t.Errorf("viewer sets description: %v", err)
	}
	b, err = e.svc.UpdateBranch(e.ctx, e.users["adam"], e.web, "main", BranchUpdate{Owner: &owner})
	e.must(err)
	if b.Owner != "team:core" || b.Description != "Main line" {
		t.Errorf("branch = %+v", b)
	}
	for _, o := range []*string{&bad, &nobody} {
		if _, err := e.svc.UpdateBranch(e.ctx, e.users["adam"], e.web, "main", BranchUpdate{Owner: o}); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("owner %s: %v", *o, err)
		}
	}
	b, err = e.svc.UpdateBranch(e.ctx, e.users["adam"], e.web, "main", BranchUpdate{Owner: &empty})
	e.must(err)
	if b.Owner != "" {
		t.Errorf("owner not cleared: %+v", b)
	}
	if _, err := e.svc.UpdateBranch(e.ctx, e.users["adam"], e.web, "nope", BranchUpdate{Description: &desc}); !errors.Is(err, domain.ErrNotFound) {
		t.Errorf("missing branch: %v", err)
	}
	if _, err := e.svc.UpdateBranch(e.ctx, e.users["adam"], e.web, "main", BranchUpdate{}); !errors.Is(err, domain.ErrInvalid) {
		t.Errorf("empty update: %v", err)
	}
	long := strings.Repeat("x", MaxDescription+1)
	if _, err := e.svc.UpdateBranch(e.ctx, e.users["adam"], e.web, "main", BranchUpdate{Description: &long}); !errors.Is(err, domain.ErrInvalid) {
		t.Errorf("long description: %v", err)
	}
}

func TestValidateFilesAndTreeDeletion(t *testing.T) {
	e := newEnv(t)
	res, err := e.svc.ValidateFiles(e.ctx, e.users["dev"], e.web, map[string]string{
		BranchesFile: "[[branch]]\nname = \"main\"\nowner = \"user:ghost\"\n",
		ConfigFile:   "[weird]\n",
		PoliciesFile: policiesV1,
	})
	e.must(err)
	if res.Valid || strings.Join(codes(res.Diagnostics), ",") != "E3008,E3010,W2101,W3103" {
		t.Errorf("result = %+v", res)
	}
	res, err = e.svc.ValidateFiles(e.ctx, e.users["adam"], e.web, map[string]string{PoliciesFile: policiesV1, ProtectionsFile: protectionsV1})
	e.must(err)
	if !res.Valid || len(res.AccessChanges) != 1 || len(res.Protections) != 2 {
		t.Errorf("result = %+v", res)
	}
	if ps, _ := e.store.ListPolicies(e.ctx, e.acme.ID); len(ps) != 0 {
		t.Error("validation applied the access document")
	}
	if _, err := e.svc.ValidateFiles(e.ctx, e.users["dev"], e.web, map[string]string{"README.md": "x"}); !errors.Is(err, domain.ErrInvalid) {
		t.Errorf("non-config path: %v", err)
	}
	if _, err := e.svc.ValidateFiles(e.ctx, e.users["view"], e.web, map[string]string{ConfigFile: ""}); !errors.Is(err, domain.ErrForbidden) {
		t.Errorf("viewer: %v", err)
	}

	e.must(e.push("dev", "main", nil, map[string]string{BranchesFile: "[[branch]]\nname = \"main\"\n"}))
	if err := e.store.GetDoc(e.ctx, Collection, e.web.ID, &State{}); err != nil {
		t.Fatal(err)
	}
	e.must(e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventTreeDeleted, TreeID: e.web.ID}))
	if err := e.store.GetDoc(e.ctx, Collection, e.web.ID, &State{}); !errors.Is(err, domain.ErrNotFound) {
		t.Errorf("tree_config survived tree deletion: %v", err)
	}
}

func TestStartAppliesFromBus(t *testing.T) {
	e := newEnv(t)
	ctx, cancel := context.WithCancel(e.ctx)
	defer cancel()
	e.svc.Start(ctx)
	ch, unsub := e.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventConfigApplied}})
	defer unsub()
	snap := e.snapshot("", map[string]string{BranchesFile: "[[branch]]\nname = \"main\"\ndescription = \"bus\"\n"})
	e.must(e.store.PutSnapshot(e.ctx, snap))
	e.must(e.store.CompareAndSwapBranchTip(e.ctx, e.web.ID, "main", "", snap.ID))
	time.Sleep(20 * time.Millisecond) // let the subscriber start
	e.bus.Publish(domain.Event{Type: domain.EventBranchPushed, TreeID: e.web.ID, Actor: "dev",
		Payload: map[string]any{"branch": "main", "old_tip": "", "new_tip": snap.ID}})
	select {
	case ev := <-ch:
		if ev.TreeID != e.web.ID {
			t.Errorf("event = %+v", ev)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("no config.applied event")
	}
	if b := e.branch("main"); b.Description != "bus" {
		t.Errorf("main = %+v", b)
	}
}
