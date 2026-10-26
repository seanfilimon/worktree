package mirror

import (
	"context"
	"crypto/sha1"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

var t0 = time.Unix(1_700_000_000, 0).UTC()

// oddPath needs quoting in a fast-import stream (and is valid on Windows).
const oddPath = "we ird/ü #1's.txt"

func requireGit(t *testing.T) {
	t.Helper()
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git is not installed")
	}
}

// world is a tree with a mirror service over an in-memory store.
type world struct {
	t     *testing.T
	ctx   context.Context
	store *meta.Memory
	objs  objstore.Store
	bus   *events.Bus
	svc   *Service
	tree  *domain.Tree
	alice *domain.Principal
	cfg   string // empty global git config for verification commands
	dir   string
}

func newWorld(t *testing.T, mutate ...func(*Options)) *world {
	t.Helper()
	ctx := context.Background()
	w := &world{t: t, ctx: ctx, store: meta.NewMemory(), dir: t.TempDir(), bus: events.NewBus()}
	w.objs = objstore.New(filepath.Join(w.dir, "objects"))
	ten := &domain.Tenant{ID: "tenant-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}
	must(t, w.store.CreateTenant(ctx, ten))
	acct := &domain.Account{ID: "acct-alice", Username: "alice", TenantID: "tenant-alice", CreatedAt: t0}
	must(t, w.store.CreateAccount(ctx, acct))
	w.alice = &domain.Principal{AccountID: acct.ID, Username: "alice", TenantID: "tenant-alice"}
	w.tree = &domain.Tree{ID: "tree-web", TenantID: ten.ID, Name: "web", DefaultBranch: "main", CreatedAt: t0}
	must(t, w.store.CreateTree(ctx, w.tree))
	w.svc = New(Deps{Store: w.store, Objects: w.objs, Bus: w.bus, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	var clockMu sync.Mutex
	clock := time.Now().UTC()
	tick := func() time.Time { // strictly increasing, so creation order is stable
		clockMu.Lock()
		defer clockMu.Unlock()
		clock = clock.Add(time.Millisecond)
		return clock
	}
	o := Options{DataDir: filepath.Join(w.dir, "data"), AllowLocal: true, Debounce: time.Hour, Backoff: []time.Duration{}, Now: tick}
	for _, m := range mutate {
		m(&o)
	}
	w.svc.Configure(o)
	w.cfg = filepath.Join(w.dir, "gitconfig")
	must(t, os.WriteFile(w.cfg, nil, 0o600))
	return w
}

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func (w *world) put(data []byte) hash.Hash {
	w.t.Helper()
	h := hash.Sum(data)
	must(w.t, w.objs.Put(w.ctx, h, data))
	return h
}

// spec is a file of a test manifest.
type spec struct {
	data    string
	exec    bool
	chunked bool
}

func (w *world) manifest(files map[string]spec) hash.Hash {
	w.t.Helper()
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	slices.Sort(paths)
	var m syncpb.ManifestRecord
	for _, p := range paths {
		f := files[p]
		var h hash.Hash
		if f.chunked {
			var cl syncpb.ChunkList
			data := []byte(f.data)
			for len(data) > 0 {
				n := min(len(data), 7)
				ch := w.put(data[:n])
				cl.Chunks = append(cl.Chunks, &syncpb.Chunk{Hash: &syncpb.ContentHash{Value: ch[:]}, Size: uint64(n)})
				data = data[n:]
			}
			cl.Size = uint64(len(f.data))
			raw, err := proto.Marshal(&cl)
			must(w.t, err)
			h = w.put(raw)
		} else {
			h = w.put([]byte(f.data))
		}
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(f.data)), Executable: f.exec, Chunked: f.chunked})
	}
	raw, err := proto.Marshal(&m)
	must(w.t, err)
	return w.put(raw)
}

func (w *world) snapshot(id string, parents []string, msg string, at time.Time, files map[string]spec) *domain.Snapshot {
	w.t.Helper()
	s := &domain.Snapshot{ID: id, TreeID: w.tree.ID, BranchName: "main", Parents: parents, ManifestHash: w.manifest(files),
		Author: "Grace Hopper <grace@example.com>", Message: msg, CreatedAt: at, PushedAt: at}
	must(w.t, w.store.PutSnapshot(w.ctx, s))
	return s
}

func (w *world) branch(name, tip string) {
	w.t.Helper()
	must(w.t, w.store.PutBranch(w.ctx, &domain.Branch{TreeID: w.tree.ID, Name: name, Tip: tip, CreatedAt: t0, UpdatedAt: t0}))
}

// git runs a verification git command.
func (w *world) git(dir string, args ...string) string {
	w.t.Helper()
	out, err := w.gitErr(dir, args...)
	if err != nil {
		w.t.Fatalf("git %s: %v\n%s", strings.Join(args, " "), err, out)
	}
	return strings.TrimSpace(out)
}

