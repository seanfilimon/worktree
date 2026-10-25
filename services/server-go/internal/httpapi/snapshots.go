package httpapi

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"mime"
	"net/http"
	"path"
	"sort"
	"strings"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/pathacl"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

const (
	defaultSnapshotLimit = 50
	maxSnapshotLimit     = 1000
)

// fileEntry is one manifest entry in the snapshot detail view.
type fileEntry struct {
	Path       string    `json:"path"`
	Hash       hash.Hash `json:"hash"`
	Size       uint64    `json:"size"`
	Executable bool      `json:"executable"`
}

type snapshotDetail struct {
	Snapshot domain.Snapshot `json:"snapshot"`
	Files    []fileEntry     `json:"files"`
	// RestrictedCount is the number of files left out of Files because the
	// caller may not read them (path policies or license rules, §13.1).
	RestrictedCount int `json:"restricted_count"`
}

func normalizeSnapshot(s *domain.Snapshot) { s.Parents = nonNil(s.Parents) }

// handleListSnapshots answers GET /api/v1/trees/{tenant}/{tree}/snapshots
// with the history of a branch (default: the tree's default branch),
// newest first.
func (a *api) handleListSnapshots(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	limit, err := queryInt(r, "limit", defaultSnapshotLimit, maxSnapshotLimit)
	if err != nil {
		return err
	}
	branchName := r.URL.Query().Get("branch")
	explicit := branchName != ""
	if !explicit {
		branchName = tree.DefaultBranch
	}
	ctx := r.Context()
	b, err := a.d.Store.GetBranch(ctx, tree.ID, branchName)
	switch {
	case errors.Is(err, domain.ErrNotFound) && !explicit:
		// Nothing pushed yet to the default branch.
		writeJSON(w, http.StatusOK, []domain.Snapshot{})
		return nil
	case errors.Is(err, domain.ErrNotFound):
		return notFoundf("branch %q not found", branchName)
	case err != nil:
		return err
	}
	if b.Tip == "" {
		writeJSON(w, http.StatusOK, []domain.Snapshot{})
		return nil
	}
	snaps, err := a.d.History.Log(ctx, b.Tip, limit)
	if err != nil {
		return err
	}
	snaps = nonNil(snaps)
	for i := range snaps {
		normalizeSnapshot(&snaps[i])
	}
	writeJSON(w, http.StatusOK, snaps)
	return nil
}

// treeSnapshot loads snapshot id and checks that it belongs to tree.
func (a *api) treeSnapshot(ctx context.Context, tree *domain.Tree, id string) (*domain.Snapshot, error) {
	s, err := a.d.Store.GetSnapshot(ctx, id)
	if errors.Is(err, domain.ErrNotFound) || (err == nil && s.TreeID != tree.ID) {
		return nil, notFoundf("snapshot %q not found", id)
	}
	return s, err
}

// loadManifest fetches and decodes the manifest object of s.
func (a *api) loadManifest(ctx context.Context, s *domain.Snapshot) (*syncpb.ManifestRecord, error) {
	raw, err := a.d.Objects.Get(ctx, s.ManifestHash)
	if err != nil {
		// A stored snapshot whose manifest is missing is server-side
		// corruption, not a client error: %v deliberately drops the
		// not-found sentinel so that this maps to 500.
		return nil, fmt.Errorf("snapshot %s: load manifest %s: %v", s.ID, s.ManifestHash, err)
	}
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("snapshot %s: decode manifest %s: %w", s.ID, s.ManifestHash, err)
	}
	return &m, nil
}

func entryHash(e *syncpb.ManifestEntry) (hash.Hash, error) {
	var h hash.Hash
	v := e.GetBlobHash().GetValue()
	if len(v) != hash.Size {
		return h, fmt.Errorf("manifest entry %q: blob hash has %d bytes, want %d", e.GetPath(), len(v), hash.Size)
	}
	copy(h[:], v)
	return h, nil
}

