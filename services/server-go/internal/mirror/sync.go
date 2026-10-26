package mirror

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// SyncResult describes one sync of a mirror.
type SyncResult struct {
	// Commits is the number of commits written to the local repository.
	Commits int `json:"commits"`
	// Refs are the outcomes of the pushed ref updates.
	Refs []RefUpdate `json:"refs"`
	// Pushed reports whether `git push` ran and succeeded.
	Pushed bool `json:"pushed"`
	// Excluded lists (up to 100) paths left out because their license does
	// not permit redistribution.
	Excluded []string `json:"excluded,omitempty"`
	// Skipped lists (up to 100) branches, tags or paths that cannot be
	// represented in Git.
	Skipped    []string `json:"skipped,omitempty"`
	Error      string   `json:"error,omitempty"`
	DurationMS int64    `json:"duration_ms"`
}

// RefUpdate is the push outcome of one ref.
type RefUpdate struct {
	Ref string `json:"ref"`
	// New is the pushed object id ("" when the ref was deleted).
	New string `json:"new,omitempty"`
	// Status: "created" | "updated" | "forced" | "deleted" | "up_to_date" |
	// "rejected" | "error".
	Status string `json:"status"`
	Reason string `json:"reason,omitempty"`
}

const maxListed = 100

var errFastImportExited = errors.New("git fast-import exited before reading the whole stream")

// repoDir is the local bare repository of mirror id ("" without a data
// directory).
func (s *Service) repoDir(id string) string {
	o := s.opts()
	if o.DataDir == "" || id == "" || strings.ContainsAny(id, `/\.`) {
		return ""
	}
	return filepath.Join(o.DataDir, "mirrors", id+".git")
}

// ---- git commands --------------------------------------------------------

// gitEnv is the environment of git commands: the server's environment
// without GIT_* variables (except the SSH command), an isolated empty
// global config, no prompts, the allowed transports and extra config
// entries (key, value pairs) passed through GIT_CONFIG_COUNT — the only
// way credentials reach git.
func (s *Service) gitEnv(configs ...string) []string {
	o := s.opts()
	var env []string
	sshCmd := false
	for _, kv := range os.Environ() {
		k, _, _ := strings.Cut(kv, "=")
		uk := strings.ToUpper(k)
		switch {
		case uk == "GIT_SSH" || uk == "GIT_SSH_COMMAND":
			sshCmd = true
		case strings.HasPrefix(uk, "GIT_") || uk == "SSH_ASKPASS" || uk == "GCM_INTERACTIVE":
			continue
		}
		env = append(env, kv)
	}
	protocols := "https:http:ssh"
	if o.AllowLocal {
		protocols += ":file"
	}
	global := filepath.Join(o.DataDir, "mirrors", "gitconfig")
	env = append(env,
		"GIT_CONFIG_NOSYSTEM=1",
		"GIT_CONFIG_GLOBAL="+global,
		"GIT_TERMINAL_PROMPT=0",
		"GCM_INTERACTIVE=never",
		"GIT_ALLOW_PROTOCOL="+protocols,
	)
	if !sshCmd {
		env = append(env, "GIT_SSH_COMMAND=ssh -o BatchMode=yes")
	}
	base := []string{"gc.auto", "0", "http.followRedirects", "false", "credential.helper", "", "core.askPass", ""}
	configs = append(base, configs...)
	n := len(configs) / 2
	env = append(env, fmt.Sprintf("GIT_CONFIG_COUNT=%d", n))
	for i := 0; i < n; i++ {
		env = append(env, fmt.Sprintf("GIT_CONFIG_KEY_%d=%s", i, configs[2*i]), fmt.Sprintf("GIT_CONFIG_VALUE_%d=%s", i, configs[2*i+1]))
	}
	return env
}

// runGit runs git with args in dir. stdin may be nil.
func (s *Service) runGit(ctx context.Context, dir string, env []string, stdin io.Reader, args ...string) (string, error) {
	g := s.gitBinary()
	if g.err != nil {
		return "", g.err
	}
	if hook := s.opts().CommandHook; hook != nil {
		hook(append([]string(nil), args...), append([]string(nil), env...))
	}
	cmd := exec.CommandContext(ctx, g.path, args...)
	cmd.Dir = dir
	cmd.Env = env
	cmd.Stdin = stdin
	cmd.WaitDelay = 5 * time.Second
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		msg := strings.TrimSpace(stderr.String())
		if msg == "" {
			msg = strings.TrimSpace(stdout.String())
		}
		if ctx.Err() != nil {
			return stdout.String(), fmt.Errorf("git %s: %w", args[0], ctx.Err())
		}
		return stdout.String(), fmt.Errorf("git %s failed: %v: %s", args[0], err, tail(msg, 1500))
	}
	return stdout.String(), nil
}