func (w *world) gitErr(dir string, args ...string) (string, error) {
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	cmd.Env = append(os.Environ(), "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+w.cfg, "GIT_TERMINAL_PROMPT=0",
		"GIT_AUTHOR_NAME=Ext", "GIT_AUTHOR_EMAIL=ext@example.com", "GIT_COMMITTER_NAME=Ext", "GIT_COMMITTER_EMAIL=ext@example.com")
	out, err := cmd.CombinedOutput()
	return string(out), err
}

// bareRemote creates an empty bare repository and returns its path and URL.
func (w *world) bareRemote(name string) (string, string) {
	w.t.Helper()
	p := filepath.Join(w.dir, name)
	w.git(w.dir, "init", "--bare", "--quiet", "--initial-branch=main", p)
	return p, fileURL(p)
}

func fileURL(p string) string {
	p = filepath.ToSlash(p)
	if runtime.GOOS == "windows" {
		return "file:///" + p
	}
	return "file://" + p
}

func (w *world) create(url string, in CreateInput) *domain.Mirror {
	w.t.Helper()
	in.URL = url
	m, err := w.svc.Create(w.ctx, w.alice, w.tree, in)
	must(w.t, err)
	return m
}

func (w *world) sync(id string) (*domain.Mirror, *SyncResult) {
	w.t.Helper()
	m, res, err := w.svc.Sync(w.ctx, w.tree, id, "alice")
	must(w.t, err)
	return m, res
}

// commitID is the SHA-1 of a canonical commit object, computed like
// libgit2's commit_create_buffer in `wt git export`.
func commitID(tree string, parents []string, sig, message string) string {
	var b strings.Builder
	b.WriteString("tree " + tree + "\n")
	for _, p := range parents {
		b.WriteString("parent " + p + "\n")
	}
	b.WriteString("author " + sig + "\ncommitter " + sig + "\n\n" + message)
	body := b.String()
	sum := sha1.Sum([]byte(fmt.Sprintf("commit %d\x00%s", len(body), body)))
	return hex.EncodeToString(sum[:])
}

// ---- unit tests ----------------------------------------------------------

func TestParseAuthorMatchesRustExporter(t *testing.T) {
	cases := []struct{ in, name, email string }{
		{"Jane Doe <jane@example.com>", "Jane Doe", "jane@example.com"},
		{"  Jane <j@x>  ", "Jane", "j@x"},
		{"jane@example.com", "jane", "jane@example.com"},
		{"jane", "jane", ""},
		{"<only@mail>", "", "only@mail"},
		{"Ev<il> <e@x>", "Evil", "e@x"},
		{"", "", ""},
	}
	for _, c := range cases {
		if got := ParseAuthor(c.in); got.Name != c.name || got.Email != c.email {
			t.Errorf("ParseAuthor(%q) = %+v, want %q %q", c.in, got, c.name, c.email)
		}
	}
	if got := signature("", t0); got != "unknown <unknown@localhost> 1700000000 +0000" {
		t.Errorf("signature = %q", got)
	}
	if got := MessageToGit("second\n\nbody  \n"); got != "second\n\nbody\n" {
		t.Errorf("MessageToGit = %q", got)
	}
	if MessageToGit(" \n") != "" {
		t.Error("blank message")
	}
}

func TestRefNamesPatternsAndQuoting(t *testing.T) {
	for _, ok := range []string{"refs/heads/main", "refs/heads/feature/x-1", "refs/tags/v1.0"} {
		if !ValidRefName(ok) {
			t.Errorf("%s should be valid", ok)
		}
	}
	for _, bad := range []string{"refs/heads/a..b", "refs/heads/x.lock", "refs/heads/.x", "refs/heads/a b", "refs/heads/a~", "refs/heads/", "refs/heads/a//b", "refs/heads/a@{1}", "refs/heads/a."} {
		if ValidRefName(bad) {
			t.Errorf("%s should be invalid", bad)
		}
	}
	pats, err := NormalizePatterns([]string{" main ", "release/*", "feature/**", "main", ""})
	if err != nil || len(pats) != 3 {
		t.Fatalf("patterns = %v, %v", pats, err)
	}
	for name, want := range map[string]bool{"main": true, "release/1.0": true, "release/1/2": false, "feature": true, "feature/a/b": true, "dev": false, "main/x": false} {
		if MatchBranch(pats, name) != want {
			t.Errorf("MatchBranch(%q) = %v", name, !want)
		}
	}
	if !MatchBranch(nil, "anything/at/all") {
		t.Error("empty patterns match everything")
	}
	for _, bad := range []string{"a b", "x//y", "[", strings.Repeat("a", MaxPattern+1)} {
		if _, err := NormalizePatterns([]string{bad}); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("pattern %q: %v", bad, err)
		}
	}
	if got := quotePath("dir/a \"b\"\\\n\x01é"); got != `"dir/a \"b\"\\\n\001é"` {
		t.Errorf("quotePath = %s", got)
	}
	if validPath("a/.git/x") || validPath("a//b") || validPath("../x") || !validPath("a/b.txt") {
		t.Error("validPath")
	}
	if got := parseMarks(":1 aaa\n:2 bbb\nnoise\n"); got[1] != "aaa" || got[2] != "bbb" || len(got) != 2 {
		t.Errorf("parseMarks = %v", got)
	}
	st := parsePorcelain("To file:///x\n*\trefs/heads/main:refs/heads/main\t[new branch]\n!\trefs/heads/dev:refs/heads/dev\t[rejected] (stale info)\nDone\n")
	if st["refs/heads/main"].flag != '*' || st["refs/heads/dev"].flag != '!' || !strings.Contains(st["refs/heads/dev"].reason, "stale") {
		t.Errorf("porcelain = %+v", st)
	}
}

