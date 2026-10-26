package scan

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Test secrets are assembled at runtime so that this file holds no literal
// token.
var (
	awsKey  = "AK" + "IA" + "Z7Q4W2E8R5T1Y3U6"
	ghToken = "gh" + "p_" + "Xk3Lm9Qw2Er5Ty8Ui1Op4As7Df0Gh6Jk2Zx9"
)

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

// fakeAuthz allows the listed actions to everyone.
type fakeAuthz struct{ allow map[domain.Action]bool }

func (a *fakeAuthz) Authorize(_ context.Context, _ *domain.Principal, act domain.Action, _ string, _ *domain.Tree) error {
	if a.allow[act] {
		return nil
	}
	return domain.ErrForbidden
}
func (a *fakeAuthz) RoleOf(context.Context, string, string) (string, error) { return "", nil }

// fakeAudit records audit events.
type fakeAudit struct {
	mu     sync.Mutex
	events []domain.AuditEvent
}

func (a *fakeAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.events = append(a.events, e)
}
func (a *fakeAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, nil
}
func (a *fakeAudit) actions() []string {
	a.mu.Lock()
	defer a.mu.Unlock()
	var out []string
	for _, e := range a.events {
		out = append(out, e.Action+":"+e.Details["action"])
	}
	return out
}

type fixture struct {
	t     testing.TB
	ctx   context.Context
	store domain.MetaStore
	objs  *objstore.DiskStore
	bus   *events.Bus
	audit *fakeAudit
	authz *fakeAuthz
	svc   *Service
	tree  *domain.Tree
	p     *domain.Principal
	seq   int
}

func newFixture(t testing.TB) *fixture {
	f := &fixture{t: t, ctx: context.Background(), store: meta.NewMemory(), objs: objstore.New(t.TempDir()),
		bus: events.NewBus(), audit: &fakeAudit{}, authz: &fakeAuthz{allow: map[domain.Action]bool{}}}
	t.Cleanup(func() { _ = f.store.Close(); f.bus.Close() })
	must(t, f.store.CreateTenant(f.ctx, &domain.Tenant{ID: "ten-1", Slug: "acme", Kind: domain.TenantOrg}))
	f.tree = &domain.Tree{ID: "tree-1", TenantID: "ten-1", Name: "web", DefaultBranch: "main"}
	must(t, f.store.CreateTree(f.ctx, f.tree))
	f.svc = New(Deps{Store: f.store, Objects: f.objs, Authz: f.authz, Bus: f.bus, Audit: f.audit,
		Logger: slog.New(slog.DiscardHandler)})
	f.p = &domain.Principal{AccountID: "acct-1", Username: "alice", TenantID: "ten-alice"}
	return f
}

// entry describes one file of a manifest.
type entry struct {
	path    string
	content string
	chunked bool
}

// manifest stores the files and their manifest; it returns the manifest
// hash.
func (f *fixture) manifest(files ...entry) hash.Hash {
	sort.Slice(files, func(i, j int) bool { return files[i].path < files[j].path })
	m := &syncpb.ManifestRecord{}
	for _, e := range files {
		h := hash.Sum([]byte(e.content))
		must(f.t, f.objs.Put(f.ctx, h, []byte(e.content)))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: e.path, BlobHash: &syncpb.ContentHash{Value: h[:]},
			Size: uint64(len(e.content)), Chunked: e.chunked})
	}
	raw, err := proto.Marshal(m)
	must(f.t, err)
	mh := hash.Sum(raw)
	must(f.t, f.objs.Put(f.ctx, mh, raw))
	return mh
}

// snap returns a new (unstored) snapshot.
func (f *fixture) snap(parent string, files ...entry) *domain.Snapshot {
	f.seq++
	s := &domain.Snapshot{ID: fmt.Sprintf("00000000-0000-4000-8000-%012d", f.seq), TreeID: f.tree.ID, BranchName: "main",
		ManifestHash: f.manifest(files...)}
	if parent != "" {
		s.Parents = []string{parent}
	}
	return s
}

// store persists snapshots and moves main to the last one.
func (f *fixture) store_(snaps ...*domain.Snapshot) {
	for _, s := range snaps {
		must(f.t, f.store.PutSnapshot(f.ctx, s))
	}
	last := snaps[len(snaps)-1]
	must(f.t, f.store.PutBranch(f.ctx, &domain.Branch{TreeID: f.tree.ID, Name: "main", Tip: last.ID}))
}