func tail(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return "…" + s[len(s)-n:]
}

// initRepo creates the bare repository of a mirror when missing and
// reports whether it was created.
func (s *Service) initRepo(ctx context.Context, dir string) (bool, error) {
	if _, err := os.Stat(filepath.Join(dir, "HEAD")); err == nil {
		return false, nil
	}
	parent := filepath.Dir(dir)
	if err := os.MkdirAll(parent, 0o750); err != nil {
		return false, err
	}
	if err := ensureFile(filepath.Join(parent, "gitconfig")); err != nil {
		return false, err
	}
	_ = removeAll(dir)
	if _, err := s.runGit(ctx, parent, s.gitEnv(), nil, "init", "--bare", "--quiet", dir); err != nil {
		return false, err
	}
	return true, nil
}

func ensureFile(p string) error {
	f, err := os.OpenFile(p, os.O_CREATE|os.O_WRONLY, 0o640)
	if err != nil {
		return err
	}
	return f.Close()
}

// localRefs returns the refs of the local repository (ref -> object id).
func (s *Service) localRefs(ctx context.Context, dir string) (map[string]string, error) {
	out, err := s.runGit(ctx, dir, s.gitEnv(), nil, "for-each-ref", "--format=%(objectname) %(refname)")
	if err != nil {
		return nil, err
	}
	refs := map[string]string{}
	for _, line := range strings.Split(out, "\n") {
		sha, ref, ok := strings.Cut(strings.TrimSpace(line), " ")
		if ok {
			refs[ref] = sha
		}
	}
	return refs, nil
}

// ---- credentials -----------------------------------------------------------

// authHeader is the http.extraHeader value for a credential: Basic auth
// of "user:password", or of "x-access-token:<token>" for a bare token.
func authHeader(cred string) string {
	if cred == "" {
		return ""
	}
	if !strings.Contains(cred, ":") {
		cred = "x-access-token:" + cred
	}
	return "Authorization: Basic " + base64.StdEncoding.EncodeToString([]byte(cred))
}

// redact removes every form of the credential from s.
func redact(s, cred string) string {
	if cred == "" {
		return s
	}
	h := authHeader(cred)
	for _, secret := range []string{h, strings.TrimPrefix(h, "Authorization: Basic "), strings.TrimPrefix(h, "Authorization: "), cred} {
		if len(secret) >= 4 {
			s = strings.ReplaceAll(s, secret, "[redacted]")
		}
	}
	if _, pass, ok := strings.Cut(cred, ":"); ok && len(pass) >= 4 {
		s = strings.ReplaceAll(s, pass, "[redacted]")
	}
	return s
}

// ---- sync ------------------------------------------------------------------

// Sync exports and pushes mirror id of tree now, waiting for a sync in
// progress first. actor names who asked ("" = automatic). A failed export
// or push is not an error: it is reported in the result and the mirror's
// status. Errors are returned for unknown or inactive mirrors and when
// mirrors are disabled.
func (s *Service) Sync(ctx context.Context, tree *domain.Tree, id, actor string) (*domain.Mirror, *SyncResult, error) {
	if _, err := s.load(ctx, tree, id); err != nil {
		return nil, nil, err
	}
	return s.sync(ctx, tree.ID, id, actor, true)
}