func TestCheckURLGuard(t *testing.T) {
	resolve := func(_ context.Context, host string) ([]netip.Addr, error) {
		switch host {
		case "git.example.com":
			return []netip.Addr{netip.MustParseAddr("93.184.216.34")}, nil
		case "intranet.example.com":
			return []netip.Addr{netip.MustParseAddr("10.1.2.3")}, nil
		}
		return nil, errors.New("no such host")
	}
	w := newWorld(t, func(o *Options) { o.AllowLocal = false; o.Resolver = resolve })
	ok := []string{
		"https://git.example.com/org/repo.git",
		"ssh://git@git.example.com/org/repo.git",
		"git@git.example.com:org/repo.git",
		"https://93.184.216.34/repo.git",
	}
	for _, u := range ok {
		if _, err := w.svc.CheckURL(w.ctx, u); err != nil {
			t.Errorf("%s: %v", u, err)
		}
	}
	bad := map[string]string{
		"":                                    "required",
		"https://intranet.example.com/r.git":  "non-public",
		"https://127.0.0.1/r.git":             "non-public",
		"https://[::1]/r.git":                 "non-public",
		"http://169.254.169.254/latest":       "non-public",
		"git@192.168.1.10:org/r.git":          "non-public",
		"https://nowhere.invalid/r.git":       "resolve",
		"file:///srv/git/r.git":               "--mirror-allow-local",
		"https://user:secret@git.example.com": "password",
		"ftp://git.example.com/r.git":         "unsupported",
		"ext::sh -c touch% /tmp/pwned":        "spaces",
		"-oProxyCommand=evil":                 "'-'",
		"ssh://-oProxyCommand=x/r.git":        "'-'",
		"C:\\repos\\x.git":                    "Git remote",
	}
	for u, want := range bad {
		_, err := w.svc.CheckURL(w.ctx, u)
		if !errors.Is(err, domain.ErrInvalid) || !strings.Contains(err.Error(), want) {
			t.Errorf("CheckURL(%q) = %v, want an invalid-argument error mentioning %q", u, err, want)
		}
	}
	w.svc.Configure(Options{AllowLocal: true, Resolver: resolve})
	for _, u := range []string{"file:///srv/git/r.git", "file:///C:/Users/Some One/r.git", "https://127.0.0.1/r.git", "https://intranet.example.com/r.git"} {
		if _, err := w.svc.CheckURL(w.ctx, u); err != nil {
			t.Errorf("AllowLocal %s: %v", u, err)
		}
	}
}

