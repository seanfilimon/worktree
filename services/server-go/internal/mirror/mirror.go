// Package mirror pushes trees to Git remotes (live push mirrors) and
// records their status (docs/ENGINEERING_PLAN.md §15.6).
//
// A mirror belongs to one tree. On branch.pushed, branch.created,
// branch.deleted, tag.pushed, tag.deleted and mr.merged events of the tree
// (debounced, Options.Debounce = 5 s) and on demand (Sync), the Service
// exports the snapshots the mirror has not seen yet into a bare Git
// repository <data>/mirrors/<id>.git with `git fast-import` and then runs
// `git push --porcelain --force-with-lease=<ref>:<last pushed>` for the
// mirrored refs. Commits are deterministic (author and committer from the
// snapshot author at its created_at in UTC, the snapshot message, the tree
// from the manifest with chunked files reassembled and mode 100755 for
// executables, parents in snapshot order): the same history always yields
// the same commit ids, matching `wt git export` (crates/worktree-git).
//
// License rules apply as for archives (§13.8): files whose general rule
// does not permit redistribution are left out and a LICENSES/NOTICE lists
// the SPDX ids of the included files.
//
// Credentials never appear on a command line or in logs: an HTTP(S)
// credential reaches git as an `http.extraHeader` through the
// GIT_CONFIG_COUNT/KEY/VALUE environment. Remote URLs pass the webhooks
// SSRF guard (no loopback, link-local or private hosts) and file:// URLs
// are refused, unless Options.AllowLocal (--mirror-allow-local).
//
// Mirrors need the git binary (Options.GitBinary, --git-binary, at least
// version 2.31); without it they are disabled with a clear status.
//
// Storage (§15): collection "mirrors", key DocKey(treeID, id), value
// mirrorDoc (the domain.Mirror plus its credential and the creator's
// account id); collection "mirror_maps", key DocKey(mirrorID), value
// mapDoc (snapshot id → commit id and the refs last pushed).
package mirror

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/netip"
	"net/url"
	"os"
	"os/exec"
	"path"
	"regexp"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/webhooks"
)

// Document collections owned by this package.
const (
	CollMirrors = "mirrors"
	CollMaps    = "mirror_maps"
)

// Mirror.LastStatus values.
const (
	StatusOK       = "ok"
	StatusError    = "error"
	StatusRunning  = "running"
	StatusDisabled = "disabled" // the git binary is missing or too old
)

// Defaults and limits.
const (
	DefaultDebounce    = 5 * time.Second
	DefaultPushTimeout = 10 * time.Minute
	MaxMirrorsPerTree  = 10
	MaxURL             = 2048
	MaxPatterns        = 50
	MaxPattern         = 200
	MaxCredential      = 4096
	MaxError           = 2048 // bytes of LastError kept

	// MinGitVersion is the oldest git that supports GIT_CONFIG_COUNT.
	MinGitVersion = "2.31"
)

// DefaultBackoff are the delays before retrying a failed automatic sync.
var DefaultBackoff = []time.Duration{time.Minute, 5 * time.Minute, 30 * time.Minute, 2 * time.Hour}

// Deps are the collaborators of the mirror package. Store and Objects are
// required; Authz enables the creator re-check before each sync, Bus the
// automatic syncs (Start) and the mirror.synced / mirror.failed events.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Options tune the Service. The zero value selects the defaults.
type Options struct {
	// DataDir holds mirrors/<id>.git (the server's --data directory).
	DataDir string
	// GitBinary is the git executable (name looked up on PATH, or a path;
	// "" = "git"). --git-binary, WT_GIT_BINARY.
	GitBinary string
	// AllowLocal permits file:// remotes and hosts on loopback, link-local
	// or private addresses (--mirror-allow-local).
	AllowLocal bool
	// Resolver resolves remote hosts for the SSRF check (nil = DNS).
	Resolver func(ctx context.Context, host string) ([]netip.Addr, error)
	// Debounce delays automatic syncs after an event (0 = DefaultDebounce).
	Debounce time.Duration
	// PushTimeout bounds one `git push` (0 = DefaultPushTimeout).
	PushTimeout time.Duration
	// Backoff are the retry delays of failed automatic syncs (nil =
	// DefaultBackoff; empty = no retries).
	Backoff []time.Duration
	// Now is the clock (nil = time.Now().UTC()).
	Now func() time.Time
	// CommandHook, when set, sees the arguments and environment of every
	// git command before it runs (tests).
	CommandHook func(args, env []string)
}