// sync runs one sync of mirror id. manual syncs report preconditions as
// errors; automatic ones (manual=false) skip inactive mirrors silently.
func (s *Service) sync(ctx context.Context, treeID, id, actor string, manual bool) (*domain.Mirror, *SyncResult, error) {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	s.mu.Lock()
	lock := s.lockOf(id)
	s.mu.Unlock()
	lock.Lock()
	defer lock.Unlock()
	s.mu.Lock()
	s.cancels[id] = cancel
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		delete(s.cancels, id)
		s.mu.Unlock()
	}()

	store := context.WithoutCancel(ctx)
	m, err := meta.GetDocT[mirrorDoc](store, s.d.Store, CollMirrors, MirrorKey(treeID, id))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, nil, errorf(domain.ErrNotFound, "mirror %q not found", id)
		}
		return nil, nil, err
	}
	if !m.Active {
		if manual {
			return nil, nil, errorf(domain.ErrConflict, "mirror %q is inactive; activate it before syncing", id)
		}
		return nil, nil, nil
	}
	if g := s.gitBinary(); g.err != nil {
		out, uerr := s.finish(store, m, StatusDisabled, g.err.Error(), nil, actor)
		if uerr != nil {
			return nil, nil, uerr
		}
		if manual {
			return nil, nil, errorf(domain.ErrConflict, "%s", g.err.Error())
		}
		return out, nil, nil
	}
	tree, err := s.d.Store.GetTree(store, treeID)
	if err != nil {
		return nil, nil, err
	}
	if _, err := s.setStatus(store, m, StatusRunning, ""); err != nil {
		return nil, nil, err
	}

	start := time.Now()
	res := &SyncResult{Refs: []RefUpdate{}}
	runErr := s.run(ctx, tree, m, res)
	res.DurationMS = time.Since(start).Milliseconds()
	status, msg := StatusOK, ""
	if runErr != nil {
		status, msg = StatusError, redact(runErr.Error(), m.Secret)
		if len(msg) > MaxError {
			msg = msg[:MaxError]
		}
		res.Error = msg
	}
	out, err := s.finish(store, m, status, msg, res, actor)
	if err != nil {
		return nil, nil, err
	}
	return out, res, nil
}

// setStatus records status/message on the stored mirror.
func (s *Service) setStatus(ctx context.Context, m *mirrorDoc, status, message string) (*domain.Mirror, error) {
	var out domain.Mirror
	now := s.now()
	err := meta.UpdateDocT(ctx, s.d.Store, CollMirrors, MirrorKey(m.TreeID, m.ID), func(cur *mirrorDoc) (*mirrorDoc, error) {
		if cur == nil {
			return nil, errorf(domain.ErrNotFound, "mirror %q not found", m.ID)
		}
		cur.LastStatus = status
		if status != StatusRunning {
			cur.LastError = message
			cur.LastSyncAt = &now
		}
		out = cur.public()
		return cur, nil
	})
	return &out, err
}

// finish records the outcome and publishes mirror.synced / mirror.failed.
func (s *Service) finish(ctx context.Context, m *mirrorDoc, status, message string, res *SyncResult, actor string) (*domain.Mirror, error) {
	out, err := s.setStatus(ctx, m, status, message)
	if err != nil {
		return nil, err
	}
	if actor == "" {
		actor = "system"
	}
	payload := map[string]any{"mirror_id": m.ID, "url": m.URL}
	typ := domain.EventMirrorSynced
	if status == StatusOK {
		if res != nil {
			payload["commits"] = res.Commits
			payload["refs"] = len(res.Refs)
			payload["pushed"] = res.Pushed
		}
	} else {
		typ = domain.EventMirrorFailed
		payload["status"] = status
		payload["error"] = message
	}
	if s.d.Bus != nil {
		s.d.Bus.Publish(domain.Event{Type: typ, TenantID: m.TenantID, TreeID: m.TreeID, Actor: actor, Payload: payload, At: s.now()})
	}
	if status != StatusOK {
		s.d.Logger.Warn("mirror: sync failed", "mirror", m.ID, "tree", m.TreeID, "url", m.URL, "status", status, "err", message)
	}
	return out, nil
}

// creator resolves the principal that created m (nil = gone or disabled).
func (s *Service) creator(ctx context.Context, m *mirrorDoc) *domain.Principal {
	if m.CreatorID != "" {
		a, err := s.d.Store.GetAccount(ctx, m.CreatorID)
		if err != nil || a.Disabled {
			return nil
		}
		return &domain.Principal{AccountID: a.ID, Username: a.Username, TenantID: a.TenantID, IsAdmin: a.IsAdmin}
	}
	rest, ok := strings.CutPrefix(m.CreatedBy, "sa:")
	if !ok {
		return nil
	}
	slug, name, ok := strings.Cut(rest, "/")
	if !ok {
		return nil
	}
	ten, err := s.d.Store.GetTenantBySlug(ctx, slug)
	if err != nil {
		return nil
	}
	sa, err := s.d.Store.GetServiceAccountByName(ctx, ten.ID, name)
	if err != nil || sa.Disabled {
		return nil
	}
	return &domain.Principal{Username: m.CreatedBy, TenantID: sa.TenantID, Kind: domain.PrincipalServiceAccount, ServiceAccountID: sa.ID}
}