func plan(oldTip string, snaps ...*domain.Snapshot) *domain.PushPlan {
	return &domain.PushPlan{Branch: "main", OldTip: oldTip, NewTip: snaps[len(snaps)-1].ID, NewSnapshots: snaps}
}

func (f *fixture) setMode(mode string) {
	_, err := f.svc.UpdateSettings(f.ctx, f.p, f.tree, SettingsPatch{Mode: &mode})
	must(f.t, err)
}

func (f *fixture) findings() []Finding {
	l, err := f.svc.ListFindings(f.ctx, f.tree.ID, "", 0)
	must(f.t, err)
	return l.Findings
}

func TestWarnModeIsTheDefaultAndRecords(t *testing.T) {
	f := newFixture(t)
	ch, cancel := f.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventSecretDetected}})
	defer cancel()
	s1 := f.snap("", entry{path: "README.md", content: "hello\n"}, entry{path: "config/aws.env", content: "# creds\nAWS_ACCESS_KEY_ID=" + awsKey + "\n"})
	if err := f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)); err != nil {
		t.Fatalf("warn mode rejected the push: %v", err)
	}
	got := f.findings()
	if len(got) != 1 {
		t.Fatalf("findings = %+v", got)
	}
	fd := got[0]
	if fd.Path != "config/aws.env" || fd.Line != 2 || fd.Rule != "aws-access-key" || fd.Status != StatusOpen ||
		fd.Action != ActionWarned || fd.Count != 1 || fd.SnapshotID != s1.ID || fd.PushedBy != "alice" || fd.Branch != "main" {
		t.Fatalf("finding = %+v", fd)
	}
	if strings.Contains(fd.Excerpt, awsKey) || !strings.Contains(fd.Excerpt, "AKIA****Y3U6") {
		t.Fatalf("excerpt not redacted: %q", fd.Excerpt)
	}
	select {
	case e := <-ch:
		pl := e.Payload.(map[string]any)
		if e.TreeID != f.tree.ID || pl["action"] != ActionWarned || pl["count"] != 1 || e.Actor != "alice" {
			t.Fatalf("event = %+v", e)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("no secret.detected event")
	}
	if a := f.audit.actions(); len(a) != 1 || a[0] != "secret.detected:warned" {
		t.Fatalf("audit = %v", a)
	}

	// The same secret pushed again updates the finding.
	f.store_(s1)
	s2 := f.snap(s1.ID, entry{path: "README.md", content: "hello again\n"}, entry{path: "config/aws.env", content: "AWS_ACCESS_KEY_ID=" + awsKey + "\n# moved\n"})
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan(s1.ID, s2)))
	got = f.findings()
	if len(got) != 1 || got[0].Count != 2 || got[0].Line != 1 || got[0].SnapshotID != s2.ID {
		t.Fatalf("after the second push: %+v", got)
	}
}

func TestBlockModeRejectsWithLocatedRedactedFindings(t *testing.T) {
	f := newFixture(t)
	f.setMode(ModeBlock)
	s1 := f.snap("", entry{path: "src/app.rs", content: "fn main() {}\nlet t = \"" + ghToken + "\";\n"})
	err := f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1))
	var denied *domain.PushDenied
	if !errors.As(err, &denied) || denied.Kind != KindSecretDetected {
		t.Fatalf("err = %v", err)
	}
	fp := f.findings()[0].Fingerprint
	for _, want := range []string{"src/app.rs:2", "[github-token]", "ghp_****2Zx9", fp, "wt compliance allow"} {
		if !strings.Contains(denied.Reason, want) {
			t.Errorf("reason lacks %q:\n%s", want, denied.Reason)
		}
	}
	if strings.Contains(denied.Reason, ghToken) {
		t.Fatal("the reason leaks the secret")
	}
	if fd := f.findings()[0]; fd.Action != ActionBlocked {
		t.Fatalf("finding = %+v", fd)
	}

	// branch:bypass_protection lets it through, recorded as bypassed.
	f.authz.allow[domain.ActProtectionBypass] = true
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)))
	if fd := f.findings()[0]; fd.Action != ActionBypassed || fd.Count != 2 {
		t.Fatalf("finding = %+v", fd)
	}
	if a := f.audit.actions(); len(a) != 2 || a[1] != "secret.detected:bypassed" {
		t.Fatalf("audit = %v", a)
	}
}