func (o Options) withDefaults() Options {
	if o.GitBinary == "" {
		o.GitBinary = "git"
	}
	if o.Resolver == nil {
		o.Resolver = defaultResolve
	}
	if o.Debounce <= 0 {
		o.Debounce = DefaultDebounce
	}
	if o.PushTimeout <= 0 {
		o.PushTimeout = DefaultPushTimeout
	}
	if o.Backoff == nil {
		o.Backoff = DefaultBackoff
	}
	if o.Now == nil {
		o.Now = func() time.Time { return time.Now().UTC() }
	}
	return o
}

// Service is the mirror component.
type Service struct {
	d Deps

	mu       sync.Mutex
	o        Options
	git      *gitInfo // nil until detected
	base     context.Context
	timers   map[string]*time.Timer // mirror id -> scheduled automatic sync
	locks    map[string]*sync.Mutex // mirror id -> serializes syncs
	cancels  map[string]context.CancelFunc
	failures map[string]int // mirror id -> consecutive automatic failures
	wg       sync.WaitGroup // background syncs
}

// New returns a Service with the default Options.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{
		d:        d,
		o:        Options{}.withDefaults(),
		timers:   map[string]*time.Timer{},
		locks:    map[string]*sync.Mutex{},
		cancels:  map[string]context.CancelFunc{},
		failures: map[string]int{},
	}
}

// Configure replaces the Options (and re-detects git); call it before
// Start.
func (s *Service) Configure(o Options) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.o = o.withDefaults()
	s.git = nil
}

func (s *Service) opts() Options {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.o
}

func (s *Service) now() time.Time { return s.opts().Now().UTC() }

// Error is a domain error with a client-facing message. errors.Is matches
// its Kind (domain.ErrNotFound, ErrInvalid, ErrConflict, ...).
type Error struct {
	Kind error
	Msg  string
}

func (e *Error) Error() string { return e.Msg }
func (e *Error) Unwrap() error { return e.Kind }

func errorf(kind error, format string, args ...any) error {
	return &Error{Kind: kind, Msg: fmt.Sprintf(format, args...)}
}

// ---- git binary ----------------------------------------------------------

// gitInfo is the result of git detection.
type gitInfo struct {
	path    string
	version string
	err     error
}

// GitStatus reports whether mirrors can run on this server.
type GitStatus struct {
	Available bool   `json:"available"`
	Version   string `json:"version,omitempty"` // e.g. "2.45.1"
	Error     string `json:"error,omitempty"`   // why mirrors are disabled
}

var gitVersionRe = regexp.MustCompile(`git version (\d+)\.(\d+)(?:\.(\d+))?`)

// detectGit finds bin and checks its version.
func detectGit(bin string) *gitInfo {
	p, err := exec.LookPath(bin)
	if err != nil {
		return &gitInfo{err: fmt.Errorf("git binary %q not found; mirrors are disabled (install git or start the server with --git-binary PATH)", bin)}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, p, "--version").Output()
	if err != nil {
		return &gitInfo{path: p, err: fmt.Errorf("running %q --version failed (%v); mirrors are disabled", p, err)}
	}
	m := gitVersionRe.FindStringSubmatch(string(out))
	if m == nil {
		return &gitInfo{path: p, err: fmt.Errorf("cannot parse the version of %q (%q); mirrors are disabled", p, strings.TrimSpace(string(out)))}
	}
	major, _ := strconv.Atoi(m[1])
	minor, _ := strconv.Atoi(m[2])
	version := m[1] + "." + m[2]
	if m[3] != "" {
		version += "." + m[3]
	}
	if major < 2 || (major == 2 && minor < 31) {
		return &gitInfo{path: p, version: version, err: fmt.Errorf("git %s is too old (mirrors need git %s or newer); mirrors are disabled", version, MinGitVersion)}
	}
	return &gitInfo{path: p, version: version}
}

