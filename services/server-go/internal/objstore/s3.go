package objstore

// S3-compatible object store (docs/ENGINEERING_PLAN.md §15.2,
// crates/worktree-protocol/specs/storage/Backends.md).
//
// Objects live at <prefix>objects/<2-hex>/<62-hex> in one bucket, holding
// the same stored encoding as DiskStore (raw or "WTZ1" + zstd frame), so a
// data directory's objects/ tree can be copied into a bucket verbatim and
// back. Keys sort in hash order, which ListObjectsV2 preserves, so List
// visits objects in ascending hash order like DiskStore.

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"

	"github.com/minio/minio-go/v7"
	"github.com/minio/minio-go/v7/pkg/credentials"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// S3Options configure an S3Store.
type S3Options struct {
	// Endpoint is host[:port] of the S3 API (e.g. "s3.amazonaws.com",
	// "minio:9000"). An http:// or https:// prefix is accepted and selects
	// the scheme (http:// implies Insecure).
	Endpoint string
	// Bucket holds the objects; it is created when missing.
	Bucket string
	// Prefix is prepended to every key ("wt/" stores objects under
	// wt/objects/...). A missing trailing slash is added.
	Prefix string
	// Region is the bucket region; empty lets the client discover it
	// (GetBucketLocation) and defaults to us-east-1.
	Region string
	// AccessKey and SecretKey are static credentials. When both are empty
	// the AWS_* / MINIO_* environment variables and the EC2/ECS instance
	// role are tried, in that order.
	AccessKey string
	SecretKey string
	// Insecure talks plain HTTP instead of HTTPS.
	Insecure bool
	// Compress stores new objects WTZ1-compressed when that saves at least
	// 10 % (same rule as DiskStore).
	Compress bool
	// Transport overrides the HTTP transport (tests); nil = default.
	Transport http.RoundTripper
	// MaxRetries bounds retries of failed requests (0 = client default).
	MaxRetries int
}

// S3Store is a [Store] backed by an S3-compatible bucket (AWS S3, MinIO,
// Ceph RGW, Cloudflare R2, ...). It implements StatStore, ListStore and
// SizeStore. It is safe for concurrent use.
type S3Store struct {
	client   *minio.Client
	bucket   string
	prefix   string
	endpoint string
	region   string
	secure   bool
	compress bool
}

// NewS3 connects to the bucket described by o, creating the bucket when it
// does not exist. It fails when the endpoint is unreachable or the
// credentials are rejected, so a misconfigured server does not start.
func NewS3(ctx context.Context, o S3Options) (*S3Store, error) {
	endpoint, secure, err := parseS3Endpoint(o.Endpoint, o.Insecure)
	if err != nil {
		return nil, err
	}
	if o.Bucket == "" {
		return nil, errors.New("objstore: s3: bucket is required")
	}
	var creds *credentials.Credentials
	if o.AccessKey != "" || o.SecretKey != "" {
		creds = credentials.NewStaticV4(o.AccessKey, o.SecretKey, "")
	} else {
		creds = credentials.NewChainCredentials([]credentials.Provider{
			&credentials.EnvAWS{},
			&credentials.EnvMinio{},
			&credentials.IAM{Client: &http.Client{Transport: http.DefaultTransport}},
		})
	}
	client, err := minio.New(endpoint, &minio.Options{
		Creds:      creds,
		Secure:     secure,
		Region:     o.Region,
		Transport:  o.Transport,
		MaxRetries: o.MaxRetries,
	})
	if err != nil {
		return nil, fmt.Errorf("objstore: s3: %w", err)
	}
	s := &S3Store{
		client:   client,
		bucket:   o.Bucket,
		prefix:   normalizePrefix(o.Prefix),
		endpoint: endpoint,
		region:   o.Region,
		secure:   secure,
		compress: o.Compress,
	}
	exists, err := client.BucketExists(ctx, o.Bucket)
	if err != nil {
		return nil, fmt.Errorf("objstore: s3: check bucket %q on %s: %w", o.Bucket, endpoint, err)
	}
	if !exists {
		if err := client.MakeBucket(ctx, o.Bucket, minio.MakeBucketOptions{Region: o.Region}); err != nil {
			// A concurrent creator (another replica) is fine.
			if again, aerr := client.BucketExists(ctx, o.Bucket); aerr != nil || !again {
				return nil, fmt.Errorf("objstore: s3: create bucket %q on %s: %w", o.Bucket, endpoint, err)
			}
		}
	}
	return s, nil
}