func TestAllowlistLetsAPushThrough(t *testing.T) {
	f := newFixture(t)
	f.setMode(ModeBlock)
	s1 := f.snap("", entry{path: "test/fixture.env", content: "KEY=" + awsKey + "\n"})
	if err := f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)); err == nil {
		t.Fatal("not blocked")
	}
	fp := f.findings()[0].Fingerprint
	if _, err := f.svc.Allow(f.ctx, f.p, f.tree, AllowRequest{Fingerprint: fp}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("allow without a reason: %v", err)
	}
	if _, err := f.svc.Allow(f.ctx, f.p, f.tree, AllowRequest{Fingerprint: "nothex", Reason: "x"}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("allow with a bad fingerprint: %v", err)
	}
	e, err := f.svc.Allow(f.ctx, f.p, f.tree, AllowRequest{Fingerprint: strings.ToUpper(fp), Reason: "test fixture"})
	must(t, err)
	if e.Fingerprint != fp || e.CreatedBy != "alice" {
		t.Fatalf("entry = %+v", e)
	}
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)))
	fd := f.findings()[0]
	if fd.Status != StatusResolved || fd.Resolution != "false_positive" || fd.Count != 1 {
		t.Fatalf("finding = %+v", fd)
	}
	// Re-adding replaces the reason; removing restores blocking.
	_, err = f.svc.Allow(f.ctx, f.p, f.tree, AllowRequest{Fingerprint: fp, Reason: "fixture key"})
	must(t, err)
	al, err := f.svc.GetAllowList(f.ctx, f.tree.ID)
	must(t, err)
	if len(al.Entries) != 1 || al.Entries[0].Reason != "fixture key" {
		t.Fatalf("allowlist = %+v", al)
	}
	must(t, f.svc.Disallow(f.ctx, f.tree, fp))
	if err := f.svc.Disallow(f.ctx, f.tree, fp); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("second disallow: %v", err)
	}
	if err := f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)); err == nil {
		t.Fatal("not blocked after removal")
	}
}

func TestOffModeAndSkippedFiles(t *testing.T) {
	f := newFixture(t)
	secret := "KEY=" + awsKey + "\n"
	big := strings.Repeat("x", 2<<20) + "\n" + secret
	s1 := f.snap("",
		entry{path: "big.txt", content: big},
		entry{path: "huge.bin", content: secret, chunked: true},
		entry{path: "fixtures/a.env", content: secret},
		entry{path: "blob.bin", content: secret + "\x00\x01"},
	)
	ex := []string{"fixtures/**"}
	_, err := f.svc.UpdateSettings(f.ctx, f.p, f.tree, SettingsPatch{Exclude: &ex})
	must(t, err)
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)))
	if got := f.findings(); len(got) != 0 {
		t.Fatalf("skipped files produced findings: %+v", got)
	}
	// A larger cap picks up big.txt.
	n := int64(4 << 20)
	_, err = f.svc.UpdateSettings(f.ctx, f.p, f.tree, SettingsPatch{MaxFileBytes: &n})
	must(t, err)
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)))
	if got := f.findings(); len(got) != 1 || got[0].Path != "big.txt" {
		t.Fatalf("findings = %+v", got)
	}
	f.setMode(ModeOff)
	s2 := f.snap("", entry{path: "x.env", content: secret})
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s2)))
	if got := f.findings(); len(got) != 1 {
		t.Fatalf("off mode recorded: %+v", got)
	}
}