// gitBinary returns the detected git (detecting it on first use).
func (s *Service) gitBinary() *gitInfo {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.git == nil {
		s.git = detectGit(s.o.GitBinary)
	}
	return s.git
}

// Git reports the git binary status.
func (s *Service) Git() GitStatus {
	g := s.gitBinary()
	if g.err != nil {
		return GitStatus{Version: g.version, Error: g.err.Error()}
	}
	return GitStatus{Available: true, Version: g.version}
}

// ---- storage -----------------------------------------------------------

// mirrorDoc is the stored form of a mirror (domain.Mirror never encodes
// its credential).
type mirrorDoc struct {
	domain.Mirror
	Secret    string `json:"credential,omitempty"`
	CreatorID string `json:"creator_id,omitempty"` // account id ("" for service accounts)
}

func (m *mirrorDoc) public() domain.Mirror {
	out := m.Mirror
	out.Credential = ""
	out.HasCredential = m.Secret != ""
	out.Branches = slices.Clone(out.Branches)
	return out
}

// MirrorKey is the "mirrors" document key of mirror id of treeID.
func MirrorKey(treeID, id string) string { return domain.DocKey(treeID, id) }

// MapKey is the "mirror_maps" document key of mirror id.
func MapKey(id string) string { return domain.DocKey(id) }

// mapDoc is the persisted export state of a mirror.
type mapDoc struct {
	MirrorID string `json:"mirror_id"`
	// Filter fingerprints the license filter the commits were built with;
	// when the rules change, the history is exported again.
	Filter string `json:"filter"`
	// Commits maps snapshot ids to the Git commit ids written for them.
	Commits map[string]string `json:"commits"`
	// Remote maps refs to the object id last pushed successfully (the
	// --force-with-lease expectation).
	Remote    map[string]string `json:"remote"`
	UpdatedAt time.Time         `json:"updated_at"`
}

func (s *Service) mirrorsOfTree(ctx context.Context, treeID string) ([]mirrorDoc, error) {
	return meta.ListDocsT[mirrorDoc](ctx, s.d.Store, CollMirrors, meta.DocPrefix(treeID))
}

func (s *Service) load(ctx context.Context, tree *domain.Tree, id string) (*mirrorDoc, error) {
	m, err := meta.GetDocT[mirrorDoc](ctx, s.d.Store, CollMirrors, MirrorKey(tree.ID, id))
	if errors.Is(err, domain.ErrNotFound) {
		return nil, errorf(domain.ErrNotFound, "mirror %q not found in tree %s", id, tree.Name)
	}
	return m, err
}

// ---- validation ----------------------------------------------------------

func defaultResolve(ctx context.Context, host string) ([]netip.Addr, error) {
	return net.DefaultResolver.LookupNetIP(ctx, "ip", host)
}

// scpRe matches scp-like Git remotes: [user@]host:path.
var scpRe = regexp.MustCompile(`^(?:([A-Za-z0-9._~][A-Za-z0-9._~-]*)@)?([A-Za-z0-9][A-Za-z0-9.-]+|\[[0-9A-Fa-f:.]+\]):(.*)$`)

// remote is a parsed mirror URL.
type remote struct {
	scheme string // https, http, ssh, file
	host   string
}

