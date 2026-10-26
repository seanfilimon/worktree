package backup

// Backup targets (docs/ENGINEERING_PLAN.md §16.11): where finished .wtbak
// archives are kept. DiskTarget is the v0.7 behaviour (<data>/backups);
// S3Target keeps them in a bucket under a key prefix (`--backup-target s3`,
// `--backup-s3-prefix`), reusing the server's S3 configuration. Archives are
// always written to a local staging file first (<data>/backups/<id>.wtbak.partial)
// and then moved into place or uploaded, so a failed upload leaves nothing
// half-written in the bucket.

import (
	"context"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/minio/minio-go/v7"

	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Target kinds.
const (
	TargetDisk = "disk"
	TargetS3   = "s3"
)

// TargetObject is one archive found in a target.
type TargetObject struct {
	ID       string
	Size     int64
	Modified time.Time
}

// Target stores finished backup archives.
type Target interface {
	// Kind is TargetDisk or TargetS3.
	Kind() string
	// Location describes the target ("/data/backups", "s3://bucket/prefix/").
	Location() string
	// Path is the display location of archive id.
	Path(id string) string
	// Store moves (disk) or uploads (S3) the finished local archive file
	// into the target as id. The local file is gone afterwards.
	Store(ctx context.Context, id, local string) error
	// List returns every archive in the target (any order).
	List(ctx context.Context) ([]TargetObject, error)
	// Open opens archive id for reading; ErrNotFound when missing. The
	// reader is an io.ReadSeeker for disk targets.
	Open(ctx context.Context, id string) (io.ReadCloser, TargetObject, error)
	// Delete removes archive id; ErrNotFound when missing.
	Delete(ctx context.Context, id string) error
}

// ---- disk ----------------------------------------------------------------

// DiskTarget keeps archives as <Dir>/<id>.wtbak.
type DiskTarget struct{ Dir string }

// NewDiskTarget returns a DiskTarget for dir.
func NewDiskTarget(dir string) *DiskTarget { return &DiskTarget{Dir: dir} }

func (d *DiskTarget) file(id string) string { return filepath.Join(d.Dir, id+FileExt) }

// Kind implements Target.
func (d *DiskTarget) Kind() string { return TargetDisk }

// Location implements Target.
func (d *DiskTarget) Location() string { return absPath(d.Dir) }

// Path implements Target.
func (d *DiskTarget) Path(id string) string { return absPath(d.file(id)) }

// Store implements Target.
func (d *DiskTarget) Store(_ context.Context, id, local string) error {
	if err := os.MkdirAll(d.Dir, 0o750); err != nil {
		return fmt.Errorf("backup: create backup directory: %w", err)
	}
	if err := os.Rename(local, d.file(id)); err != nil {
		return fmt.Errorf("backup: finish file: %w", err)
	}
	return nil
}

// List implements Target.
func (d *DiskTarget) List(_ context.Context) ([]TargetObject, error) {
	entries, err := os.ReadDir(d.Dir)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("backup: read backup directory: %w", err)
	}
	var out []TargetObject
	for _, e := range entries {
		id, ok := strings.CutSuffix(e.Name(), FileExt)
		if !ok || !ValidID(id) || !e.Type().IsRegular() {
			continue
		}
		fi, err := e.Info()
		if err != nil {
			continue
		}
		out = append(out, TargetObject{ID: id, Size: fi.Size(), Modified: fi.ModTime().UTC()})
	}
	return out, nil
}

// Open implements Target; the reader is an *os.File.
func (d *DiskTarget) Open(_ context.Context, id string) (io.ReadCloser, TargetObject, error) {
	if !ValidID(id) {
		return nil, TargetObject{}, ErrNotFound
	}
	f, err := os.Open(d.file(id))
	if errors.Is(err, fs.ErrNotExist) {
		return nil, TargetObject{}, ErrNotFound
	}
	if err != nil {
		return nil, TargetObject{}, fmt.Errorf("backup: open: %w", err)
	}
	fi, err := f.Stat()
	if err != nil || !fi.Mode().IsRegular() {
		f.Close()
		return nil, TargetObject{}, ErrNotFound
	}
	return f, TargetObject{ID: id, Size: fi.Size(), Modified: fi.ModTime().UTC()}, nil
}

// Delete implements Target.
func (d *DiskTarget) Delete(_ context.Context, id string) error {
	if !ValidID(id) {
		return ErrNotFound
	}
	err := os.Remove(d.file(id))
	if errors.Is(err, fs.ErrNotExist) {
		return ErrNotFound
	}
	if err != nil {
		return fmt.Errorf("backup: delete file: %w", err)
	}
	return nil
}

// ---- S3 ------------------------------------------------------------------

// s3PartSize is the multipart part size of archive uploads; smaller
// archives are uploaded with a single PUT.
const s3PartSize = 64 << 20