func TestEveryNewSnapshotIsScanned(t *testing.T) {
	f := newFixture(t)
	base := f.snap("", entry{path: "README.md", content: "hi\n"})
	f.store_(base)
	// The secret is added in s1 and removed again in s2: it still lands in
	// history and must be found, attributed to s1.
	s1 := f.snap(base.ID, entry{path: "README.md", content: "hi\n"}, entry{path: "leak.txt", content: "t=" + ghToken + "\n"})
	s2 := f.snap(s1.ID, entry{path: "README.md", content: "hi\n"})
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan(base.ID, s1, s2)))
	got := f.findings()
	if len(got) != 1 || got[0].SnapshotID != s1.ID || got[0].Path != "leak.txt" {
		t.Fatalf("findings = %+v", got)
	}
	// Unchanged files (README.md, the base content) are not rescanned, and a
	// branch created at a stored snapshot scans nothing.
	if c, err := f.svc.collectPush(f.ctx, plan(base.ID, s1, s2), &Settings{Mode: ModeWarn}); err != nil || len(c.files) != 1 {
		t.Fatalf("collected %+v, %v", c, err)
	}
	empty := &domain.PushPlan{Branch: "feature", NewTip: base.ID}
	if c, err := f.svc.collectPush(f.ctx, empty, &Settings{Mode: ModeWarn}); err != nil || len(c.files) != 0 {
		t.Fatalf("branch creation collected %+v, %v", c, err)
	}
}

func TestSettingsValidation(t *testing.T) {
	f := newFixture(t)
	st, err := f.svc.GetSettings(f.ctx, f.tree.ID)
	must(t, err)
	if st.Mode != ModeWarn || st.Exclude == nil || st.effectiveMaxFile() != DefaultMaxFileBytes {
		t.Fatalf("defaults = %+v", st)
	}
	bad := "loud"
	if _, err := f.svc.UpdateSettings(f.ctx, f.p, f.tree, SettingsPatch{Mode: &bad}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad mode: %v", err)
	}
	n := int64(MaxMaxFileBytes + 1)
	if _, err := f.svc.UpdateSettings(f.ctx, f.p, f.tree, SettingsPatch{MaxFileBytes: &n}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad size: %v", err)
	}
	ex := []string{"a/../b"}
	if _, err := f.svc.UpdateSettings(f.ctx, f.p, f.tree, SettingsPatch{Exclude: &ex}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad glob: %v", err)
	}
	ex = []string{"vendor/**", " vendor/** ", "", "*.lock"}
	st, err = f.svc.UpdateSettings(f.ctx, f.p, f.tree, SettingsPatch{Exclude: &ex})
	must(t, err)
	if len(st.Exclude) != 2 || st.UpdatedBy != "alice" || st.UpdatedAt == nil {
		t.Fatalf("settings = %+v", st)
	}
}

func TestResolveAndList(t *testing.T) {
	f := newFixture(t)
	s1 := f.snap("", entry{path: "a.env", content: "K=" + awsKey + "\n"}, entry{path: "b.rs", content: "t=" + ghToken + "\n"})
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)))
	all := f.findings()
	if len(all) != 2 {
		t.Fatalf("findings = %+v", all)
	}
	if _, err := f.svc.Resolve(f.ctx, f.p, f.tree, all[0].ID, ResolveRequest{Resolution: "lost"}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad resolution: %v", err)
	}
	if _, err := f.svc.Resolve(f.ctx, f.p, f.tree, "nope", ResolveRequest{Resolution: "revoked"}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown finding: %v", err)
	}
	r, err := f.svc.Resolve(f.ctx, f.p, f.tree, all[0].ID, ResolveRequest{Resolution: "revoked", Note: "rotated"})
	must(t, err)
	if r.Status != StatusResolved || r.ResolvedBy != "alice" || r.ResolvedAt == nil || r.Note != "rotated" {
		t.Fatalf("resolved = %+v", r)
	}
	open, err := f.svc.ListFindings(f.ctx, f.tree.ID, StatusOpen, 0)
	must(t, err)
	if len(open.Findings) != 1 || open.Open != 1 || open.Resolved != 1 {
		t.Fatalf("open list = %+v", open)
	}
	lim, err := f.svc.ListFindings(f.ctx, f.tree.ID, "", 1)
	must(t, err)
	if len(lim.Findings) != 1 || lim.Open+lim.Resolved != 2 {
		t.Fatalf("limited list = %+v", lim)
	}
	// A warned push of the same secret reopens it.
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)))
	if g, _ := f.svc.GetFinding(f.ctx, f.tree.ID, all[0].ID); g.Status != StatusOpen || g.Resolution != "" {
		t.Fatalf("not reopened: %+v", g)
	}
	r, err = f.svc.Resolve(f.ctx, f.p, f.tree, all[0].ID, ResolveRequest{Resolution: "test_data"})
	must(t, err)
	r, err = f.svc.Resolve(f.ctx, f.p, f.tree, all[0].ID, ResolveRequest{Reopen: true})
	must(t, err)
	if r.Status != StatusOpen {
		t.Fatalf("reopen = %+v", r)
	}
}