func parseRemote(raw string) (remote, error) {
	if !strings.Contains(raw, "://") {
		if m := scpRe.FindStringSubmatch(raw); m != nil && m[3] != "" {
			return remote{scheme: "ssh", host: strings.Trim(m[2], "[]")}, nil
		}
		return remote{}, errorf(domain.ErrInvalid, "url must be an https://, http://, ssh:// or scp-like (user@host:path) Git remote")
	}
	u, err := url.Parse(raw)
	if err != nil || u.Opaque != "" {
		return remote{}, errorf(domain.ErrInvalid, "url is not a valid URL")
	}
	scheme := strings.ToLower(u.Scheme)
	switch scheme {
	case "https", "http", "ssh":
		if u.Hostname() == "" {
			return remote{}, errorf(domain.ErrInvalid, "url must name a host")
		}
		if _, ok := u.User.Password(); ok {
			return remote{}, errorf(domain.ErrInvalid, "url must not contain a password: pass it as the mirror credential (wt mirror add --token-env VAR)")
		}
		if strings.HasPrefix(u.User.Username(), "-") || strings.HasPrefix(u.Hostname(), "-") {
			return remote{}, errorf(domain.ErrInvalid, "url user and host must not start with '-'")
		}
		return remote{scheme: scheme, host: u.Hostname()}, nil
	case "file":
		if u.Host != "" && !strings.EqualFold(u.Host, "localhost") {
			return remote{}, errorf(domain.ErrInvalid, "file:// urls must not name a host")
		}
		if u.Path == "" || u.Path == "/" {
			return remote{}, errorf(domain.ErrInvalid, "file:// url must name a repository path")
		}
		return remote{scheme: "file"}, nil
	default:
		return remote{}, errorf(domain.ErrInvalid, "unsupported url scheme %q (want https, http, ssh or scp-like user@host:path)", u.Scheme)
	}
}

// CheckURL validates a mirror URL and, unless AllowLocal, refuses file://
// URLs and hosts that are or resolve to loopback, link-local or private
// addresses (the webhooks SSRF guard). It returns the trimmed URL.
func (s *Service) CheckURL(ctx context.Context, raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", errorf(domain.ErrInvalid, "url is required")
	}
	if len(raw) > MaxURL {
		return "", errorf(domain.ErrInvalid, "url must be at most %d bytes", MaxURL)
	}
	// Local paths may contain spaces (the URL is one argv element after
	// "--"); network URLs may not.
	local := len(raw) >= 7 && strings.EqualFold(raw[:7], "file://")
	if strings.HasPrefix(raw, "-") || strings.IndexFunc(raw, func(r rune) bool { return (unicode.IsSpace(r) && !(local && r == ' ')) || unicode.IsControl(r) }) >= 0 {
		return "", errorf(domain.ErrInvalid, "url must not start with '-' or contain spaces or control characters")
	}
	r, err := parseRemote(raw)
	if err != nil {
		return "", err
	}
	o := s.opts()
	if r.scheme == "file" {
		if !o.AllowLocal {
			return "", errorf(domain.ErrInvalid, "file:// mirrors are refused unless the server is started with --mirror-allow-local")
		}
		return raw, nil
	}
	if o.AllowLocal {
		return raw, nil
	}
	var addrs []netip.Addr
	if ip, err := netip.ParseAddr(r.host); err == nil {
		addrs = []netip.Addr{ip}
	} else {
		rctx, cancel := context.WithTimeout(ctx, 5*time.Second)
		addrs, err = o.Resolver(rctx, r.host)
		cancel()
		if err != nil || len(addrs) == 0 {
			return "", errorf(domain.ErrInvalid, "cannot resolve mirror host %q", r.host)
		}
	}
	for _, ip := range addrs {
		if webhooks.Blocked(ip) {
			return "", errorf(domain.ErrInvalid,
				"mirror host %q resolves to the non-public address %s; the server refuses loopback, link-local and private remotes unless started with --mirror-allow-local",
				r.host, ip.Unmap())
		}
	}
	return raw, nil
}

