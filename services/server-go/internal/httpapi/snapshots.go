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

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
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
	files := make([]fileEntry, 0, len(m.GetEntries()))
	for _, e := range m.GetEntries() {
		h, err := entryHash(e)
		if err != nil {
			return err
		}
		files = append(files, fileEntry{Path: e.GetPath(), Hash: h, Size: e.GetSize(), Executable: e.GetExecutable()})
	}
	sort.Slice(files, func(i, j int) bool { return files[i].Path < files[j].Path })
	normalizeSnapshot(s)
	writeJSON(w, http.StatusOK, snapshotDetail{Snapshot: *s, Files: files})
	return nil
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
	if err := a.d.License.CheckRead(ctx, principal(r), tree, []string{filePath}); err != nil {
		return err
	}
	h, err := entryHash(entry)
	if err != nil {
		return err
	}
	data, err := a.d.Objects.Get(ctx, h)
	if err != nil {
		// A referenced blob that is missing is corruption (500), as above.
		return fmt.Errorf("blob %s of %q: %v", h, filePath, err)
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