// wanted is one ref the mirror should carry.
type wanted struct {
	ref    string // refs/heads/x or refs/tags/x
	target string // snapshot id
	tag    *domain.Tag
}

// run exports the mirror's refs into its local repository and pushes them.
func (s *Service) run(ctx context.Context, tree *domain.Tree, m *mirrorDoc, res *SyncResult) error {
	store := context.WithoutCancel(ctx)
	if s.d.Authz != nil {
		p := s.creator(store, m)
		if p == nil || s.d.Authz.Authorize(store, p, domain.ActTreeRead, tree.TenantID, tree) != nil {
			return fmt.Errorf("the mirror's creator %q may no longer read the tree; recreate the mirror as a current administrator", m.CreatedBy)
		}
	}
	if _, err := s.CheckURL(ctx, m.URL); err != nil {
		return err
	}
	dir := s.repoDir(m.ID)
	if dir == "" {
		return errors.New("mirrors need a server data directory")
	}
	created, err := s.initRepo(ctx, dir)
	if err != nil {
		return err
	}

	// What to mirror.
	branches, err := s.d.Store.ListBranches(store, tree.ID)
	if err != nil {
		return err
	}
	var want []wanted
	existsInTree := map[string]bool{}
	for _, b := range branches {
		ref := "refs/heads/" + b.Name
		existsInTree[ref] = true
		if b.Tip == "" || !MatchBranch(m.Branches, b.Name) {
			continue
		}
		if !ValidRefName(ref) {
			res.Skipped = appendLimited(res.Skipped, "branch "+b.Name)
			continue
		}
		want = append(want, wanted{ref: ref, target: b.Tip})
	}
	tags, err := s.d.Store.ListTags(store, tree.ID)
	if err != nil {
		return err
	}
	for i := range tags {
		t := tags[i]
		ref := "refs/tags/" + t.Name
		existsInTree[ref] = true
		if !m.Tags || t.Target == "" {
			continue
		}
		if !ValidRefName(ref) {
			res.Skipped = appendLimited(res.Skipped, "tag "+t.Name)
			continue
		}
		want = append(want, wanted{ref: ref, target: t.Target, tag: &t})
	}
	sort.Slice(want, func(i, j int) bool { return want[i].ref < want[j].ref })

	// Export state.
	rules, err := s.d.Store.ListLicenseRules(store, tree.ID)
	if err != nil && !errors.Is(err, domain.ErrNotFound) {
		return err
	}
	f := newFilter(rules, s.now())
	state, err := meta.GetDocT[mapDoc](store, s.d.Store, CollMaps, MapKey(m.ID))
	if errors.Is(err, domain.ErrNotFound) {
		state, err = &mapDoc{MirrorID: m.ID}, nil
	}
	if err != nil {
		return err
	}
	if state.Commits == nil || created || state.Filter != f.fingerprint() {
		state.Commits = map[string]string{}
	}
	if state.Remote == nil {
		state.Remote = map[string]string{}
	}
	state.Filter = f.fingerprint()

	n, err := s.export(ctx, dir, f, state, want, res)
	if err != nil && len(state.Commits) > 0 && ctx.Err() == nil {
		// The local repository may have lost objects the map refers to:
		// rebuild it from scratch once.
		s.d.Logger.Warn("mirror: incremental export failed; rebuilding", "mirror", m.ID, "err", err)
		state.Commits = map[string]string{}
		_ = removeAll(dir)
		if _, err = s.initRepo(ctx, dir); err == nil {
			n, err = s.export(ctx, dir, f, state, want, res)
		}
	}
	if err != nil {
		return err
	}
	res.Commits = n
	state.UpdatedAt = s.now()
	if err := s.d.Store.PutDoc(store, CollMaps, MapKey(m.ID), state); err != nil {
		return err
	}

	// Push.
	local, err := s.localRefs(ctx, dir)
	if err != nil {
		return err
	}
	pushErr := s.push(ctx, dir, m, state, local, want, existsInTree, res)
	state.UpdatedAt = s.now()
	if err := s.d.Store.PutDoc(store, CollMaps, MapKey(m.ID), state); err != nil {
		return errors.Join(pushErr, err)
	}
	return pushErr
}