func TestCRUD(t *testing.T) {
	w := newWorld(t)
	m := w.create("https://git.example.com/a.git", CreateInput{Branches: []string{"main", "release/*"}, Credential: "tok-123456"})
	if !m.Active || !m.Tags || !m.HasCredential || m.Credential != "" || m.CreatedBy != "alice" || len(m.Branches) != 2 {
		t.Fatalf("created = %+v", m)
	}
	if !w.svc.Scheduled(m.ID) {
		t.Error("an active new mirror is scheduled for its first sync")
	}
	if _, err := w.svc.Create(w.ctx, w.alice, w.tree, CreateInput{URL: "https://git.example.com/a.git"}); !errors.Is(err, domain.ErrConflict) {
		t.Errorf("duplicate url: %v", err)
	}
	if _, err := w.svc.Create(w.ctx, nil, w.tree, CreateInput{URL: "https://git.example.com/b.git"}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Errorf("anonymous: %v", err)
	}
	if _, err := w.svc.Create(w.ctx, w.alice, w.tree, CreateInput{URL: "ssh://git@git.example.com/b.git", Credential: "x"}); !errors.Is(err, domain.ErrInvalid) {
		t.Errorf("ssh credential: %v", err)
	}
	if _, err := w.svc.Create(w.ctx, w.alice, w.tree, CreateInput{URL: "https://git.example.com/b.git", Credential: "a\nb"}); !errors.Is(err, domain.ErrInvalid) {
		t.Errorf("multi-line credential: %v", err)
	}
	f := false
	other := w.create("https://git.example.com/c.git", CreateInput{Tags: &f, Active: &f})
	if other.Tags || other.Active || w.svc.Scheduled(other.ID) {
		t.Fatalf("other = %+v", other)
	}
	list, err := w.svc.List(w.ctx, w.tree)
	if err != nil || len(list) != 2 || list[0].ID != m.ID {
		t.Fatalf("list = %+v, %v", list, err)
	}
	raw, err := w.store.ListDocs(w.ctx, CollMirrors, "")
	must(t, err)
	if !strings.Contains(string(raw[0])+string(raw[1]), "tok-123456") {
		t.Error("the credential is stored")
	}

	empty := ""
	branches := []string{}
	tr := true
	got, changed, err := w.svc.Update(w.ctx, w.tree, m.ID, Patch{Credential: &empty, Branches: &branches, Active: &tr})
	if err != nil || got.HasCredential || len(got.Branches) != 0 || strings.Join(changed, ",") != "branches,credential,active" {
		t.Fatalf("update = %+v %v %v", got, changed, err)
	}
	dup := "https://git.example.com/c.git"
	if _, _, err := w.svc.Update(w.ctx, w.tree, m.ID, Patch{URL: &dup}); !errors.Is(err, domain.ErrConflict) {
		t.Errorf("update to a used url: %v", err)
	}
	if _, _, err := w.svc.Update(w.ctx, w.tree, "nope", Patch{Active: &tr}); !errors.Is(err, domain.ErrNotFound) {
		t.Errorf("update missing: %v", err)
	}
	if _, err := w.svc.Get(w.ctx, &domain.Tree{ID: "other", Name: "other"}, m.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Errorf("get from another tree: %v", err)
	}
	must(t, w.svc.Delete(w.ctx, w.tree, m.ID))
	if w.svc.Scheduled(m.ID) {
		t.Error("deleting unschedules")
	}
	if err := w.svc.Delete(w.ctx, w.tree, m.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Errorf("second delete: %v", err)
	}
	for i := 0; i < MaxMirrorsPerTree-1; i++ {
		w.create(fmt.Sprintf("https://git.example.com/n%d.git", i), CreateInput{Active: &f})
	}
	if _, err := w.svc.Create(w.ctx, w.alice, w.tree, CreateInput{URL: "https://git.example.com/full.git"}); !errors.Is(err, domain.ErrConflict) {
		t.Errorf("over the limit: %v", err)
	}
}

func TestGitMissingDisablesMirrors(t *testing.T) {
	w := newWorld(t, func(o *Options) { o.GitBinary = "wt-definitely-not-git-binary" })
	st := w.svc.Git()
	if st.Available || !strings.Contains(st.Error, "not found") {
		t.Fatalf("git status = %+v", st)
	}
	m := w.create("file:///nowhere.git", CreateInput{})
	_, _, err := w.svc.Sync(w.ctx, w.tree, m.ID, "alice")
	if !errors.Is(err, domain.ErrConflict) || !strings.Contains(err.Error(), "disabled") {
		t.Fatalf("sync: %v", err)
	}
	got, _ := w.svc.Get(w.ctx, w.tree, m.ID)
	if got.LastStatus != StatusDisabled || !strings.Contains(got.LastError, "--git-binary") {
		t.Fatalf("mirror = %+v", got)
	}
}

// ---- export and push -------------------------------------------------------