// S3Target keeps archives as <Prefix><id>.wtbak in a bucket.
type S3Target struct {
	b      *objstore.S3Bucket
	prefix string
}

// DefaultS3Prefix returns the default --backup-s3-prefix for an S3 object
// store prefix: "<s3-prefix>backups/".
func DefaultS3Prefix(objectsPrefix string) string {
	return objstore.NormalizeS3Prefix(objectsPrefix) + "backups/"
}

// NewS3Target returns a target on bucket b under prefix (normalized: no
// leading slash, a trailing one).
func NewS3Target(b *objstore.S3Bucket, prefix string) *S3Target {
	return &S3Target{b: b, prefix: objstore.NormalizeS3Prefix(prefix)}
}

// Prefix returns the normalized key prefix.
func (s *S3Target) Prefix() string { return s.prefix }

func (s *S3Target) key(id string) string { return s.prefix + id + FileExt }

// Kind implements Target.
func (s *S3Target) Kind() string { return TargetS3 }

// Location implements Target.
func (s *S3Target) Location() string { return "s3://" + s.b.Bucket + "/" + s.prefix }

// Path implements Target.
func (s *S3Target) Path(id string) string { return "s3://" + s.b.Bucket + "/" + s.key(id) }

// Store implements Target: uploads local (single PUT up to 64 MiB, else
// multipart) and removes it.
func (s *S3Target) Store(ctx context.Context, id, local string) error {
	f, err := os.Open(local)
	if err != nil {
		return fmt.Errorf("backup: open staged archive: %w", err)
	}
	fi, err := f.Stat()
	if err != nil {
		f.Close()
		return fmt.Errorf("backup: stat staged archive: %w", err)
	}
	_, err = s.b.Client.PutObject(ctx, s.b.Bucket, s.key(id), f, fi.Size(), minio.PutObjectOptions{
		ContentType:          "application/zstd",
		PartSize:             s3PartSize,
		SendContentMd5:       true,
		DisableContentSha256: true,
		UserMetadata:         map[string]string{"wt-backup-id": id},
	})
	f.Close()
	if err != nil {
		return fmt.Errorf("backup: upload %s to %s: %w", id, s.Location(), err)
	}
	if err := os.Remove(local); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return fmt.Errorf("backup: remove staged archive: %w", err)
	}
	return nil
}

// List implements Target.
func (s *S3Target) List(ctx context.Context) ([]TargetObject, error) {
	lctx, cancel := context.WithCancel(ctx)
	defer cancel()
	var out []TargetObject
	for obj := range s.b.Client.ListObjects(lctx, s.b.Bucket, minio.ListObjectsOptions{Prefix: s.prefix}) {
		if obj.Err != nil {
			return nil, fmt.Errorf("backup: list %s: %w", s.Location(), obj.Err)
		}
		id, ok := strings.CutSuffix(strings.TrimPrefix(obj.Key, s.prefix), FileExt)
		if !ok || !ValidID(id) {
			continue
		}
		out = append(out, TargetObject{ID: id, Size: obj.Size, Modified: obj.LastModified.UTC()})
	}
	return out, ctx.Err()
}

// Open implements Target.
func (s *S3Target) Open(ctx context.Context, id string) (io.ReadCloser, TargetObject, error) {
	if !ValidID(id) {
		return nil, TargetObject{}, ErrNotFound
	}
	info, err := s.b.Client.StatObject(ctx, s.b.Bucket, s.key(id), minio.StatObjectOptions{})
	if objstore.IsS3NotFound(err) {
		return nil, TargetObject{}, ErrNotFound
	}
	if err != nil {
		return nil, TargetObject{}, fmt.Errorf("backup: stat %s: %w", s.Path(id), err)
	}
	obj, err := s.b.Client.GetObject(ctx, s.b.Bucket, s.key(id), minio.GetObjectOptions{})
	if err != nil {
		return nil, TargetObject{}, fmt.Errorf("backup: open %s: %w", s.Path(id), err)
	}
	return obj, TargetObject{ID: id, Size: info.Size, Modified: info.LastModified.UTC()}, nil
}

// Delete implements Target.
func (s *S3Target) Delete(ctx context.Context, id string) error {
	if !ValidID(id) {
		return ErrNotFound
	}
	if _, err := s.b.Client.StatObject(ctx, s.b.Bucket, s.key(id), minio.StatObjectOptions{}); objstore.IsS3NotFound(err) {
		return ErrNotFound
	} else if err != nil {
		return fmt.Errorf("backup: stat %s: %w", s.Path(id), err)
	}
	if err := s.b.Client.RemoveObject(ctx, s.b.Bucket, s.key(id), minio.RemoveObjectOptions{}); err != nil {
		return fmt.Errorf("backup: delete %s: %w", s.Path(id), err)
	}
	return nil
}

// Compile-time checks.
var (
	_ Target = (*DiskTarget)(nil)
	_ Target = (*S3Target)(nil)
)