func appendLimited(list []string, v string) []string {
	if len(list) < maxListed {
		list = append(list, v)
	}
	return list
}

// export writes the missing commits and annotated tags with fast-import
// and sets the local refs to want. It returns the number of new commits
// and records them in state.Commits.
func (s *Service) export(ctx context.Context, dir string, f filter, state *mapDoc, want []wanted, res *SyncResult) (int, error) {
	e := newExporter(ctx, s, f, state.Commits)
	var tips []string
	var keep []wanted
	for _, w := range want {
		if _, err := e.snapshot(w.target); err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				res.Skipped = appendLimited(res.Skipped, w.ref+" (missing snapshot "+w.target+")")
				continue
			}
			return 0, err
		}
		tips = append(tips, w.target)
		keep = append(keep, w)
	}
	order, err := e.plan(tips)
	if err != nil {
		return 0, err
	}
	var annotated []domain.Tag
	for _, w := range keep {
		if w.tag != nil && w.tag.Message != "" {
			annotated = append(annotated, *w.tag)
		}
	}

	if len(order) > 0 || len(annotated) > 0 {
		marksFile := filepath.Join(dir, "wt-mirror-marks")
		_ = os.Remove(marksFile)
		defer os.Remove(marksFile)
		pr, pw := io.Pipe()
		werr := make(chan error, 1)
		go func() {
			err := e.stream(pw, order, annotated)
			pw.CloseWithError(err)
			werr <- err
		}()
		_, runErr := s.runGit(ctx, dir, s.gitEnv(), pr, "fast-import", "--quiet", "--force", "--done", "--date-format=raw", "--export-marks="+marksFile)
		_ = pr.CloseWithError(errFastImportExited)
		streamErr := <-werr
		switch {
		case streamErr != nil && !errors.Is(streamErr, errFastImportExited):
			// Reading the tree failed (missing object, cancelled...): the
			// stream was cut, so fast-import failed too; report the cause.
			return 0, streamErr
		case runErr != nil:
			return 0, runErr
		case streamErr != nil:
			return 0, streamErr
		}
		raw, err := os.ReadFile(marksFile)
		if err != nil {
			return 0, fmt.Errorf("read fast-import marks: %w", err)
		}
		marks := parseMarks(string(raw))
		for id, mark := range e.marks {
			sha, ok := marks[mark]
			if !ok {
				return 0, fmt.Errorf("fast-import did not report the commit of snapshot %s", id)
			}
			state.Commits[id] = sha
		}
	}
	for p := range e.excluded {
		res.Excluded = appendLimited(res.Excluded, p)
	}
	sort.Strings(res.Excluded)
	for p := range e.skipped {
		res.Skipped = appendLimited(res.Skipped, "path "+p)
	}

	// Local refs: exactly the wanted ones (annotated tags were written by
	// fast-import).
	local, err := s.localRefs(ctx, dir)
	if err != nil {
		return 0, err
	}
	var cmds strings.Builder
	desired := map[string]bool{}
	for _, w := range keep {
		desired[w.ref] = true
		if w.tag != nil && w.tag.Message != "" {
			continue
		}
		sha := state.Commits[w.target]
		if sha == "" {
			return 0, fmt.Errorf("no commit for snapshot %s (%s)", w.target, w.ref)
		}
		if local[w.ref] != sha {
			fmt.Fprintf(&cmds, "update %s %s\n", w.ref, sha)
		}
	}
	for ref := range local {
		if !desired[ref] {
			fmt.Fprintf(&cmds, "delete %s\n", ref)
		}
	}
	if cmds.Len() > 0 {
		if _, err := s.runGit(ctx, dir, s.gitEnv(), strings.NewReader(cmds.String()), "update-ref", "--stdin"); err != nil {
			return 0, err
		}
	}
	return len(e.written), nil
}