// handleGetSnapshot answers GET /api/v1/trees/{tenant}/{tree}/snapshots/{id}.
func (a *api) handleGetSnapshot(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	ctx := r.Context()
	s, err := a.treeSnapshot(ctx, tree, r.PathValue("id"))
	if err != nil {
		return err
	}
	m, err := a.loadManifest(ctx, s)
	if err != nil {
		return err
	}
	paths := make([]string, 0, len(m.GetEntries()))
	for _, e := range m.GetEntries() {
		paths = append(paths, e.GetPath())
	}
	rs, err := a.readRestrictions(ctx, principal(r), tree, paths)
	if err != nil {
		return err
	}
	files := make([]fileEntry, 0, len(m.GetEntries()))
	restricted := 0
	for _, e := range m.GetEntries() {
		if rs.restricted(e.GetPath()) {
			restricted++
			continue
		}
		h, err := entryHash(e)
		if err != nil {
			return err
		}
		files = append(files, fileEntry{Path: e.GetPath(), Hash: h, Size: e.GetSize(), Executable: e.GetExecutable()})
	}
	sort.Slice(files, func(i, j int) bool { return files[i].Path < files[j].Path })
	normalizeSnapshot(s)
	writeJSON(w, http.StatusOK, snapshotDetail{Snapshot: *s, Files: files, RestrictedCount: restricted})
	return nil
}

// ---- read restrictions (docs/ENGINEERING_PLAN.md §13.1) ----

// Error codes of restricted reads.
const (
	codePathDenied        = "path_denied"        // 403: an access policy denies the path
	codeLicenseRestricted = "license_restricted" // 451: the tree's license rules forbid reading it
)

// readRestrictions records which paths of a request the caller may not
// read. The zero value (and nil) restricts nothing.
type readRestrictions struct {
	denied   map[string]string // path -> denying policy name
	licensed map[string]bool
}

// restricted reports whether path may not be read.
func (rs *readRestrictions) restricted(path string) bool {
	if rs == nil {
		return false
	}
	_, denied := rs.denied[path]
	return denied || rs.licensed[path]
}

// check returns the error for reading path's content: 403 path_denied for
// a policy deny, 451 license_restricted for a license restriction, or nil.
func (rs *readRestrictions) check(path string) error {
	if rs == nil {
		return nil
	}
	if policy, ok := rs.denied[path]; ok {
		msg := "reading " + path + " is denied by an access policy"
		if policy != "" {
			msg = "reading " + path + " is denied by policy " + policy
		}
		return &apiError{status: http.StatusForbidden, code: codePathDenied, msg: msg}
	}
	if rs.licensed[path] {
		return &apiError{status: http.StatusUnavailableForLegalReasons, code: codeLicenseRestricted,
			msg: path + " is restricted by the tree's license rules"}
	}
	return nil
}

// readRestrictions evaluates path-deny policies (tree:read) and license
// read rules for paths of tree. The caller must already hold tree:read.
func (a *api) readRestrictions(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) (*readRestrictions, error) {
	if len(paths) == 0 {
		return nil, nil
	}
	rs := &readRestrictions{}
	ev := a.d.Access
	if ev == nil {
		ev, _ = a.d.Authz.(domain.AccessEvaluator)
	}
	if ev != nil {
		denied, err := pathacl.DeniedPaths(ctx, a.d.Store, ev, p, tree, domain.ActTreeRead, paths)
		if err != nil {
			return nil, err
		}
		rs.denied = denied
	}
	if a.d.License != nil {
		licensed, err := a.licenseUnreadable(ctx, p, tree, paths)
		if err != nil {
			return nil, err
		}
		for _, path := range licensed {
			if rs.licensed == nil {
				rs.licensed = make(map[string]bool, len(licensed))
			}
			rs.licensed[path] = true
		}
	}
	return rs, nil
}

