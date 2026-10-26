package codesearch

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// On-disk layout (docs/ENGINEERING_PLAN.md §15.9):
//
//	<dir>/<tree id>/state.json        indexed branches of the tree
//	<dir>/<tree id>/<manifest>.seg    one segment per indexed manifest
//
// Segments are immutable and content-addressed by manifest hash; a new
// one is written to a temporary file and renamed into place. state.json
// is replaced atomically (temporary file + rename) after the segments it
// names exist, and segments no branch refers to any more are deleted
// afterwards, so a crash at any point leaves a consistent index (at worst
// with an unreferenced segment that the next write of the tree removes).

const stateFile = "state.json"

// diskState is the content of state.json.
type diskState struct {
	Format        int          `json:"format"`
	TreeID        string       `json:"tree_id"`
	DefaultBranch string       `json:"default_branch"`
	Branches      []diskBranch `json:"branches"`
}

// diskBranch is one indexed branch: its tip and the Meta of its segment
// (Meta.Manifest names the segment; "" for an empty branch).
type diskBranch struct {
	Branch     string    `json:"branch"`
	SnapshotID string    `json:"snapshot_id"`
	Meta       Meta      `json:"meta"`
	LastUsed   time.Time `json:"last_used"`
}

// treeDirName maps a tree id to a directory name; ids with unusual
// characters are hex-encoded.
func treeDirName(id string) string {
	ok := id != "" && id != "." && id != ".." && len(id) <= 128
	for _, c := range id {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_' || c == '.') {
			ok = false
			break
		}
	}
	if ok {
		return id
	}
	return "x-" + hex.EncodeToString([]byte(id))
}

// treeDir returns the index directory of a tree ("" without persistence).
func (ix *Index) treeDir(treeID string) string {
	if ix.opt.Dir == "" {
		return ""
	}
	return filepath.Join(ix.opt.Dir, treeDirName(treeID))
}

func segFileName(m hash.Hash) string { return m.Hex() + ".seg" }

// readState reads a tree's state.json; (nil, nil) when there is none.
func readState(dir string) (*diskState, error) {
	raw, err := os.ReadFile(filepath.Join(dir, stateFile))
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var st diskState
	if err := json.Unmarshal(raw, &st); err != nil {
		return nil, fmt.Errorf("%w: %s: %v", errSegCorrupt, stateFile, err)
	}
	if st.Format != FormatVersion {
		return nil, fmt.Errorf("%w: %s has format %d, want %d", errSegFormat, stateFile, st.Format, FormatVersion)
	}
	return &st, nil
}

// writeFileAtomic writes data to path through a temporary file in the same
// directory, synced before it is renamed over path.
func writeFileAtomic(path string, data []byte) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	var rnd [6]byte
	_, _ = rand.Read(rnd[:])
	tmp := path + ".tmp-" + hex.EncodeToString(rnd[:])
	f, err := os.OpenFile(tmp, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o644)
	if err != nil {
		return err
	}
	_, werr := f.Write(data)
	if werr == nil {
		werr = f.Sync()
	}
	if cerr := f.Close(); werr == nil {
		werr = cerr
	}
	if werr == nil {
		werr = os.Rename(tmp, path)
	}
	if werr != nil {
		_ = os.Remove(tmp)
		return werr
	}
	syncDir(dir)
	return nil
}

// syncDir makes a rename durable where the platform supports it.
func syncDir(dir string) {
	d, err := os.Open(dir)
	if err != nil {
		return
	}
	_ = d.Sync() // fails harmlessly on Windows
	_ = d.Close()
}

// loadSegmentFile reads and decodes a segment from disk. A missing file is
// fs.ErrNotExist; a format mismatch or corruption is errSegFormat /
// errSegCorrupt (the file is removed so that the branch is rebuilt).
func (ix *Index) loadSegmentFile(treeID string, m hash.Hash) (*segment, error) {
	dir := ix.treeDir(treeID)
	if dir == "" {
		return nil, fs.ErrNotExist
	}
	path := filepath.Join(dir, segFileName(m))
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	seg, err := decodeSegment(m, data)
	if err != nil {
		ix.d.Logger.Warn("codesearch: discarding unusable index segment", "tree", treeID, "file", path, "err", err)
		_ = os.Remove(path)
		return nil, err
	}
	ix.diskLoads.Add(1)
	return seg, nil
}

// writeSegmentFile stores an encoded segment (no-op without persistence).
func (ix *Index) writeSegmentFile(treeID string, m hash.Hash, data []byte) error {
	dir := ix.treeDir(treeID)
	if dir == "" {
		return nil
	}
	return writeFileAtomic(filepath.Join(dir, segFileName(m)), data)
}

// dirUsage returns the bytes of a tree's index files on disk.
func dirUsage(dir string) int64 {
	if dir == "" {
		return 0
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return 0
	}
	var n int64
	for _, e := range entries {
		if info, err := e.Info(); err == nil && !e.IsDir() {
			n += info.Size()
		}
	}
	return n
}

// gcDir deletes the segments of dir that keep does not name, and stale
// temporary files.
func gcDir(dir string, keep map[string]bool) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return
	}
	for _, e := range entries {
		name := e.Name()
		if e.IsDir() || name == stateFile {
			continue
		}
		if i := strings.Index(name, ".tmp-"); i >= 0 {
			// A temporary file of an in-flight write is named after its
			// target; only abandoned ones (target not kept, or old) go.
			if info, err := e.Info(); err == nil && (!keep[name[:i]] || time.Since(info.ModTime()) > time.Hour) {
				_ = os.Remove(filepath.Join(dir, name))
			}
			continue
		}
		if strings.HasSuffix(name, ".seg") && !keep[name] {
			_ = os.Remove(filepath.Join(dir, name)) // may fail while a reader has it open (Windows); retried next time
		}
	}
}

// listTreeDirs returns the tree ids that have an index directory (from
// the directory names; the files are not read).
func (ix *Index) listTreeDirs() []string {
	if ix.opt.Dir == "" {
		return nil
	}
	entries, err := os.ReadDir(ix.opt.Dir)
	if err != nil {
		return nil
	}
	var out []string
	for _, e := range entries {
		if !e.IsDir() {
			continue
		}
		if id, ok := treeIDFromDirName(e.Name()); ok {
			out = append(out, id)
		}
	}
	return out
}

// treeIDFromDirName inverts treeDirName.
func treeIDFromDirName(name string) (string, bool) {
	if rest, ok := strings.CutPrefix(name, "x-"); ok {
		raw, err := hex.DecodeString(rest)
		if err == nil && treeDirName(string(raw)) == name {
			return string(raw), true
		}
	}
	return name, treeDirName(name) == name
}

// removeAllRetry removes a directory tree, retrying briefly: on Windows a
// file another goroutine is reading cannot be deleted.
func removeAllRetry(dir string) error {
	var err error
	for i := 0; i < 20; i++ {
		if err = os.RemoveAll(dir); err == nil {
			return nil
		}
		time.Sleep(time.Duration(i+1) * 5 * time.Millisecond)
	}
	return err
}