// push pushes the wanted refs (and deletes the refs of deleted branches
// and tags that this mirror pushed before) with --force-with-lease, then
// records the refs that were updated in state.Remote.
func (s *Service) push(ctx context.Context, dir string, m *mirrorDoc, state *mapDoc, local map[string]string, want []wanted, existsInTree map[string]bool, res *SyncResult) error {
	type update struct {
		ref, sha string // sha "" = delete
	}
	var updates []update
	desired := map[string]bool{}
	for _, w := range want {
		if sha := local[w.ref]; sha != "" {
			desired[w.ref] = true
			updates = append(updates, update{w.ref, sha})
		}
	}
	for ref := range state.Remote {
		if desired[ref] {
			continue
		}
		if existsInTree[ref] {
			delete(state.Remote, ref) // no longer mirrored: leave it alone
			continue
		}
		updates = append(updates, update{ref: ref}) // deleted in the tree
	}
	if len(updates) == 0 {
		return nil
	}
	sort.Slice(updates, func(i, j int) bool { return updates[i].ref < updates[j].ref })

	args := []string{"push", "--porcelain"}
	var specs []string
	for _, u := range updates {
		if old, ok := state.Remote[u.ref]; ok {
			args = append(args, "--force-with-lease="+u.ref+":"+old)
		}
		if u.sha == "" {
			specs = append(specs, ":"+u.ref)
		} else {
			specs = append(specs, u.ref+":"+u.ref)
		}
	}
	args = append(args, "--", m.URL)
	args = append(args, specs...)
	var configs []string
	if h := authHeader(m.Secret); h != "" {
		configs = append(configs, "http.extraHeader", h)
	}
	pctx, cancel := context.WithTimeout(ctx, s.opts().PushTimeout)
	defer cancel()
	out, err := s.runGit(pctx, dir, s.gitEnv(configs...), nil, args...)

	statuses := parsePorcelain(out)
	for _, u := range updates {
		st, ok := statuses[u.ref]
		if !ok {
			if err != nil {
				res.Refs = append(res.Refs, RefUpdate{Ref: u.ref, New: u.sha, Status: "error"})
			}
			continue
		}
		ru := RefUpdate{Ref: u.ref, New: u.sha, Reason: redact(st.reason, m.Secret)}
		switch st.flag {
		case ' ':
			ru.Status = "updated"
		case '+':
			ru.Status = "forced"
		case '*':
			ru.Status = "created"
		case '-':
			ru.Status = "deleted"
		case '=':
			ru.Status = "up_to_date"
		default:
			ru.Status = "rejected"
		}
		if ru.Status != "rejected" {
			if u.sha == "" {
				delete(state.Remote, u.ref)
			} else {
				state.Remote[u.ref] = u.sha
			}
		}
		res.Refs = append(res.Refs, ru)
	}
	if err != nil {
		return err
	}
	res.Pushed = true
	return nil
}

type porcelainStatus struct {
	flag   byte
	reason string
}

// parsePorcelain reads `git push --porcelain` output: "<flag>\t<src>:<dst>\t<summary> (<reason>)".
func parsePorcelain(out string) map[string]porcelainStatus {
	st := map[string]porcelainStatus{}
	for _, line := range strings.Split(out, "\n") {
		line = strings.TrimRight(line, "\r")
		if len(line) < 3 || line[1] != '\t' {
			continue
		}
		parts := strings.SplitN(line[2:], "\t", 2)
		_, dst, ok := strings.Cut(parts[0], ":")
		if !ok {
			continue
		}
		reason := ""
		if len(parts) == 2 {
			reason = parts[1]
		}
		st[dst] = porcelainStatus{flag: line[0], reason: reason}
	}
	return st
}

// ---- automatic syncs -------------------------------------------------------

// syncEvents are the tree events that trigger a sync.
var syncEvents = map[string]bool{
	domain.EventBranchPushed:  true,
	domain.EventBranchCreated: true,
	domain.EventBranchDeleted: true,
	domain.EventTagPushed:     true,
	domain.EventTagDeleted:    true,
	domain.EventMRMerged:      true,
}