// parseS3Endpoint accepts "host[:port]" or a URL with an http/https
// scheme and returns host[:port] and whether to use TLS.
func parseS3Endpoint(raw string, insecure bool) (string, bool, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", false, errors.New("objstore: s3: endpoint is required")
	}
	if strings.Contains(raw, "://") {
		u, err := url.Parse(raw)
		if err != nil || u.Host == "" || (u.Path != "" && u.Path != "/") || u.RawQuery != "" {
			return "", false, fmt.Errorf("objstore: s3: endpoint %q: want host[:port] or scheme://host[:port]", raw)
		}
		switch u.Scheme {
		case "http":
			return u.Host, false, nil
		case "https":
			return u.Host, !insecure, nil
		default:
			return "", false, fmt.Errorf("objstore: s3: endpoint %q: scheme must be http or https", raw)
		}
	}
	if strings.ContainsAny(raw, "/?#") {
		return "", false, fmt.Errorf("objstore: s3: endpoint %q: want host[:port]", raw)
	}
	return raw, !insecure, nil
}

// normalizePrefix trims leading slashes and adds a trailing one.
func normalizePrefix(p string) string {
	p = strings.TrimLeft(strings.TrimSpace(p), "/")
	if p != "" && !strings.HasSuffix(p, "/") {
		p += "/"
	}
	return p
}

// Bucket returns the bucket name.
func (s *S3Store) Bucket() string { return s.bucket }

// Prefix returns the normalized key prefix ("" or ending in "/").
func (s *S3Store) Prefix() string { return s.prefix }

// Endpoint returns host[:port] of the S3 API.
func (s *S3Store) Endpoint() string { return s.endpoint }

// Region returns the configured region ("" = discovered).
func (s *S3Store) Region() string { return s.region }

// Secure reports whether the store talks HTTPS.
func (s *S3Store) Secure() bool { return s.secure }

// Compresses reports whether new objects are stored compressed.
func (s *S3Store) Compresses() bool { return s.compress }

// objectsPrefix returns <prefix>objects/.
func (s *S3Store) objectsPrefix() string { return s.prefix + "objects/" }

// key returns <prefix>objects/<2-hex>/<62-hex>.
func (s *S3Store) key(h hash.Hash) string {
	hex := h.Hex()
	return s.objectsPrefix() + hex[:2] + "/" + hex[2:]
}

// hashFromKey parses an object key; ok is false for foreign keys.
func (s *S3Store) hashFromKey(key string) (hash.Hash, bool) {
	rest, ok := strings.CutPrefix(key, s.objectsPrefix())
	if !ok {
		return hash.Hash{}, false
	}
	fan, name, ok := strings.Cut(rest, "/")
	if !ok || !isLowerHex(fan, 2) || !isLowerHex(name, hash.HexSize-2) {
		return hash.Hash{}, false
	}
	h, err := hash.FromHex(fan + name)
	return h, err == nil
}

// isNoSuchKey reports whether err is S3's "object does not exist".
func isNoSuchKey(err error) bool {
	return minio.ToErrorResponse(err).Code == minio.NoSuchKey
}

// ctxErr returns ctx's error when err was caused by its cancellation.
func ctxErr(ctx context.Context, err error) error {
	if cerr := ctx.Err(); cerr != nil && (errors.Is(err, cerr) || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded)) {
		return cerr
	}
	return nil
}

// Put implements Store.Put.
func (s *S3Store) Put(ctx context.Context, h hash.Hash, data []byte) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if hash.Sum(data) != h {
		return ErrHashMismatch
	}
	if exists, err := s.Has(ctx, h); err != nil {
		return err
	} else if exists {
		return nil
	}
	stored, err := EncodeObject(data, s.compress)
	if err != nil {
		return err
	}
	// Single PUT (objects are at most 64 MiB on the wire); a plain body
	// with a Content-MD5 works with every S3 implementation. Concurrent
	// Puts of the same object write identical bytes, so the race is benign.
	_, err = s.client.PutObject(ctx, s.bucket, s.key(h), bytes.NewReader(stored), int64(len(stored)), minio.PutObjectOptions{
		ContentType:          "application/octet-stream",
		DisableMultipart:     true,
		DisableContentSha256: true,
		SendContentMd5:       true,
	})
	if err != nil {
		if cerr := ctxErr(ctx, err); cerr != nil {
			return cerr
		}
		return fmt.Errorf("objstore: s3: put %s: %w", h, err)
	}
	return nil
}

// Get implements Store.Get.
func (s *S3Store) Get(ctx context.Context, h hash.Hash) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	obj, err := s.client.GetObject(ctx, s.bucket, s.key(h), minio.GetObjectOptions{})
	if err == nil {
		defer obj.Close()
		var data []byte
		data, err = io.ReadAll(io.LimitReader(obj, maxDecodedSize+1))
		if err == nil {
			if len(data) > maxDecodedSize {
				return nil, fmt.Errorf("%w: %s: stored object exceeds %d bytes", ErrCorrupt, h, maxDecodedSize)
			}
			return decodeStored(h, data)
		}
	}
	if isNoSuchKey(err) {
		return nil, ErrNotFound
	}
	if cerr := ctxErr(ctx, err); cerr != nil {
		return nil, cerr
	}
	return nil, fmt.Errorf("objstore: s3: get %s: %w", h, err)
}