// NormalizePatterns validates branch glob patterns ("main", "release/*",
// "feature/**"), dropping blanks and duplicates. Empty = every branch.
func NormalizePatterns(patterns []string) ([]string, error) {
	if len(patterns) > MaxPatterns {
		return nil, errorf(domain.ErrInvalid, "at most %d branch patterns", MaxPatterns)
	}
	out := []string{}
	for _, p := range patterns {
		p = strings.TrimSpace(p)
		if p == "" {
			continue
		}
		if len(p) > MaxPattern || strings.IndexFunc(p, func(r rune) bool { return unicode.IsSpace(r) || unicode.IsControl(r) }) >= 0 {
			return nil, errorf(domain.ErrInvalid, "invalid branch pattern %q", p)
		}
		for _, seg := range strings.Split(p, "/") {
			if seg == "" {
				return nil, errorf(domain.ErrInvalid, "invalid branch pattern %q (empty segment)", p)
			}
			if _, err := path.Match(seg, ""); err != nil {
				return nil, errorf(domain.ErrInvalid, "invalid branch pattern %q: %v", p, err)
			}
		}
		if !slices.Contains(out, p) {
			out = append(out, p)
		}
	}
	return out, nil
}

// MatchBranch reports whether branch matches one of patterns (empty =
// every branch). Patterns are '/'-separated: "*" matches within one
// segment, "**" as a whole segment matches any number of segments.
func MatchBranch(patterns []string, branch string) bool {
	if len(patterns) == 0 {
		return true
	}
	name := strings.Split(branch, "/")
	for _, p := range patterns {
		if matchSegs(strings.Split(p, "/"), name) {
			return true
		}
	}
	return false
}

func matchSegs(pat, name []string) bool {
	for len(pat) > 0 {
		if pat[0] == "**" {
			for i := len(name); i >= 0; i-- {
				if matchSegs(pat[1:], name[i:]) {
					return true
				}
			}
			return false
		}
		if len(name) == 0 {
			return false
		}
		if ok, err := path.Match(pat[0], name[0]); err != nil || !ok {
			return false
		}
		pat, name = pat[1:], name[1:]
	}
	return len(name) == 0
}

func validCredential(c string, r remote) error {
	if len(c) > MaxCredential || !utf8.ValidString(c) || strings.ContainsAny(c, "\r\n\x00") {
		return errorf(domain.ErrInvalid, "credential must be 1-%d bytes of UTF-8 without line breaks", MaxCredential)
	}
	if r.scheme != "https" && r.scheme != "http" {
		return errorf(domain.ErrInvalid, "a credential applies to http(s) remotes only; %s remotes use the server's own keys", r.scheme)
	}
	return nil
}

// ---- CRUD ------------------------------------------------------------------

// CreateInput is the body of POST {tree}/mirrors.
type CreateInput struct {
	URL string `json:"url"`
	// Branches are glob patterns of the branches to mirror (empty = all).
	Branches []string `json:"branches,omitempty"`
	// Tags mirrors the tree's tags (default true).
	Tags *bool `json:"tags,omitempty"`
	// Credential for http(s) remotes: a token or "user:password". Never
	// returned.
	Credential string `json:"credential,omitempty"`
	Active     *bool  `json:"active,omitempty"`
}

// Patch is the body of PATCH {tree}/mirrors/{id}; nil fields are unchanged.
type Patch struct {
	URL      *string   `json:"url,omitempty"`
	Branches *[]string `json:"branches,omitempty"`
	Tags     *bool     `json:"tags,omitempty"`
	// Credential replaces the credential; "" removes it.
	Credential *string `json:"credential,omitempty"`
	Active     *bool   `json:"active,omitempty"`
}

// List returns the mirrors of tree (oldest first).
func (s *Service) List(ctx context.Context, tree *domain.Tree) ([]domain.Mirror, error) {
	docs, err := s.mirrorsOfTree(ctx, tree.ID)
	if err != nil {
		return nil, err
	}
	out := make([]domain.Mirror, 0, len(docs))
	for i := range docs {
		out = append(out, docs[i].public())
	}
	slices.SortFunc(out, func(a, b domain.Mirror) int {
		if c := a.CreatedAt.Compare(b.CreatedAt); c != 0 {
			return c
		}
		return strings.Compare(a.ID, b.ID)
	})
	return out, nil
}

