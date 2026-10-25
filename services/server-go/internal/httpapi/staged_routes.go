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
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// registerStagedRoutes registers the v0.5 staged-snapshot routes
// (DELETE {tree}/staged, docs/ENGINEERING_PLAN.md §13.11).
func (a *api) registerStagedRoutes(tree string) {
	a.handle("DELETE "+tree+"/staged", a.handleUnstage)
	// v0.6 (§14.9): browse a staged entry's working manifest.
	a.handle("GET "+tree+"/staged/{snapshot}/files", a.handleStagedFiles)
	a.handle("GET "+tree+"/staged/{snapshot}/files/{path...}", a.handleStagedFile)
}

// stagedFilesResponse is the body of GET {tree}/staged/{snapshot}/files.
type stagedFilesResponse struct {
	Staged domain.StagedSnapshot `json:"staged"`
	Files  []fileEntry           `json:"files"`
	// RestrictedCount is the number of files left out of Files because the
	// caller may not read them (path policies or license rules, §13.1).
	RestrictedCount int `json:"restricted_count"`
}

// stagedEntry finds the live staged entry with snapshot id in tree (any
// author, any branch). The staged service checks tree:read.
func (a *api) stagedEntry(r *http.Request, tree *domain.Tree, id string) (*domain.StagedSnapshot, error) {
	if a.d.Staged == nil {
		return nil, &apiError{status: http.StatusNotImplemented, code: "not_implemented", msg: "staged snapshots are not enabled on this server"}
	}
	all, err := a.d.Staged.List(r.Context(), principal(r), tree, "")
	if err != nil {
		return nil, err
	}
	for i := range all {
		if all[i].SnapshotID == id {
			return &all[i], nil
		}
	}
	return nil, notFoundf("staged snapshot %q not found", id)
}

// stagedManifest loads the working manifest of a staged entry. Entries
// staged without a manifest (or whose manifest objects never reached the
// server) are 404: there is nothing to browse.
func (a *api) stagedManifest(ctx context.Context, st *domain.StagedSnapshot) (*syncpb.ManifestRecord, error) {
	if st.ManifestHash == "" {
		return nil, notFoundf("staged snapshot %s has no manifest", st.SnapshotID)
	}
	h, err := hash.FromHex(st.ManifestHash)
	if err != nil {
		return nil, fmt.Errorf("staged snapshot %s: manifest hash %q: %v", st.SnapshotID, st.ManifestHash, err)
	}
	raw, err := a.d.Objects.Get(ctx, h)
	if errors.Is(err, objstore.ErrNotFound) || errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("the manifest of staged snapshot %s is not stored on the server", st.SnapshotID)
	}
	if err != nil {
		return nil, fmt.Errorf("staged snapshot %s: load manifest %s: %w", st.SnapshotID, h, err)
	}
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("staged snapshot %s: decode manifest %s: %w", st.SnapshotID, h, err)
	}
	return &m, nil
}

// handleStagedFiles answers GET /api/v1/trees/{tenant}/{tree}/staged/{snapshot}/files
// with the files of a staged entry's working manifest (restricted paths
// left out and counted, like the snapshot detail).
func (a *api) handleStagedFiles(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	ctx := r.Context()
	st, err := a.stagedEntry(r, tree, r.PathValue("snapshot"))
	if err != nil {
		return err
	}
	m, err := a.stagedManifest(ctx, st)
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
	files := make([]fileEntry, 0, len(paths))
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
	st.FilesChanged = nonNil(st.FilesChanged)
	writeJSON(w, http.StatusOK, stagedFilesResponse{Staged: *st, Files: files, RestrictedCount: restricted})
	return nil
}