// licenseUnreadable lists the paths the license checker refuses to let p
// read, using its bulk UnreadablePaths API when available. The common
// all-readable case costs one check.
func (a *api) licenseUnreadable(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) ([]string, error) {
	type lister interface {
		UnreadablePaths(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) ([]string, error)
	}
	if l, ok := a.d.License.(lister); ok {
		return l.UnreadablePaths(ctx, p, tree, paths)
	}
	err := a.d.License.CheckRead(ctx, p, tree, paths)
	if err == nil {
		return nil, nil
	}
	if !isReadDenied(err) {
		return nil, err
	}
	var out []string
	for _, path := range paths {
		err := a.d.License.CheckRead(ctx, p, tree, []string{path})
		switch {
		case err == nil:
		case isReadDenied(err):
			out = append(out, path)
		default:
			return nil, err
		}
	}
	return out, nil
}

// activeContentTypes are rendered by browsers as documents or scripts; they
// are served as plain text so that tree content can never execute in the
// API's origin.
var activeContentTypes = []string{
	"text/html", "application/xhtml+xml", "image/svg+xml", "text/xml", "application/xml",
	"text/javascript", "application/javascript", "application/ecmascript", "text/ecmascript",
}

// blobContentType picks a Content-Type for a file: by extension, then by
// sniffing the bytes, downgrading active types to text/plain.
func blobContentType(name string, data []byte) string {
	ct := mime.TypeByExtension(path.Ext(name))
	if ct == "" {
		ct = http.DetectContentType(data)
	}
	mt, _, err := mime.ParseMediaType(ct)
	if err != nil {
		return "application/octet-stream"
	}
	for _, active := range activeContentTypes {
		if mt == active {
			return "text/plain; charset=utf-8"
		}
	}
	return ct
}

// handleBlob answers GET /api/v1/trees/{tenant}/{tree}/blob/{snapshot}/{path...}
// with the raw bytes of one file. Supports Range and conditional requests;
// the response is immutable for a given snapshot.
func (a *api) handleBlob(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	filePath := r.PathValue("path")
	if filePath == "" || strings.HasPrefix(filePath, "/") || path.Clean(filePath) != filePath {
		return badRequestf("invalid file path %q", filePath)
	}
	ctx := r.Context()
	s, err := a.treeSnapshot(ctx, tree, r.PathValue("snapshot"))
	if err != nil {
		return err
	}
	// Restricted paths are refused before the lookup, so that their
	// existence is not revealed either.
	rs, err := a.readRestrictions(ctx, principal(r), tree, []string{filePath})
	if err != nil {
		return err
	}
	if err := rs.check(filePath); err != nil {
		return err
	}
	m, err := a.loadManifest(ctx, s)
	if err != nil {
		return err
	}
	entries := m.GetEntries()
	i := sort.Search(len(entries), func(i int) bool { return entries[i].GetPath() >= filePath })
	var entry *syncpb.ManifestEntry
	if i < len(entries) && entries[i].GetPath() == filePath {
		entry = entries[i]
	} else {
		// Manifests are specified as sorted, but tolerate unsorted ones.
		for _, e := range entries {
			if e.GetPath() == filePath {
				entry = e
				break
			}
		}
	}
	if entry == nil {
		return notFoundf("file %q not found in snapshot %s", filePath, s.ID)
	}
	h, err := entryHash(entry)
	if err != nil {
		return err
	}
	data, err := content.ReadEntry(ctx, a.d.Objects, entry)
	if err != nil {
		// A referenced blob (or chunk) that is missing is corruption (500),
		// as above.
		return fmt.Errorf("content %s of %q: %v", h, filePath, err)
	}

	hdr := w.Header()
	hdr.Set("Content-Type", blobContentType(filePath, data))
	hdr.Set("X-Content-Type-Options", "nosniff")
	hdr.Set("Content-Security-Policy", "default-src 'none'; sandbox")
	hdr.Set("ETag", `"`+h.Hex()+`"`)
	hdr.Set("Cache-Control", "private, max-age=31536000, immutable")
	disposition := "inline"
	if r.URL.Query().Has("download") {
		disposition = "attachment"
	}
	if cd := mime.FormatMediaType(disposition, map[string]string{"filename": path.Base(filePath)}); cd != "" {
		hdr.Set("Content-Disposition", cd)
	}
	http.ServeContent(w, r, "", s.CreatedAt, bytes.NewReader(data))
	return nil
}