func TestSyncExportsHistoryAndPushesIncrementally(t *testing.T) {
	requireGit(t)
	w := newWorld(t)
	events, cancel := w.bus.Subscribe(domain.EventFilter{})
	defer cancel()
	ctx := w.ctx
	must(t, w.store.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: w.tree.ID, PathGlob: "secret/**", SPDX: "LicenseRef-Proprietary"}))
	must(t, w.store.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: w.tree.ID, PathGlob: "src/**", SPDX: "MIT"}))
	must(t, w.store.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: w.tree.ID, PathGlob: "**", SPDX: "LicenseRef-Proprietary", Grantee: "partner"}))

	big := strings.Repeat("chunked content ", 5)
	s1 := w.snapshot("s1", nil, "first", t0, map[string]spec{"README.md": {data: "# web\n"}})
	s2 := w.snapshot("s2", []string{"s1"}, "second\n\nbody  \n", t0.Add(time.Hour), map[string]spec{
		"README.md":      {data: "# web v2\n"},
		"run.sh":         {data: "#!/bin/sh\necho hi\n", exec: true},
		"src/main.go":    {data: "package main\n"},
		"secret/key.txt": {data: "hunter2"},
		"big.bin":        {data: big, chunked: true},
		oddPath:          {data: "odd\n"},
	})
	sf := w.snapshot("sf", []string{"s1"}, "feature", t0.Add(90*time.Minute), map[string]spec{
		"README.md": {data: "# web\n"}, "feature.txt": {data: "f\n"},
	})
	sm := w.snapshot("sm", []string{"s2", "sf"}, "Merge feature", t0.Add(2*time.Hour), map[string]spec{
		"README.md": {data: "# web v2\n"}, "run.sh": {data: "#!/bin/sh\necho hi\n", exec: true}, "src/main.go": {data: "package main\n"},
		"secret/key.txt": {data: "hunter2"}, "big.bin": {data: big, chunked: true}, "feature.txt": {data: "f\n"},
		oddPath: {data: "odd\n"},
	})
	w.branch("main", sm.ID)
	w.branch("feature", sf.ID)
	w.branch("wip/skip-me", s1.ID)
	must(t, w.store.PutTag(ctx, &domain.Tag{TreeID: w.tree.ID, Name: "v1", Target: s1.ID, Tagger: "Ada <ada@example.com>", Message: "Version 1", CreatedAt: t0.Add(time.Minute)}))
	must(t, w.store.PutTag(ctx, &domain.Tag{TreeID: w.tree.ID, Name: "light", Target: s2.ID, CreatedAt: t0}))

	remote, url := w.bareRemote("remote.git")
	m := w.create(url, CreateInput{Branches: []string{"main", "feature"}})
	got, res := w.sync(m.ID)
	if got.LastStatus != StatusOK || got.LastError != "" || got.LastSyncAt == nil {
		t.Fatalf("mirror = %+v, result %+v", got, res)
	}
	if res.Commits != 4 || !res.Pushed || len(res.Refs) != 4 {
		t.Fatalf("result = %+v", res)
	}
	for _, r := range res.Refs {
		if r.Status != "created" {
			t.Errorf("ref %+v", r)
		}
	}
	if !slices.Equal(res.Excluded, []string{"secret/key.txt"}) {
		t.Errorf("excluded = %v", res.Excluded)
	}

	refs := w.git(remote, "for-each-ref", "--format=%(refname) %(objecttype)")
	if refs != "refs/heads/feature commit\nrefs/heads/main commit\nrefs/tags/light commit\nrefs/tags/v1 tag" {
		t.Fatalf("remote refs:\n%s", refs)
	}

	// Commit ids equal the canonical objects `wt git export` writes.
	sig := func(at time.Time) string { return fmt.Sprintf("Grace Hopper <grace@example.com> %d +0000", at.Unix()) }
	treeOf := func(rev string) string { return w.git(remote, "rev-parse", rev+"^{tree}") }
	c1 := commitID(treeOf("v1^{commit}"), nil, sig(t0), "first\n")
	if got := w.git(remote, "rev-parse", "v1^{commit}"); got != c1 {
		t.Errorf("root commit %s, want %s", got, c1)
	}
	c2 := commitID(treeOf("light"), []string{c1}, sig(t0.Add(time.Hour)), "second\n\nbody\n")
	if got := w.git(remote, "rev-parse", "light"); got != c2 {
		t.Errorf("second commit %s, want %s", got, c2)
	}
	cf := w.git(remote, "rev-parse", "feature")
	cm := commitID(treeOf("main"), []string{c2, cf}, sig(t0.Add(2*time.Hour)), "Merge feature\n")
	if got := w.git(remote, "rev-parse", "main"); got != cm {
		t.Errorf("merge commit %s, want %s", got, cm)
	}
	if parents := w.git(remote, "log", "-1", "--format=%P", "main"); parents != c2+" "+cf {
		t.Errorf("merge parents %q", parents)
	}
	if n := w.git(remote, "rev-list", "--count", "main"); n != "4" {
		t.Errorf("main has %s commits", n)
	}

	// Tree contents: chunked file reassembled, executable bit, exclusion,
	// NOTICE, odd path.
	if got := w.git(remote, "cat-file", "-p", "main:big.bin"); got != strings.TrimSpace(big) {
		t.Errorf("big.bin = %q", got)
	}
	if got := w.git(remote, "cat-file", "-p", "main:"+oddPath); got != "odd" {
		t.Errorf("odd path = %q", got)
	}
	ls := w.git(remote, "ls-tree", "-r", "--name-only", "main")
	if strings.Contains(ls, "secret/") || !strings.Contains(ls, "LICENSES/NOTICE") || !strings.Contains(ls, "feature.txt") {
		t.Errorf("main tree:\n%s", ls)
	}
	if mode := strings.Fields(w.git(remote, "ls-tree", "main", "run.sh"))[0]; mode != "100755" {
		t.Errorf("run.sh mode %s", mode)
	}
	if mode := strings.Fields(w.git(remote, "ls-tree", "main", "README.md"))[0]; mode != "100644" {
		t.Errorf("README.md mode %s", mode)
	}
	notice := w.git(remote, "cat-file", "-p", "main:LICENSES/NOTICE")
	for _, want := range []string{"W0rkTree license notice", "MIT\n    src/main.go", "1 path was excluded"} {
		if !strings.Contains(notice, want) {
			t.Errorf("NOTICE lacks %q:\n%s", want, notice)
		}
	}
	if strings.Contains(notice, "secret") {
		t.Errorf("NOTICE names an excluded path:\n%s", notice)
	}
	if _, err := w.gitErr(remote, "cat-file", "-e", "v1^{commit}:LICENSES/NOTICE"); err == nil {
		t.Error("a snapshot without covered or excluded paths gets no NOTICE")
	}
	tag := w.git(remote, "cat-file", "-p", "v1")
	if !strings.Contains(tag, "tagger Ada <ada@example.com> 1700000060 +0000") || !strings.HasSuffix(tag, "Version 1") {
		t.Errorf("annotated tag:\n%s", tag)
	}

	// A clone sees the files.
	clone := filepath.Join(w.dir, "clone")
	w.git(w.dir, "clone", "--quiet", url, clone)
	if b, err := os.ReadFile(filepath.Join(clone, "README.md")); err != nil || string(b) != "# web v2\n" {
		t.Errorf("clone README = %q, %v", b, err)
	}

	// mirror.synced was published; the map is persisted.
	select {
	case e := <-drain(events, domain.EventMirrorSynced):
		if e.TreeID != w.tree.ID || e.Actor != "alice" {
			t.Errorf("event = %+v", e)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("no mirror.synced event")
	}
	state, err := meta.GetDocT[mapDoc](ctx, w.store, CollMaps, MapKey(m.ID))
	must(t, err)
	if len(state.Commits) != 4 || state.Commits["sm"] != cm || state.Remote["refs/heads/main"] != cm {
		t.Fatalf("map = %+v", state)
	}

	// A second push syncs incrementally: one new commit.
	s3 := w.snapshot("s3", []string{"sm"}, "third", t0.Add(3*time.Hour), map[string]spec{
		"README.md": {data: "# web v3\n"}, "run.sh": {data: "#!/bin/sh\necho hi\n"}, "src/main.go": {data: "package main\n"},
		"secret/key.txt": {data: "hunter3"}, "feature.txt": {data: "f\n"},
	})
	w.branch("main", s3.ID)
	_, res = w.sync(m.ID)
	if res.Commits != 1 || !res.Pushed {
		t.Fatalf("incremental result = %+v", res)
	}
	statuses := map[string]string{}
	for _, r := range res.Refs {
		statuses[r.Ref] = r.Status
	}
	if statuses["refs/heads/main"] != "updated" || statuses["refs/heads/feature"] != "up_to_date" {
		t.Errorf("statuses = %v", statuses)
	}
	if parent := w.git(remote, "rev-parse", "main^"); parent != cm {
		t.Errorf("main^ = %s, want %s", parent, cm)
	}
	ls = w.git(remote, "ls-tree", "-r", "main")
	if strings.Contains(ls, "big.bin") || strings.Contains(ls, "secret") || !strings.Contains(ls, "100644 blob") || strings.Contains(ls, "100755") {
		t.Errorf("third tree:\n%s", ls)
	}
	if got := w.git(remote, "cat-file", "-p", "main:README.md"); got != "# web v3" {
		t.Errorf("README = %q", got)
	}

	// Nothing new: no commits, refs up to date.
	_, res = w.sync(m.ID)
	if res.Commits != 0 || !res.Pushed {
		t.Fatalf("idempotent result = %+v", res)
	}

	// Deleting a mirrored branch deletes it on the remote; branches that
	// stop matching are left alone.
	must(t, w.store.DeleteBranch(ctx, w.tree.ID, "feature"))
	_, res = w.sync(m.ID)
	if res.Refs[0].Ref != "refs/heads/feature" || res.Refs[0].Status != "deleted" {
		t.Fatalf("delete result = %+v", res)
	}
	if _, err := w.gitErr(remote, "rev-parse", "--verify", "refs/heads/feature"); err == nil {
		t.Error("feature still exists on the remote")
	}

	// --force-with-lease: someone else moved the remote branch.
	work := filepath.Join(w.dir, "work")
	w.git(w.dir, "clone", "--quiet", url, work)
	must(t, os.WriteFile(filepath.Join(work, "ext.txt"), []byte("x"), 0o600))
	w.git(work, "add", "ext.txt")
	w.git(work, "commit", "--quiet", "-m", "external")
	w.git(work, "push", "--quiet", "origin", "main")
	external := w.git(remote, "rev-parse", "main")
	s4 := w.snapshot("s4", []string{"s3"}, "fourth", t0.Add(4*time.Hour), map[string]spec{"README.md": {data: "4\n"}})
	w.branch("main", s4.ID)
	got, res = w.sync(m.ID)
	if got.LastStatus != StatusError || got.LastError == "" || res.Pushed || res.Error == "" {
		t.Fatalf("lease failure: mirror %+v result %+v", got, res)
	}
	if now := w.git(remote, "rev-parse", "main"); now != external {
		t.Error("a stale lease must not overwrite the remote")
	}
	select {
	case e := <-drain(events, domain.EventMirrorFailed):
		if !strings.Contains(fmt.Sprint(e.Payload), "error") {
			t.Errorf("failed event = %+v", e)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("no mirror.failed event")
	}

	// Determinism: a second mirror of the same tree writes the same ids.
	remote2, url2 := w.bareRemote("remote2.git")
	m2 := w.create(url2, CreateInput{})
	_, res = w.sync(m2.ID)
	if res.Error != "" {
		t.Fatalf("second mirror: %+v", res)
	}
	if a, b := w.git(remote2, "rev-parse", "v1^{commit}"), c1; a != b {
		t.Errorf("v1 differs across mirrors: %s vs %s", a, b)
	}
	if w.git(remote2, "rev-parse", "main~1") != w.git(w.svc.repoDir(m.ID), "rev-parse", "main~1") {
		t.Error("main~1 differs across mirrors")
	}
	if _, err := w.gitErr(remote2, "rev-parse", "--verify", "refs/heads/wip/skip-me"); err != nil {
		t.Error("an unfiltered mirror carries every branch")
	}
}

// drain returns a channel yielding the next event of type typ from ch.
func drain(ch <-chan domain.Event, typ string) <-chan domain.Event {
	out := make(chan domain.Event, 1)
	go func() {
		for e := range ch {
			if e.Type == typ {
				out <- e
				return
			}
		}
	}()
	return out
}

func TestLicenseChangeRewritesHistory(t *testing.T) {
	requireGit(t)
	w := newWorld(t)
	s1 := w.snapshot("s1", nil, "first", t0, map[string]spec{"a.txt": {data: "a"}, "private/b.txt": {data: "b"}})
	w.branch("main", s1.ID)
	remote, url := w.bareRemote("remote.git")
	m := w.create(url, CreateInput{})
	_, res := w.sync(m.ID)
	before := w.git(remote, "rev-parse", "main")
	if res.Error != "" || !strings.Contains(w.git(remote, "ls-tree", "-r", "--name-only", "main"), "private/b.txt") {
		t.Fatalf("first sync %+v", res)
	}
	must(t, w.store.PutLicenseRule(w.ctx, &domain.LicenseRule{TreeID: w.tree.ID, PathGlob: "private/**", SPDX: "LicenseRef-Proprietary"}))
	_, res = w.sync(m.ID)
	if res.Error != "" || res.Commits != 1 || res.Refs[0].Status != "forced" {
		t.Fatalf("rewrite result = %+v", res)
	}
	after := w.git(remote, "rev-parse", "main")
	if after == before || strings.Contains(w.git(remote, "ls-tree", "-r", "--name-only", "main"), "private/") {
		t.Fatal("the restricted path must disappear from the mirrored history")
	}
}

func TestCredentialTravelsInHeaderOnly(t *testing.T) {
	requireGit(t)
	const token = "tok-s3cr3t-value"
	var mu sync.Mutex
	var auths []string
	srv := httptest.NewServer(http.HandlerFunc(func(rw http.ResponseWriter, r *http.Request) {
		mu.Lock()
		auths = append(auths, r.Header.Get("Authorization"))
		mu.Unlock()
		http.Error(rw, "nope", http.StatusForbidden)
	}))
	defer srv.Close()
	var cmdMu sync.Mutex
	var argv [][]string
	var envs [][]string
	w := newWorld(t, func(o *Options) {
		o.CommandHook = func(args, env []string) {
			cmdMu.Lock()
			defer cmdMu.Unlock()
			argv = append(argv, args)
			envs = append(envs, env)
		}
	})
	s1 := w.snapshot("s1", nil, "first", t0, map[string]spec{"a.txt": {data: "a"}})
	w.branch("main", s1.ID)
	m := w.create(srv.URL+"/org/repo.git", CreateInput{Credential: token})
	got, res := w.sync(m.ID)
	if got.LastStatus != StatusError || res.Pushed {
		t.Fatalf("push to a refusing server: %+v %+v", got, res)
	}
	want := "Basic " + base64.StdEncoding.EncodeToString([]byte("x-access-token:"+token))
	mu.Lock()
	sent := slices.Contains(auths, want)
	mu.Unlock()
	if !sent {
		t.Fatalf("the remote never saw the credential header; saw %q", auths)
	}
	cmdMu.Lock()
	defer cmdMu.Unlock()
	pushed := false
	for i, args := range argv {
		joined := strings.Join(args, " ")
		if strings.Contains(joined, token) || strings.Contains(joined, want) {
			t.Fatalf("credential on the command line: %s", joined)
		}
		if args[0] == "push" {
			pushed = true
			if !slices.Contains(envs[i], "GIT_TERMINAL_PROMPT=0") || !strings.Contains(strings.Join(envs[i], "\n"), "=http.extraHeader") {
				t.Errorf("push env lacks the header config")
			}
		} else if strings.Contains(strings.Join(envs[i], "\n"), want) {
			t.Errorf("credential leaked to the environment of git %s", args[0])
		}
	}
	if !pushed {
		t.Fatal("no push ran")
	}
	if strings.Contains(got.LastError, token) || strings.Contains(got.LastError, want) {
		t.Fatalf("LastError leaks the credential: %s", got.LastError)
	}
	if redact("x "+token+" "+want, token) != "x [redacted] Basic [redacted]" {
		t.Errorf("redact = %q", redact("x "+token+" "+want, token))
	}
	if authHeader("user:pass") != "Authorization: Basic "+base64.StdEncoding.EncodeToString([]byte("user:pass")) {
		t.Error("user:password credentials are used as given")
	}
}

type denyAll struct{}

func (denyAll) Authorize(context.Context, *domain.Principal, domain.Action, string, *domain.Tree) error {
	return domain.ErrForbidden
}
func (denyAll) RoleOf(context.Context, string, string) (string, error) { return "", nil }

func TestCreatorMustStillReadTheTree(t *testing.T) {
	requireGit(t)
	w := newWorld(t)
	w.svc.d.Authz = denyAll{}
	s1 := w.snapshot("s1", nil, "first", t0, map[string]spec{"a.txt": {data: "a"}})
	w.branch("main", s1.ID)
	_, url := w.bareRemote("remote.git")
	m := w.create(url, CreateInput{})
	got, res := w.sync(m.ID)
	if got.LastStatus != StatusError || !strings.Contains(res.Error, "may no longer read") {
		t.Fatalf("sync = %+v %+v", got, res)
	}
}

func TestEventsTriggerDebouncedSyncsAndRetries(t *testing.T) {
	requireGit(t)
	w := newWorld(t, func(o *Options) {
		o.Debounce = 30 * time.Millisecond
		o.Backoff = []time.Duration{40 * time.Millisecond}
	})
	ctx, cancel := context.WithCancel(w.ctx)
	defer func() { cancel(); w.svc.Wait() }()
	w.svc.Start(ctx)
	s1 := w.snapshot("s1", nil, "first", t0, map[string]spec{"a.txt": {data: "a"}})
	w.branch("main", s1.ID)
	remote, url := w.bareRemote("remote.git")
	m := w.create(url, CreateInput{})
	waitFor(t, "the first automatic sync", func() bool {
		got, _ := w.svc.Get(w.ctx, w.tree, m.ID)
		return got.LastStatus == StatusOK
	})
	s2 := w.snapshot("s2", []string{"s1"}, "second", t0.Add(time.Hour), map[string]spec{"a.txt": {data: "b"}})
	w.branch("main", s2.ID)
	for i := 0; i < 5; i++ { // a burst of events collapses into one sync
		w.bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: w.tree.TenantID, TreeID: w.tree.ID})
	}
	waitFor(t, "the pushed snapshot to reach the remote", func() bool {
		out, err := w.gitErr(remote, "log", "-1", "--format=%s", "main")
		return err == nil && strings.TrimSpace(out) == "second"
	})

	// A failing mirror is retried once (Backoff has one entry).
	bad := w.create(fileURL(filepath.Join(w.dir, "missing.git")), CreateInput{})
	waitFor(t, "the failing sync and its retry", func() bool {
		got, _ := w.svc.Get(w.ctx, w.tree, bad.ID)
		w.svc.mu.Lock()
		n := w.svc.failures[bad.ID]
		w.svc.mu.Unlock()
		return got.LastStatus == StatusError && n == 2 && !w.svc.Scheduled(bad.ID)
	})

	// tree.deleted removes the mirrors and their repositories.
	dir := w.svc.repoDir(m.ID)
	if _, err := os.Stat(dir); err != nil {
		t.Fatalf("local repository: %v", err)
	}
	w.svc.Wait()
	must(t, w.svc.HandleEvent(w.ctx, domain.Event{Type: domain.EventTreeDeleted, TenantID: w.tree.TenantID, TreeID: w.tree.ID}))
	if list, _ := w.svc.List(w.ctx, w.tree); len(list) != 0 {
		t.Fatalf("mirrors left: %+v", list)
	}
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Errorf("repository left behind: %v", err)
	}
	if docs, _ := w.store.ListDocs(w.ctx, CollMaps, ""); len(docs) != 0 {
		t.Errorf("maps left: %d", len(docs))
	}
}

func waitFor(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(30 * time.Second)
	for time.Now().Before(deadline) {
		if cond() {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func TestNoticeMatchesArchiveLayout(t *testing.T) {
	rules := []domain.LicenseRule{{PathGlob: "src/**", SPDX: "MIT"}}
	got := string(license.BuildNotice(rules, []string{"src/b.go", "src/a.go", "x"}, []string{"Tree: acme/web"}, "\nfooter\n"))
	want := "W0rkTree license notice\n=======================\n\nTree: acme/web\n\n" +
		"SPDX license identifiers of the files in this export, from the tree's\n" +
		"license rules. Files that are not listed are not covered by a rule.\n\nMIT\n    src/a.go\n    src/b.go\n\nfooter\n"
	if got != want {
		t.Errorf("notice:\n%s", got)
	}
	if license.BuildNotice(rules, []string{"x"}, nil, "") != nil {
		t.Error("nothing covered and no footer: no notice")
	}
}