// stat returns the object's info; ErrNotFound when it does not exist.
func (s *S3Store) stat(ctx context.Context, h hash.Hash) (minio.ObjectInfo, error) {
	if err := ctx.Err(); err != nil {
		return minio.ObjectInfo{}, err
	}
	info, err := s.client.StatObject(ctx, s.bucket, s.key(h), minio.StatObjectOptions{})
	if err == nil {
		return info, nil
	}
	if isNoSuchKey(err) {
		return minio.ObjectInfo{}, ErrNotFound
	}
	if cerr := ctxErr(ctx, err); cerr != nil {
		return minio.ObjectInfo{}, cerr
	}
	return minio.ObjectInfo{}, fmt.Errorf("objstore: s3: stat %s: %w", h, err)
}

// Has implements Store.Has. A missing bucket or rejected credentials are
// errors, not "absent".
func (s *S3Store) Has(ctx context.Context, h hash.Hash) (bool, error) {
	_, err := s.stat(ctx, h)
	if err == nil {
		return true, nil
	}
	if errors.Is(err, ErrNotFound) {
		return false, nil
	}
	return false, err
}

// Size implements SizeStore.Size: the stored (possibly compressed) size.
func (s *S3Store) Size(ctx context.Context, h hash.Hash) (int64, error) {
	info, err := s.stat(ctx, h)
	if err != nil {
		return 0, err
	}
	return info.Size, nil
}

// Delete implements Store.Delete. Deleting a missing object is a no-op
// (S3 DeleteObject is idempotent).
func (s *S3Store) Delete(ctx context.Context, h hash.Hash) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	err := s.client.RemoveObject(ctx, s.bucket, s.key(h), minio.RemoveObjectOptions{})
	if err == nil || isNoSuchKey(err) {
		return nil
	}
	if cerr := ctxErr(ctx, err); cerr != nil {
		return cerr
	}
	return fmt.Errorf("objstore: s3: delete %s: %w", h, err)
}

// Ping checks that the bucket is reachable and exists (health probe:
// S3 answers HEAD on a key of a missing bucket like a missing key, so Has
// alone cannot tell).
func (s *S3Store) Ping(ctx context.Context) error {
	ok, err := s.client.BucketExists(ctx, s.bucket)
	if err != nil {
		if cerr := ctxErr(ctx, err); cerr != nil {
			return cerr
		}
		return fmt.Errorf("objstore: s3: bucket %q: %w", s.bucket, err)
	}
	if !ok {
		return fmt.Errorf("objstore: s3: bucket %q does not exist", s.bucket)
	}
	return nil
}

// walk lists every object key under <prefix>objects/ in key (= hash)
// order, passing its hash and stored size to fn. Foreign keys are skipped.
func (s *S3Store) walk(ctx context.Context, fn func(h hash.Hash, size int64) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	lctx, cancel := context.WithCancel(ctx)
	defer cancel() // stops the lister goroutine when fn fails
	for obj := range s.client.ListObjects(lctx, s.bucket, minio.ListObjectsOptions{
		Prefix:    s.objectsPrefix(),
		Recursive: true,
	}) {
		if obj.Err != nil {
			if cerr := ctxErr(ctx, obj.Err); cerr != nil {
				return cerr
			}
			return fmt.Errorf("objstore: s3: list: %w", obj.Err)
		}
		h, ok := s.hashFromKey(obj.Key)
		if !ok {
			continue
		}
		if err := fn(h, obj.Size); err != nil {
			return err
		}
	}
	return ctx.Err()
}

// List implements [ListStore] (ascending hash order).
func (s *S3Store) List(ctx context.Context, fn func(hash.Hash) error) error {
	return s.walk(ctx, func(h hash.Hash, _ int64) error { return fn(h) })
}

// Stats implements [StatStore] by listing the bucket (stored sizes).
func (s *S3Store) Stats(ctx context.Context) (count, size int64, err error) {
	err = s.walk(ctx, func(_ hash.Hash, n int64) error {
		count++
		size += n
		return nil
	})
	if err != nil {
		return 0, 0, err
	}
	return count, size, nil
}

// Compile-time assertions that S3Store satisfies the store interfaces.
var (
	_ Store     = (*S3Store)(nil)
	_ StatStore = (*S3Store)(nil)
	_ ListStore = (*S3Store)(nil)
	_ SizeStore = (*S3Store)(nil)
)
