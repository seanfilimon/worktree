package syncsvc

// history-v2 (§16.10): force pushes, protection bypass, push idempotency,
// the server reflog and DeleteTag, through the real meta store, authorizer
// and review service.

import (
	"context"
	"strings"
	"testing"
	"time"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

type forceFixture struct {
	t       *testing.T
	ctx     context.Context
	store   *meta.Memory
	objects *objstore.DiskStore
	svc     *Service
	bus     *fakeBus
	audit   *fakeAudit
	tree    *domain.Tree
	users   map[string]*domain.Principal
}

// newForceFixture: tenant acme with olivia (owner), adam (admin), dave
// (developer) and vera (viewer); tree "web".
func newForceFixture(t *testing.T) *forceFixture {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	hist := meta.NewHistory(store)
	az := authz.New(store, authz.Options{})
	f := &forceFixture{t: t, ctx: ctx, store: store, objects: objstore.New(t.TempDir()),
		bus: &fakeBus{}, audit: &fakeAudit{}, users: map[string]*domain.Principal{}}
	rev := review.New(store, hist, az, f.bus, f.audit)
	f.svc = New(Deps{Store: store, Objects: f.objects, History: hist, Authz: az, Review: rev,
		Bus: f.bus, Audit: f.audit, PushChecks: []domain.PushChecker{rev}})
	acme := &domain.Tenant{ID: "t-acme", Slug: "acme", Kind: domain.TenantOrg}
	must(t, store.CreateTenant(ctx, acme))
	for user, role := range map[string]string{"olivia": domain.RoleOwner, "adam": domain.RoleAdmin, "dave": domain.RoleDeveloper, "vera": domain.RoleViewer} {
		acct := &domain.Account{ID: "a-" + user, Username: user, TenantID: "t-" + user}
		must(t, store.CreateAccount(ctx, acct))
		must(t, store.PutMembership(ctx, &domain.Membership{TenantID: acme.ID, AccountID: acct.ID, Role: role}))
		f.users[user] = &domain.Principal{AccountID: acct.ID, Username: user, TenantID: acct.TenantID}
	}
	f.tree = &domain.Tree{ID: "tree-web", TenantID: acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	must(t, store.CreateTree(ctx, f.tree))
	return f
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

// as returns a context for user; bypass adds the bypass metadata.
func (f *forceFixture) as(user string, bypass bool) context.Context {
	ctx := iam.WithPrincipal(f.ctx, f.users[user])
	if bypass {
		ctx = metadata.NewIncomingContext(ctx, metadata.Pairs(BypassMetadataKey, "true"))
	}
	return ctx
}

// commit stores the objects of a new snapshot and returns it.
func (f *forceFixture) commit(files map[string]string, parents ...commit) commit {
	f.t.Helper()
	c := makeCommit(f.tree.ID, files, parents...)
	for _, o := range c.objects {
		must(f.t, f.objects.Put(f.ctx, hash.Hash(o.GetHash().GetValue()), o.GetData()))
	}
	return c
}

func (f *forceFixture) push(ctx context.Context, req *syncpb.PushRequest) *syncpb.PushResponse {
	f.t.Helper()
	resp, err := f.svc.Push(ctx, req)
	if err != nil {
		f.t.Fatalf("push: %v", err)
	}
	return resp
}

func (f *forceFixture) tip(branch string) string {
	b, err := f.store.GetBranch(f.ctx, f.tree.ID, branch)
	if err != nil {
		return ""
	}
	return b.Tip
}

func (f *forceFixture) protect(p domain.BranchProtection) {
	p.TreeID = f.tree.ID
	must(f.t, f.store.PutProtection(f.ctx, &p))
}

func rejectedWith(t *testing.T, resp *syncpb.PushResponse, want string) {
	t.Helper()
	if resp.GetAccepted() {
		t.Fatalf("push accepted, want a %s rejection", want)
	}
	r := resp.GetRejectionReason()
	var got string
	switch {
	case r.GetConflictDetected() != nil:
		got = "conflict"
	case r.GetAccessDenied() != nil:
		got = "access_denied"
	case r.GetBranchProtection() != nil:
		got = "branch_protection"
	default:
		got = r.String()
	}
	if got != want {
		t.Fatalf("rejection = %s (%v), want %s", got, r, want)
	}
}

func forced(req *syncpb.PushRequest) *syncpb.PushRequest {
	req.Force = true
	return req
}

// diverged pushes c1 ← c2 to main and returns c1, c2 and c3 (a sibling of
// c2, not pushed).
func (f *forceFixture) diverged() (c1, c2, c3 commit) {
	c1 = f.commit(map[string]string{"a.txt": "1"})
	c2 = f.commit(map[string]string{"a.txt": "2"}, c1)
	if resp := f.push(f.as("dave", false), pushReq(f.tree.ID, "main", "", c1, c2)); !resp.GetAccepted() {
		f.t.Fatalf("initial push: %v", resp)
	}
	c3 = f.commit(map[string]string{"a.txt": "3"}, c1)
	return c1, c2, c3
}

func TestForcePushRewindsBranch(t *testing.T) {
	f := newForceFixture(t)
	c1, c2, c3 := f.diverged()

	// A plain push of diverged history is a conflict.
	rejectedWith(t, f.push(f.as("adam", false), pushReq(f.tree.ID, "main", c2.id, c3)), "conflict")
	// Developers lack branch:force_push.
	rejectedWith(t, f.push(f.as("dave", false), forced(pushReq(f.tree.ID, "main", c2.id, c3))), "access_denied")
	// Lease: a stale old_tip refuses the force push.
	rejectedWith(t, f.push(f.as("adam", false), forced(pushReq(f.tree.ID, "main", c1.id, c3))), "conflict")
	if f.tip("main") != c2.id {
		t.Fatal("refused pushes must not move main")
	}

	resp := f.push(f.as("adam", false), forced(pushReq(f.tree.ID, "main", c2.id, c3)))
	if !resp.GetAccepted() || resp.GetNewTip().GetValue() != c3.id || f.tip("main") != c3.id {
		t.Fatalf("force push: %v, main = %s", resp, f.tip("main"))
	}
	ev := f.bus.ofType(domain.EventBranchForced)
	if len(ev) != 1 || ev[0].Payload.(map[string]any)["old_tip"] != c2.id || ev[0].Actor != "adam" {
		t.Fatalf("force events = %+v", ev)
	}
	pushed := f.bus.ofType(domain.EventBranchPushed)
	if last := pushed[len(pushed)-1].Payload.(map[string]any); last["forced"] != true {
		t.Fatalf("branch.pushed payload = %v", last)
	}
	audits := f.audit.actions("branch.push")
	if a := audits[len(audits)-1]; a.Details["forced"] != "true" || a.Details["old_tip"] != c2.id || a.Details["bypass"] != "" {
		t.Fatalf("audit = %+v", a)
	}

	// The reflog keeps the rewound tip.
	entries, err := ListReflog(f.ctx, f.store, f.tree.ID, "main", 0)
	must(t, err)
	if len(entries) != 2 {
		t.Fatalf("reflog = %+v", entries)
	}
	if e := entries[0]; e.Kind != "force_push" || !e.Forced || e.OldTip != c2.id || e.NewTip != c3.id || e.Actor != "adam" {
		t.Fatalf("newest entry = %+v", e)
	}
	if e := entries[1]; e.Kind != "push" || e.Forced || e.OldTip != "" || e.NewTip != c2.id || e.Count != 2 {
		t.Fatalf("oldest entry = %+v", e)
	}
	if other, _ := ListReflog(f.ctx, f.store, f.tree.ID, "ma", 0); len(other) != 0 {
		t.Fatalf("branch prefix leaked: %+v", other)
	}

	// A force push without old_tip is unconditional.
	c4 := f.commit(map[string]string{"a.txt": "4"}, c1)
	if resp := f.push(f.as("adam", false), forced(pushReq(f.tree.ID, "main", "", c4))); !resp.GetAccepted() {
		t.Fatalf("unconditional force: %v", resp)
	}
	// A fast-forward with force set is an ordinary push.
	c5 := f.commit(map[string]string{"a.txt": "5"}, c4)
	if resp := f.push(f.as("dave", false), forced(pushReq(f.tree.ID, "main", c4.id, c5))); !resp.GetAccepted() {
		t.Fatalf("forced fast-forward: %v", resp)
	}
	if n := len(f.bus.ofType(domain.EventBranchForced)); n != 2 {
		t.Fatalf("force events = %d, want 2", n)
	}
}

func TestForcePushProtections(t *testing.T) {
	f := newForceFixture(t)
	_, c2, c3 := f.diverged()

	// require_linear: refused for admins, and for owners without the bypass.
	f.protect(domain.BranchProtection{Pattern: "main", RequireLinear: true})
	rejectedWith(t, f.push(f.as("adam", false), forced(pushReq(f.tree.ID, "main", c2.id, c3))), "branch_protection")
	rejectedWith(t, f.push(f.as("adam", true), forced(pushReq(f.tree.ID, "main", c2.id, c3))), "branch_protection")
	resp := f.push(f.as("olivia", false), forced(pushReq(f.tree.ID, "main", c2.id, c3)))
	rejectedWith(t, resp, "branch_protection")
	if rule := resp.GetRejectionReason().GetBranchProtection().GetRule(); !strings.Contains(rule, "require_linear") {
		t.Fatalf("rule = %q", rule)
	}
	// Owners bypass with the metadata; audited.
	if resp := f.push(f.as("olivia", true), forced(pushReq(f.tree.ID, "main", c2.id, c3))); !resp.GetAccepted() {
		t.Fatalf("owner bypass: %v", resp)
	}
	bypass := f.audit.actions(AuditProtectionBypassed)
	if len(bypass) != 1 || bypass[0].Details["operation"] != "force_push" || bypass[0].Details["bypass"] != "true" || bypass[0].Actor != "olivia" {
		t.Fatalf("bypass audit = %+v", bypass)
	}
	entries, _ := ListReflog(f.ctx, f.store, f.tree.ID, "main", 1)
	if len(entries) != 1 || !entries[0].Bypass || !entries[0].Forced || len(entries[0].Bypassed) == 0 {
		t.Fatalf("reflog = %+v", entries)
	}

	// no_force_push is absolute, even for owners bypassing protection.
	f.protect(domain.BranchProtection{Pattern: "main", NoForcePush: true})
	c4 := f.commit(map[string]string{"a.txt": "4"}, c2)
	rejectedWith(t, f.push(f.as("olivia", true), forced(pushReq(f.tree.ID, "main", c3.id, c4))), "branch_protection")
	if f.tip("main") != c3.id {
		t.Fatal("main moved")
	}
}

func TestBypassDirectPush(t *testing.T) {
	f := newForceFixture(t)
	f.protect(domain.BranchProtection{Pattern: "main", NoDirectPush: true})
	c1 := f.commit(map[string]string{"a.txt": "1"})
	rejectedWith(t, f.push(f.as("olivia", false), pushReq(f.tree.ID, "main", "", c1)), "branch_protection")
	rejectedWith(t, f.push(f.as("adam", true), pushReq(f.tree.ID, "main", "", c1)), "branch_protection")
	if resp := f.push(f.as("olivia", true), pushReq(f.tree.ID, "main", "", c1)); !resp.GetAccepted() {
		t.Fatalf("owner bypass: %v", resp)
	}
	pushes := f.audit.actions("branch.push")
	if len(pushes) != 1 || pushes[0].Details["bypass"] != "true" || !strings.Contains(pushes[0].Details["bypassed"], "merge request") {
		t.Fatalf("push audit = %+v", pushes)
	}
	if b := f.audit.actions(AuditProtectionBypassed); len(b) != 1 || b[0].Details["operation"] != "push" {
		t.Fatalf("bypass audit = %+v", b)
	}
	// Unprotected branches ignore the metadata: nothing is bypassed.
	c2 := f.commit(map[string]string{"b.txt": "1"})
	if resp := f.push(f.as("olivia", true), pushReq(f.tree.ID, "dev", "", c2)); !resp.GetAccepted() {
		t.Fatal(resp)
	}
	if b := f.audit.actions(AuditProtectionBypassed); len(b) != 1 {
		t.Fatalf("bypass audit = %+v", b)
	}
}

func TestPushIdempotency(t *testing.T) {
	f := newForceFixture(t)
	c1 := f.commit(map[string]string{"a.txt": "1"})
	req := pushReq(f.tree.ID, "main", "", c1)
	req.OperationId = "op-1"
	if resp := f.push(f.as("dave", false), req); !resp.GetAccepted() {
		t.Fatal(resp)
	}
	// The branch moves on; a retry of op-1 returns the first answer without
	// pushing again.
	c2 := f.commit(map[string]string{"a.txt": "2"}, c1)
	f.push(f.as("dave", false), pushReq(f.tree.ID, "main", c1.id, c2))
	before := len(f.audit.actions("branch.push"))
	resp := f.push(f.as("dave", false), req)
	if !resp.GetAccepted() || resp.GetNewTip().GetValue() != c1.id || f.tip("main") != c2.id {
		t.Fatalf("replay = %v, main = %s", resp, f.tip("main"))
	}
	if after := len(f.audit.actions("branch.push")); after != before {
		t.Fatal("a replayed push must not be executed again")
	}
	// Rejections are replayed too.
	c3 := f.commit(map[string]string{"a.txt": "3"}, c1)
	stale := pushReq(f.tree.ID, "main", c1.id, c3)
	stale.OperationId = "op-2"
	rejectedWith(t, f.push(f.as("dave", false), stale), "conflict")
	rejectedWith(t, f.push(f.as("dave", false), stale), "conflict")
	// Another account's id is not shared.
	other := pushReq(f.tree.ID, "main", c2.id, c3)
	other.OperationId = "op-1"
	other.Force = true
	if resp := f.push(f.as("adam", false), other); !resp.GetAccepted() || f.tip("main") != c3.id {
		t.Fatalf("adam op-1: %v", resp)
	}
	// Entries expire after 24 h.
	f.svc.now = func() time.Time { return time.Now().UTC().Add(PushOpTTL + time.Minute) }
	if resp := f.svc.replayPush(f.ctx, f.users["dave"], f.tree.ID, "op-1"); resp != nil {
		t.Fatalf("expired op replayed: %v", resp)
	}
	f.svc.pruneExpired(f.ctx)
	if raws, _ := f.store.ListDocs(f.ctx, PushOpsCollection, ""); len(raws) != 0 {
		t.Fatalf("push_ops after prune: %d", len(raws))
	}
	// Malformed ids are refused.
	bad := pushReq(f.tree.ID, "main", c3.id, c3)
	bad.OperationId = "has spaces"
	_, err := f.svc.Push(f.as("dave", false), bad)
	wantCode(t, err, codes.InvalidArgument)
	bad.OperationId = strings.Repeat("x", maxOperationID+1)
	_, err = f.svc.Push(f.as("dave", false), bad)
	wantCode(t, err, codes.InvalidArgument)
}

func TestReflogRetentionAndTreeDeletion(t *testing.T) {
	f := newForceFixture(t)
	c1 := f.commit(map[string]string{"a.txt": "1"})
	req := pushReq(f.tree.ID, "main", "", c1)
	req.OperationId = "op"
	f.push(f.as("dave", false), req)
	if entries, _ := ListReflog(f.ctx, f.store, f.tree.ID, "", 0); len(entries) != 1 {
		t.Fatalf("reflog = %+v", entries)
	}
	// Entries older than 90 days are pruned.
	f.svc.now = func() time.Time { return time.Now().UTC().Add(ReflogTTL + time.Hour) }
	f.svc.pruneExpired(f.ctx)
	if entries, _ := ListReflog(f.ctx, f.store, f.tree.ID, "", 0); len(entries) != 0 {
		t.Fatalf("reflog after prune = %+v", entries)
	}
	// Deleting the tree removes its documents.
	f.svc.now = func() time.Time { return time.Now().UTC() }
	c2 := f.commit(map[string]string{"a.txt": "2"}, c1)
	req = pushReq(f.tree.ID, "main", c1.id, c2)
	req.OperationId = "op2"
	f.push(f.as("dave", false), req)
	f.svc.deleteTreeDocs(f.ctx, f.tree.ID)
	for _, coll := range []string{PushOpsCollection, ReflogCollection} {
		if raws, _ := f.store.ListDocs(f.ctx, coll, ""); len(raws) != 0 {
			t.Fatalf("%s after tree deletion: %d", coll, len(raws))
		}
	}
}

func TestDeleteTagRPC(t *testing.T) {
	f := newForceFixture(t)
	c1 := f.commit(map[string]string{"a.txt": "1"})
	f.push(f.as("dave", false), pushReq(f.tree.ID, "main", "", c1))
	for _, name := range []string{"v1", "v2"} {
		resp, err := f.svc.PushTag(f.as("dave", false), &syncpb.PushTagRequest{TreeId: &syncpb.TreeId{Value: f.tree.ID},
			Tag: &syncpb.TagRecord{Name: name, Target: &syncpb.SnapshotId{Value: c1.id}}})
		if err != nil || !resp.GetAccepted() {
			t.Fatalf("push tag %s: %v %v", name, resp, err)
		}
	}
	del := func(user, name string) (*syncpb.DeleteTagResponse, error) {
		return f.svc.DeleteTag(f.as(user, false), &syncpb.DeleteTagRequest{TreeId: &syncpb.TreeId{Value: f.tree.ID}, Name: name})
	}
	// Viewers cannot delete tags.
	_, err := del("vera", "v1")
	wantCode(t, err, codes.PermissionDenied)
	// Tag protections apply.
	rv := review.New(f.store, meta.NewHistory(f.store), authz.New(f.store, authz.Options{}), nil, nil)
	if _, err := rv.PutTagProtection(f.ctx, f.users["olivia"], f.tree, domain.TagProtection{Pattern: "v2", NoDelete: true}); err != nil {
		t.Fatal(err)
	}
	resp, err := del("dave", "v2")
	if err != nil || resp.GetDeleted() || !strings.Contains(resp.GetError(), "cannot be deleted") {
		t.Fatalf("protected: %v %v", resp, err)
	}
	if len(f.audit.actions("tag.delete_rejected")) != 1 {
		t.Fatal("refusal not audited")
	}
	// Developers delete unprotected tags.
	resp, err = del("dave", "v1")
	if err != nil || !resp.GetDeleted() {
		t.Fatalf("delete: %v %v", resp, err)
	}
	if _, err := f.store.GetTag(f.ctx, f.tree.ID, "v1"); err == nil {
		t.Fatal("tag still stored")
	}
	if len(f.bus.ofType(EventTagDeleted)) != 1 || len(f.audit.actions("tag.delete")) != 1 {
		t.Fatal("event or audit missing")
	}
	resp, err = del("dave", "v1")
	if err != nil || resp.GetDeleted() || !strings.Contains(resp.GetError(), "not found") {
		t.Fatalf("missing: %v %v", resp, err)
	}
	_, err = del("dave", "")
	wantCode(t, err, codes.InvalidArgument)
}
