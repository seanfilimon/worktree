package backup

import (
	"archive/tar"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/klauspost/compress/zstd"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// ErrDataDirNotEmpty is returned by Restore when the target data directory
// holds files and Force is not set.
var ErrDataDirNotEmpty = errors.New("backup: data directory is not empty (use --force to restore over it)")

// ErrCorruptArchive is returned by Restore for an archive that is
// truncated, malformed or whose contents do not match their hashes.
var ErrCorruptArchive = errors.New("backup: corrupt archive")

// RestoreOptions configure Restore.
type RestoreOptions struct {
	// File is the .wtbak archive.
	File string
	// Source, when set, is read instead of File (server-ops §16.11: an
	// archive streamed from the backup target, e.g. an S3 bucket).
	Source io.Reader
	// DataDir receives meta.db.
	DataDir string
	// Objects receives every object (a DiskStore rooted at DataDir, or the
	// server's S3 store).
	Objects objstore.Store
	// Force allows a non-empty DataDir: meta.db is replaced and objects
	// are added next to the existing ones.
	Force bool
	// Progress, if set, is called after every restored object.
	Progress func(objects int64)
}

// RestoreReport summarizes a restore.
type RestoreReport struct {
	Manifest    Manifest `json:"manifest"`
	Objects     int64    `json:"objects"`
	ObjectBytes int64    `json:"object_bytes"`
	MetaDBPath  string   `json:"meta_db_path"`
}

// Restore unpacks the archive at o.File into o.DataDir and o.Objects. The
// server must be stopped. Every object is verified against its hash (and
// re-verified by the store's Put) and meta.db against the manifest's
// SHA-256 before the database is moved into place, so a failed restore
// leaves no meta.db behind (objects already written are harmless:
// content-addressed and collected by GC when unreachable).
func Restore(ctx context.Context, o RestoreOptions) (*RestoreReport, error) {
	if o.Objects == nil {
		return nil, errors.New("backup: restore: no object store")
	}
	if !o.Force {
		empty, err := dirEmpty(o.DataDir)
		if err != nil {
			return nil, err
		}
		if !empty {
			return nil, fmt.Errorf("%w: %s", ErrDataDirNotEmpty, o.DataDir)
		}
	}
	if err := os.MkdirAll(o.DataDir, 0o750); err != nil {
		return nil, fmt.Errorf("backup: create data directory: %w", err)
	}
	src := o.Source
	if src == nil {
		f, err := os.Open(o.File)
		if err != nil {
			return nil, fmt.Errorf("backup: open archive: %w", err)
		}
		defer f.Close()
		src = f
	}
	zr, err := zstd.NewReader(src)
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrCorruptArchive, err)
	}
	defer zr.Close()

	finalDB := filepath.Join(o.DataDir, "meta.db")
	tmpDB := finalDB + ".restoring"
	_ = os.Remove(tmpDB)
	moved := false
	defer func() {
		if !moved {
			_ = os.Remove(tmpDB)
		}
	}()

	rep := &RestoreReport{MetaDBPath: finalDB}
	var (
		manifest   *Manifest
		dbSize     int64 = -1
		dbSHA      string
		seenObject = map[hash.Hash]bool{}
	)
	tr := tar.NewReader(zr)
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		hdr, err := tr.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("%w: %v", ErrCorruptArchive, err)
		}
		if hdr.Typeflag != tar.TypeReg {
			continue
		}
		switch {
		case hdr.Name == MemberMetaDB:
			if dbSize >= 0 {
				return nil, fmt.Errorf("%w: duplicate %s", ErrCorruptArchive, MemberMetaDB)
			}
			dbSize, dbSHA, err = writeFileFrom(tmpDB, tr)
			if err != nil {
				return nil, err
			}
		case hdr.Name == MemberManifest:
			var m Manifest
			body, err := io.ReadAll(io.LimitReader(tr, 1<<20))
			if err != nil {
				return nil, fmt.Errorf("%w: read manifest: %v", ErrCorruptArchive, err)
			}
			if err := json.Unmarshal(body, &m); err != nil {
				return nil, fmt.Errorf("%w: manifest: %v", ErrCorruptArchive, err)
			}
			if m.Format != FormatName || m.Version < 1 {
				return nil, fmt.Errorf("%w: not a %s archive (format %q version %d)", ErrCorruptArchive, FormatName, m.Format, m.Version)
			}
			if m.Version > FormatVersion {
				return nil, fmt.Errorf("backup: archive format version %d is newer than this server supports (%d)", m.Version, FormatVersion)
			}
			manifest = &m
		default:
			h, ok := parseObjectName(hdr.Name)
			if !ok {
				continue // unknown member: ignored (forward compatible)
			}
			if hdr.Size < 0 || hdr.Size > maxObjectSize {
				return nil, fmt.Errorf("%w: object %s has size %d", ErrCorruptArchive, h, hdr.Size)
			}
			data := make([]byte, hdr.Size)
			if _, err := io.ReadFull(tr, data); err != nil {
				return nil, fmt.Errorf("%w: object %s: %v", ErrCorruptArchive, h, err)
			}
			if hash.Sum(data) != h {
				return nil, fmt.Errorf("%w: object %s does not match its hash", ErrCorruptArchive, h)
			}
			if err := o.Objects.Put(ctx, h, data); err != nil {
				return nil, fmt.Errorf("backup: restore object %s: %w", h, err)
			}
			if !seenObject[h] {
				seenObject[h] = true
				rep.Objects++
				rep.ObjectBytes += int64(len(data))
			}
			if o.Progress != nil {
				o.Progress(rep.Objects)
			}
		}
	}

	switch {
	case manifest == nil:
		return nil, fmt.Errorf("%w: %s is missing (truncated archive?)", ErrCorruptArchive, MemberManifest)
	case dbSize < 0:
		return nil, fmt.Errorf("%w: %s is missing", ErrCorruptArchive, MemberMetaDB)
	case dbSize != manifest.MetaDB.Size || dbSHA != manifest.MetaDB.SHA256:
		return nil, fmt.Errorf("%w: meta.db does not match the manifest (size %d / %d, sha256 %s / %s)",
			ErrCorruptArchive, dbSize, manifest.MetaDB.Size, dbSHA, manifest.MetaDB.SHA256)
	case rep.Objects != manifest.Objects:
		return nil, fmt.Errorf("%w: %d objects in the archive, the manifest lists %d", ErrCorruptArchive, rep.Objects, manifest.Objects)
	}
	rep.Manifest = *manifest

	// The database must open (schema check) before it replaces anything.
	db, err := meta.NewBolt(tmpDB)
	if err != nil {
		return nil, fmt.Errorf("backup: restored meta.db does not open: %w", err)
	}
	if err := db.Close(); err != nil {
		return nil, fmt.Errorf("backup: close restored meta.db: %w", err)
	}
	if err := os.Rename(tmpDB, finalDB); err != nil {
		return nil, fmt.Errorf("backup: move meta.db into place: %w", err)
	}
	moved = true
	return rep, nil
}