// handleStagedFile answers GET /api/v1/trees/{tenant}/{tree}/staged/{snapshot}/files/{path...}
// with the raw bytes of one staged file, with the read checks of the blob
// endpoint (path deny 403, license 451, checked before the lookup).
func (a *api) handleStagedFile(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	filePath := r.PathValue("path")
	if filePath == "" || strings.HasPrefix(filePath, "/") || path.Clean(filePath) != filePath {
		return badRequestf("invalid file path %q", filePath)
	}
	ctx := r.Context()
	st, err := a.stagedEntry(r, tree, r.PathValue("snapshot"))
	if err != nil {
		return err
	}
	rs, err := a.readRestrictions(ctx, principal(r), tree, []string{filePath})
	if err != nil {
		return err
	}
	if err := rs.check(filePath); err != nil {
		return err
	}
	m, err := a.stagedManifest(ctx, st)
	if err != nil {
		return err
	}
	var entry *syncpb.ManifestEntry
	for _, e := range m.GetEntries() {
		if e.GetPath() == filePath {
			entry = e
			break
		}
	}
	if entry == nil {
		return notFoundf("file %q not found in staged snapshot %s", filePath, st.SnapshotID)
	}
	h, err := entryHash(entry)
	if err != nil {
		return err
	}
	data, err := content.ReadEntry(ctx, a.d.Objects, entry)
	if errors.Is(err, objstore.ErrNotFound) || errors.Is(err, domain.ErrNotFound) {
		// Staged content is uploaded best effort by the client: a missing
		// blob is "not available", not server corruption.
		return notFoundf("the content of %q in staged snapshot %s is not stored on the server", filePath, st.SnapshotID)
	}
	if err != nil {
		return fmt.Errorf("staged content %s of %q: %w", h, filePath, err)
	}
	hdr := w.Header()
	hdr.Set("Content-Type", blobContentType(filePath, data))
	hdr.Set("X-Content-Type-Options", "nosniff")
	hdr.Set("Content-Security-Policy", "default-src 'none'; sandbox")
	hdr.Set("ETag", `"`+h.Hex()+`"`)
	hdr.Set("Cache-Control", "private, no-cache")
	disposition := "inline"
	if r.URL.Query().Has("download") {
		disposition = "attachment"
	}
	if cd := mime.FormatMediaType(disposition, map[string]string{"filename": path.Base(filePath)}); cd != "" {
		hdr.Set("Content-Disposition", cd)
	}
	http.ServeContent(w, r, "", st.StagedAt, bytes.NewReader(data))
	return nil
}

// unstageResponse is the body of DELETE {tree}/staged.
type unstageResponse struct {
	Removed []domain.StagedSnapshot `json:"removed"`
}

// handleUnstage answers DELETE /api/v1/trees/{tenant}/{tree}/staged?branch=&account=.
//
// It removes the caller's staged snapshot on branch (every branch when
// branch is omitted). account= (a username or account id) removes another
// account's entries and requires tree:admin. The removed entries are
// returned; removing nothing is not an error.
func (a *api) handleUnstage(w http.ResponseWriter, r *http.Request) error {
	u, ok := a.d.Staged.(staged.Unstager)
	if a.d.Staged == nil || !ok {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented", msg: "unstaging is not enabled on this server"}
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	q := r.URL.Query()
	branch := q.Get("branch")
	if branch != "" {
		if err := validateBranchName(branch); err != nil {
			return err
		}
	}
	accountID := ""
	if ref := q.Get("account"); ref != "" {
		accountID, err = a.resolveAccountRef(r, ref)
		if err != nil {
			return err
		}
	}
	removed, err := u.Unstage(r.Context(), p, tree, accountID, branch)
	if err != nil {
		return err
	}
	for _, s := range removed {
		a.audit(r, "staged.unstage", t.ID, "staged", tree.ID+"/"+s.BranchName, map[string]string{
			"tree":     tree.Name,
			"branch":   s.BranchName,
			"author":   s.Author,
			"snapshot": s.SnapshotID,
		})
	}
	for i := range removed {
		removed[i].FilesChanged = nonNil(removed[i].FilesChanged)
	}
	writeJSON(w, http.StatusOK, unstageResponse{Removed: nonNil(removed)})
	return nil
}

// resolveAccountRef maps a username (or, failing that, an account id) to
// an account id.
func (a *api) resolveAccountRef(r *http.Request, ref string) (string, error) {
	ctx := r.Context()
	acct, err := a.d.Store.GetAccountByUsername(ctx, ref)
	if err == nil {
		return acct.ID, nil
	}
	if !errors.Is(err, domain.ErrNotFound) {
		return "", err
	}
	if acct, err := a.d.Store.GetAccount(ctx, ref); err == nil {
		return acct.ID, nil
	} else if !errors.Is(err, domain.ErrNotFound) {
		return "", err
	}
	return "", notFoundf("account %q not found", ref)
}