// Get returns mirror id of tree.
func (s *Service) Get(ctx context.Context, tree *domain.Tree, id string) (*domain.Mirror, error) {
	m, err := s.load(ctx, tree, id)
	if err != nil {
		return nil, err
	}
	out := m.public()
	return &out, nil
}

// Create adds a mirror to tree on behalf of p and, when active, schedules
// its first sync.
func (s *Service) Create(ctx context.Context, p *domain.Principal, tree *domain.Tree, in CreateInput) (*domain.Mirror, error) {
	if p == nil {
		return nil, errorf(domain.ErrUnauthenticated, "authentication required")
	}
	target, err := s.CheckURL(ctx, in.URL)
	if err != nil {
		return nil, err
	}
	r, _ := parseRemote(target)
	branches, err := NormalizePatterns(in.Branches)
	if err != nil {
		return nil, err
	}
	if in.Credential != "" {
		if err := validCredential(in.Credential, r); err != nil {
			return nil, err
		}
	}
	existing, err := s.mirrorsOfTree(ctx, tree.ID)
	if err != nil {
		return nil, err
	}
	if len(existing) >= MaxMirrorsPerTree {
		return nil, errorf(domain.ErrConflict, "tree %s already has %d mirrors (the maximum)", tree.Name, MaxMirrorsPerTree)
	}
	for i := range existing {
		if existing[i].URL == target {
			return nil, errorf(domain.ErrConflict, "tree %s already mirrors to %s", tree.Name, target)
		}
	}
	m := mirrorDoc{
		Mirror: domain.Mirror{
			ID:        uuid.NewString(),
			TenantID:  tree.TenantID,
			TreeID:    tree.ID,
			URL:       target,
			Branches:  branches,
			Tags:      in.Tags == nil || *in.Tags,
			Active:    in.Active == nil || *in.Active,
			CreatedBy: p.Username,
			CreatedAt: s.now(),
		},
		Secret:    in.Credential,
		CreatorID: p.AccountID,
	}
	if err := s.d.Store.CreateDoc(ctx, CollMirrors, MirrorKey(tree.ID, m.ID), m); err != nil {
		return nil, err
	}
	if m.Active {
		s.schedule(tree.ID, m.ID, s.opts().Debounce)
	}
	out := m.public()
	return &out, nil
}

// Update applies patch to mirror id of tree. changed lists the fields set
// (for the audit log). Changing the URL forgets what was pushed to the old
// remote; an active mirror is synced again soon.
func (s *Service) Update(ctx context.Context, tree *domain.Tree, id string, patch Patch) (mirror *domain.Mirror, changed []string, err error) {
	cur, err := s.load(ctx, tree, id)
	if err != nil {
		return nil, nil, err
	}
	target := cur.URL
	if patch.URL != nil {
		if target, err = s.CheckURL(ctx, *patch.URL); err != nil {
			return nil, nil, err
		}
	}
	r, _ := parseRemote(target)
	var branches []string
	if patch.Branches != nil {
		if branches, err = NormalizePatterns(*patch.Branches); err != nil {
			return nil, nil, err
		}
	}
	if patch.Credential != nil && *patch.Credential != "" {
		if err := validCredential(*patch.Credential, r); err != nil {
			return nil, nil, err
		}
	} else if patch.URL != nil && cur.Secret != "" && (patch.Credential == nil) {
		if err := validCredential(cur.Secret, r); err != nil {
			return nil, nil, err
		}
	}
	if patch.URL != nil && target != cur.URL {
		others, err := s.mirrorsOfTree(ctx, tree.ID)
		if err != nil {
			return nil, nil, err
		}
		for i := range others {
			if others[i].ID != id && others[i].URL == target {
				return nil, nil, errorf(domain.ErrConflict, "tree %s already mirrors to %s", tree.Name, target)
			}
		}
	}
	urlChanged := false
	var out domain.Mirror
	err = meta.UpdateDocT(ctx, s.d.Store, CollMirrors, MirrorKey(tree.ID, id), func(m *mirrorDoc) (*mirrorDoc, error) {
		if m == nil {
			return nil, errorf(domain.ErrNotFound, "mirror %q not found in tree %s", id, tree.Name)
		}
		changed = changed[:0]
		if patch.URL != nil {
			urlChanged = m.URL != target
			m.URL = target
			changed = append(changed, "url")
		}
		if patch.Branches != nil {
			m.Branches = branches
			changed = append(changed, "branches")
		}
		if patch.Tags != nil {
			m.Tags = *patch.Tags
			changed = append(changed, "tags")
		}
		if patch.Credential != nil {
			m.Secret = *patch.Credential
			changed = append(changed, "credential")
		}
		if patch.Active != nil {
			m.Active = *patch.Active
			changed = append(changed, "active")
		}
		out = m.public()
		return m, nil
	})
	if err != nil {
		return nil, nil, err
	}
	if urlChanged {
		if err := meta.UpdateDocT(ctx, s.d.Store, CollMaps, MapKey(id), func(m *mapDoc) (*mapDoc, error) {
			if m == nil {
				return nil, nil
			}
			m.Remote = map[string]string{}
			return m, nil
		}); err != nil {
			return nil, nil, err
		}
	}
	s.mu.Lock()
	delete(s.failures, id)
	s.mu.Unlock()
	if out.Active {
		s.schedule(tree.ID, id, s.opts().Debounce)
	} else {
		s.unschedule(id)
	}
	return &out, changed, nil
}