// writeFileFrom copies r into a new file at path and returns its size and
// SHA-256.
func writeFileFrom(path string, r io.Reader) (int64, string, error) {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return 0, "", fmt.Errorf("backup: create %s: %w", path, err)
	}
	sum := sha256.New()
	n, err := io.Copy(io.MultiWriter(f, sum), r)
	if err != nil {
		_ = f.Close()
		return 0, "", fmt.Errorf("%w: meta.db: %v", ErrCorruptArchive, err)
	}
	if err := f.Sync(); err != nil {
		_ = f.Close()
		return 0, "", fmt.Errorf("backup: sync %s: %w", path, err)
	}
	if err := f.Close(); err != nil {
		return 0, "", fmt.Errorf("backup: close %s: %w", path, err)
	}
	return n, hex.EncodeToString(sum.Sum(nil)), nil
}

// dirEmpty reports whether dir is missing or has no entries. A backups/
// directory is ignored (server-ops §16.11): restoring by id from the disk
// target reads <data>/backups/<id>.wtbak of an otherwise empty data
// directory.
func dirEmpty(dir string) (bool, error) {
	entries, err := os.ReadDir(dir)
	if errors.Is(err, fs.ErrNotExist) {
		return true, nil
	}
	if err != nil {
		return false, fmt.Errorf("backup: read data directory: %w", err)
	}
	for _, e := range entries {
		if e.IsDir() && e.Name() == "backups" {
			continue
		}
		return false, nil
	}
	return true, nil
}

// ReadManifest returns the manifest of the archive at path (it reads the
// whole stream: the manifest is the last member).
func ReadManifest(path string) (*Manifest, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	zr, err := zstd.NewReader(f)
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrCorruptArchive, err)
	}
	defer zr.Close()
	tr := tar.NewReader(zr)
	for {
		hdr, err := tr.Next()
		if errors.Is(err, io.EOF) {
			return nil, fmt.Errorf("%w: %s is missing", ErrCorruptArchive, MemberManifest)
		}
		if err != nil {
			return nil, fmt.Errorf("%w: %v", ErrCorruptArchive, err)
		}
		if hdr.Name != MemberManifest {
			continue
		}
		body, err := io.ReadAll(io.LimitReader(tr, 1<<20))
		if err != nil {
			return nil, fmt.Errorf("%w: %v", ErrCorruptArchive, err)
		}
		var m Manifest
		if err := json.NewDecoder(bytes.NewReader(body)).Decode(&m); err != nil {
			return nil, fmt.Errorf("%w: manifest: %v", ErrCorruptArchive, err)
		}
		return &m, nil
	}
}
