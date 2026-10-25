package backup

// The .wtbak archive format (crates/worktree-protocol/specs/storage/Backends.md):
// a zstd-compressed POSIX tar stream holding, in this order,
//
//	meta.db                     consistent bbolt snapshot (tx.WriteTo)
//	objects/<2-hex>/<62-hex>    every object's uncompressed content
//	manifest.json               Manifest (written last: it carries counts)
//
// Objects are stored as content (not their WTZ1 encoding) so the archive
// is independent of the store's compression setting; the outer zstd frame
// compresses them anyway. Restore checks every object against its name.

import (
	"archive/tar"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"time"

	"github.com/klauspost/compress/zstd"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Archive member names.
const (
	MemberMetaDB   = "meta.db"
	MemberManifest = "manifest.json"
	objectsDir     = "objects/"
)

// FormatName and FormatVersion identify the archive format in the
// manifest.
const (
	FormatName    = "wtbak"
	FormatVersion = 1
)

// FileExt is the backup file extension.
const FileExt = ".wtbak"

// maxObjectSize bounds one archived object (objects are at most 64 MiB on
// the wire; the margin covers locally built ones).
const maxObjectSize = 1 << 30

// Manifest describes an archive (manifest.json).
type Manifest struct {
	Format        string       `json:"format"`
	Version       int          `json:"version"`
	ID            string       `json:"id"`
	CreatedAt     time.Time    `json:"created_at"`
	CreatedBy     string       `json:"created_by"`
	Note          string       `json:"note,omitempty"`
	ServerVersion string       `json:"server_version,omitempty"`
	ObjectStore   string       `json:"object_store"`
	MetaDB        ManifestFile `json:"meta_db"`
	// Objects is the number of objects archived; ObjectBytes their total
	// (uncompressed) content size.
	Objects     int64 `json:"objects"`
	ObjectBytes int64 `json:"object_bytes"`
}

// ManifestFile describes one archived file.
type ManifestFile struct {
	Path   string `json:"path"`
	Size   int64  `json:"size"`
	SHA256 string `json:"sha256"`
}

// Snapshotter produces a consistent copy of the metadata database
// (implemented by *meta.Bolt).
type Snapshotter interface {
	Snapshot(ctx context.Context, fn func(size int64, db io.WriterTo) error) error
}

// objectSource is what writeArchive needs from the object store.
type objectSource interface {
	objstore.Store
	objstore.ListStore
}

// countingWriter counts bytes written through it.
type countingWriter struct {
	w io.Writer
	n int64
}

func (c *countingWriter) Write(p []byte) (int, error) {
	n, err := c.w.Write(p)
	c.n += int64(n)
	return n, err
}

// writeArchive streams a backup into w. m supplies the descriptive fields;
// the counts and the meta.db digest are filled in and returned.
func writeArchive(ctx context.Context, w io.Writer, meta Snapshotter, objs objectSource, m Manifest) (Manifest, error) {
	zw, err := zstd.NewWriter(w, zstd.WithEncoderLevel(zstd.SpeedDefault))
	if err != nil {
		return m, fmt.Errorf("backup: zstd: %w", err)
	}
	tw := tar.NewWriter(zw)
	mtime := m.CreatedAt.UTC().Truncate(time.Second)
	header := func(name string, size int64) *tar.Header {
		return &tar.Header{Name: name, Size: size, Mode: 0o644, ModTime: mtime, Typeflag: tar.TypeReg, Format: tar.FormatPAX}
	}

	// 1. The metadata snapshot, inside one read transaction.
	err = meta.Snapshot(ctx, func(size int64, db io.WriterTo) error {
		if err := tw.WriteHeader(header(MemberMetaDB, size)); err != nil {
			return err
		}
		sum := sha256.New()
		cw := &countingWriter{w: io.MultiWriter(tw, sum)}
		if _, err := db.WriteTo(cw); err != nil {
			return fmt.Errorf("write meta.db: %w", err)
		}
		if cw.n != size {
			return fmt.Errorf("meta.db snapshot wrote %d bytes, expected %d", cw.n, size)
		}
		m.MetaDB = ManifestFile{Path: MemberMetaDB, Size: size, SHA256: hex.EncodeToString(sum.Sum(nil))}
		return nil
	})
	if err != nil {
		return m, fmt.Errorf("backup: snapshot metadata: %w", err)
	}

	// 2. Every object. Objects pushed after the snapshot are harmless
	// extras; one deleted meanwhile (GC of an unreachable object) is
	// skipped.
	m.Objects, m.ObjectBytes = 0, 0
	err = objs.List(ctx, func(h hash.Hash) error {
		data, err := objs.Get(ctx, h)
		if errors.Is(err, objstore.ErrNotFound) {
			return nil
		}
		if err != nil {
			return err
		}
		if err := tw.WriteHeader(header(objectName(h), int64(len(data)))); err != nil {
			return err
		}
		if _, err := tw.Write(data); err != nil {
			return err
		}
		m.Objects++
		m.ObjectBytes += int64(len(data))
		return nil
	})
	if err != nil {
		return m, fmt.Errorf("backup: archive objects: %w", err)
	}

	// 3. The manifest.
	body, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return m, err
	}
	body = append(body, '\n')
	if err := tw.WriteHeader(header(MemberManifest, int64(len(body)))); err != nil {
		return m, fmt.Errorf("backup: write manifest: %w", err)
	}
	if _, err := tw.Write(body); err != nil {
		return m, fmt.Errorf("backup: write manifest: %w", err)
	}
	if err := tw.Close(); err != nil {
		return m, fmt.Errorf("backup: finish tar: %w", err)
	}
	if err := zw.Close(); err != nil {
		return m, fmt.Errorf("backup: finish zstd: %w", err)
	}
	return m, nil
}

// objectName returns objects/<2-hex>/<62-hex>.
func objectName(h hash.Hash) string {
	x := h.Hex()
	return objectsDir + x[:2] + "/" + x[2:]
}

// parseObjectName is the inverse of objectName.
func parseObjectName(name string) (hash.Hash, bool) {
	rest, ok := strings.CutPrefix(name, objectsDir)
	if !ok {
		return hash.Hash{}, false
	}
	fan, tail, ok := strings.Cut(rest, "/")
	if !ok || len(fan) != 2 || len(fan)+len(tail) != hash.HexSize || strings.ToLower(rest) != rest {
		return hash.Hash{}, false
	}
	h, err := hash.FromHex(fan + tail)
	return h, err == nil
}