// Start subscribes to the bus (syncing the mirrors of a tree Debounce
// after its pushes, branch/tag changes and merges, and removing the
// mirrors of deleted trees) and resumes mirrors whose last sync did not
// finish or failed. It returns once subscribed; background work stops
// when ctx is cancelled. Call Configure before Start.
func (s *Service) Start(ctx context.Context) {
	s.mu.Lock()
	s.base = ctx
	s.mu.Unlock()
	var events <-chan domain.Event
	cancel := func() {}
	if s.d.Bus != nil {
		events, cancel = s.d.Bus.Subscribe(domain.EventFilter{})
	}
	if docs, err := meta.ListDocsT[mirrorDoc](ctx, s.d.Store, CollMirrors, ""); err == nil {
		for i := range docs {
			d := &docs[i]
			if d.Active && (d.LastStatus == StatusRunning || d.LastStatus == StatusError || d.LastStatus == "") {
				s.schedule(d.TreeID, d.ID, s.opts().Debounce)
			}
		}
	} else {
		s.d.Logger.WarnContext(ctx, "mirror: listing mirrors failed", "err", err)
	}
	go func() {
		defer cancel()
		for {
			select {
			case <-ctx.Done():
				s.mu.Lock()
				for id, t := range s.timers {
					t.Stop()
					delete(s.timers, id)
				}
				s.mu.Unlock()
				return
			case e, ok := <-events:
				if !ok {
					events = nil
					continue
				}
				if err := s.HandleEvent(ctx, e); err != nil {
					s.d.Logger.WarnContext(ctx, "mirror: handling event failed", "type", e.Type, "tree", e.TreeID, "err", err)
				}
			}
		}
	}()
}

// HandleEvent schedules the syncs an event calls for (Start calls it for
// each bus event; tests call it directly). tree.deleted removes the
// mirrors of the tree.
func (s *Service) HandleEvent(ctx context.Context, e domain.Event) error {
	if e.TreeID == "" {
		return nil
	}
	if e.Type == domain.EventTreeDeleted {
		return s.deleteTreeMirrors(ctx, e.TreeID)
	}
	if !syncEvents[e.Type] {
		return nil
	}
	docs, err := s.mirrorsOfTree(ctx, e.TreeID)
	if err != nil {
		return err
	}
	for i := range docs {
		if docs[i].Active {
			s.mu.Lock()
			delete(s.failures, docs[i].ID) // new content: a new retry cycle
			s.mu.Unlock()
			s.schedule(e.TreeID, docs[i].ID, s.opts().Debounce)
		}
	}
	return nil
}

// schedule (re)arms the automatic sync of mirror id after delay.
func (s *Service) schedule(treeID, id string, delay time.Duration) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.base != nil && s.base.Err() != nil {
		return
	}
	if t := s.timers[id]; t != nil {
		t.Stop()
	}
	var t *time.Timer
	t = time.AfterFunc(delay, func() {
		s.mu.Lock()
		if s.timers[id] != t {
			s.mu.Unlock()
			return
		}
		delete(s.timers, id)
		base := s.base
		s.wg.Add(1)
		s.mu.Unlock()
		defer s.wg.Done()
		if base == nil {
			base = context.Background()
		}
		s.runAuto(base, treeID, id)
	})
	s.timers[id] = t
}

func (s *Service) unschedule(id string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if t := s.timers[id]; t != nil {
		t.Stop()
		delete(s.timers, id)
	}
}

// runAuto runs an automatic sync and schedules a retry when it fails.
func (s *Service) runAuto(ctx context.Context, treeID, id string) {
	if ctx.Err() != nil {
		return
	}
	out, _, err := s.sync(ctx, treeID, id, "", false)
	if err != nil {
		if !errors.Is(err, domain.ErrNotFound) {
			s.d.Logger.WarnContext(ctx, "mirror: automatic sync failed", "mirror", id, "err", err)
		}
		return
	}
	if out == nil || ctx.Err() != nil {
		return
	}
	backoff := s.opts().Backoff
	s.mu.Lock()
	if out.LastStatus == StatusOK {
		delete(s.failures, id)
		s.mu.Unlock()
		return
	}
	n := s.failures[id]
	s.failures[id] = n + 1
	s.mu.Unlock()
	if out.LastStatus == StatusError && n < len(backoff) {
		s.schedule(treeID, id, backoff[n])
	}
}

// Scheduled reports whether an automatic sync of mirror id is pending.
func (s *Service) Scheduled(id string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.timers[id] != nil
}

// Wait blocks until the automatic syncs running now have finished.
func (s *Service) Wait() { s.wg.Wait() }