func TestDeleteTreeAndStart(t *testing.T) {
	f := newFixture(t)
	ctx, cancel := context.WithCancel(f.ctx)
	defer cancel()
	f.svc.Start(ctx)
	s1 := f.snap("", entry{path: "a.env", content: "K=" + awsKey + "\n"})
	must(t, f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1)))
	_, err := f.svc.Allow(f.ctx, f.p, f.tree, AllowRequest{Fingerprint: f.findings()[0].Fingerprint, Reason: "x"})
	must(t, err)
	f.setMode(ModeBlock)
	f.bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: "ten-1", TreeID: f.tree.ID})
	deadline := time.Now().Add(3 * time.Second)
	for {
		raws, _ := f.store.ListDocs(f.ctx, CollFindings, "")
		var al AllowList
		gone := len(raws) == 0 && errors.Is(f.store.GetDoc(f.ctx, CollAllow, f.tree.ID, &al), domain.ErrNotFound) &&
			errors.Is(f.store.GetDoc(f.ctx, CollSettings, f.tree.ID, &al), domain.ErrNotFound)
		if gone {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("documents of the deleted tree remain")
		}
		time.Sleep(20 * time.Millisecond)
	}
}

func TestScanSnapshotAndRefs(t *testing.T) {
	f := newFixture(t)
	s1 := f.snap("", entry{path: "a.env", content: "K=" + awsKey + "\n"}, entry{path: "ok.txt", content: "fine\n"})
	f.store_(s1)
	must(t, f.store.PutTag(f.ctx, &domain.Tag{TreeID: f.tree.ID, Name: "v1", Target: s1.ID}))
	for _, ref := range []string{"", "main", "v1", s1.ID} {
		res, err := f.svc.ScanSnapshot(f.ctx, f.tree, ref)
		must(t, err)
		if res.SnapshotID != s1.ID || res.Files != 2 || len(res.Matches) != 1 || res.Matches[0].Allowed {
			t.Fatalf("ref %q: %+v", ref, res)
		}
	}
	if _, err := f.svc.ScanSnapshot(f.ctx, f.tree, "nope"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown ref: %v", err)
	}
	res, _ := f.svc.ScanSnapshot(f.ctx, f.tree, "")
	_, err := f.svc.Allow(f.ctx, f.p, f.tree, AllowRequest{Fingerprint: res.Matches[0].Fingerprint, Reason: "ok"})
	must(t, err)
	res, _ = f.svc.ScanSnapshot(f.ctx, f.tree, "")
	if !res.Matches[0].Allowed {
		t.Fatalf("allowlisted match not flagged: %+v", res.Matches)
	}
	// Nothing is stored by on-demand scans.
	if got := f.findings(); len(got) != 0 {
		t.Fatalf("findings = %+v", got)
	}
}

func TestSummarize(t *testing.T) {
	f := newFixture(t)
	other := &domain.Tree{ID: "tree-2", TenantID: "ten-1", Name: "api", DefaultBranch: "main"}
	must(t, f.store.CreateTree(f.ctx, other))
	f.setMode(ModeBlock)
	s1 := f.snap("", entry{path: "a.env", content: "K=" + awsKey + "\n"})
	_ = f.svc.CheckPushPlan(f.ctx, f.p, f.tree, plan("", s1))
	must(t, f.store.PutLicenseRule(f.ctx, &domain.LicenseRule{TreeID: other.ID, PathGlob: "**", SPDX: "GPL-2.0"}))
	sum, err := f.svc.Summarize(f.ctx)
	must(t, err)
	if sum.Trees != 2 || sum.Modes.Block != 1 || sum.Modes.Warn != 1 || sum.OpenFindings != 1 || sum.InvalidRules != 1 ||
		sum.LicenseRules != 1 || len(sum.Recent) != 1 || sum.Recent[0].Tenant != "acme" || sum.ScanRules < 15 {
		t.Fatalf("summary = %+v", sum)
	}
	if sum.TreeList[0].Tree != "web" || sum.TreeList[0].BlockedFindings != 1 || sum.TreeList[0].LastFindingAt == nil {
		t.Fatalf("tree list = %+v", sum.TreeList)
	}
}