// Delete removes mirror id of tree, its export state and its local Git
// repository (cancelling a sync in progress). The remote is not touched.
func (s *Service) Delete(ctx context.Context, tree *domain.Tree, id string) error {
	if _, err := s.load(ctx, tree, id); err != nil {
		return err
	}
	return s.remove(ctx, tree.ID, id)
}

func (s *Service) remove(ctx context.Context, treeID, id string) error {
	s.unschedule(id)
	s.mu.Lock()
	if cancel := s.cancels[id]; cancel != nil {
		cancel()
	}
	lock := s.lockOf(id)
	s.mu.Unlock()
	lock.Lock()
	defer lock.Unlock()
	var errs []error
	if err := s.d.Store.DeleteDoc(ctx, CollMirrors, MirrorKey(treeID, id)); err != nil && !errors.Is(err, domain.ErrNotFound) {
		errs = append(errs, err)
	}
	if err := s.d.Store.DeleteDoc(ctx, CollMaps, MapKey(id)); err != nil && !errors.Is(err, domain.ErrNotFound) {
		errs = append(errs, err)
	}
	if dir := s.repoDir(id); dir != "" {
		if err := removeAll(dir); err != nil {
			errs = append(errs, fmt.Errorf("remove %s: %w", dir, err))
		}
	}
	s.mu.Lock()
	delete(s.failures, id)
	delete(s.locks, id)
	s.mu.Unlock()
	return errors.Join(errs...)
}

// deleteTreeMirrors removes the mirrors of a deleted tree.
func (s *Service) deleteTreeMirrors(ctx context.Context, treeID string) error {
	docs, err := s.mirrorsOfTree(ctx, treeID)
	if err != nil {
		return err
	}
	var errs []error
	for i := range docs {
		if err := s.remove(ctx, treeID, docs[i].ID); err != nil {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}

// removeAll is os.RemoveAll retried briefly (Windows keeps files of a
// just-exited git process locked for a moment).
func removeAll(dir string) error {
	var err error
	for i := 0; i < 10; i++ {
		if err = os.RemoveAll(dir); err == nil {
			return nil
		}
		time.Sleep(time.Duration(i+1) * 50 * time.Millisecond)
	}
	return err
}

// lockOf returns the sync lock of mirror id; s.mu must be held.
func (s *Service) lockOf(id string) *sync.Mutex {
	l := s.locks[id]
	if l == nil {
		l = &sync.Mutex{}
		s.locks[id] = l
	}
	return l
}
