package objstore

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net/http/httptest"
	"os"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

const (
	fakeAccessKey = "AKIDFAKE"
	fakeSecretKey = "fake-secret"
)

// newFakeS3Store serves a FakeS3 over httptest and connects an S3Store to
// it (path-style, plain HTTP).
func newFakeS3Store(t *testing.T, prefix string, compress bool) (*S3Store, *FakeS3) {
	t.Helper()
	fake := NewFakeS3()
	fake.AccessKey = fakeAccessKey
	srv := httptest.NewServer(fake)
	t.Cleanup(srv.Close)
	s, err := NewS3(context.Background(), S3Options{
		Endpoint: srv.URL, Bucket: "wt-test", Prefix: prefix, Region: "us-east-1",
		AccessKey: fakeAccessKey, SecretKey: fakeSecretKey, Compress: compress, MaxRetries: 1,
	})
	if err != nil {
		t.Fatalf("NewS3: %v", err)
	}
	return s, fake
}

// storeUnderTest is what the conformance suite needs.
type storeUnderTest interface {
	Store
	StatStore
	ListStore
	SizeStore
}

// conformance runs the Store / StatStore / ListStore / SizeStore contract
// against s, which must start empty and compress new objects.
func conformance(t *testing.T, s storeUnderTest) {
	ctx := context.Background()

	t.Run("empty", func(t *testing.T) {
		n, b, err := s.Stats(ctx)
		if err != nil || n != 0 || b != 0 {
			t.Fatalf("Stats on empty store = %d, %d, %v", n, b, err)
		}
		if _, err := s.Get(ctx, hash.Sum([]byte("missing"))); !errors.Is(err, ErrNotFound) {
			t.Fatalf("Get missing = %v, want ErrNotFound", err)
		}
		if ok, err := s.Has(ctx, hash.Sum([]byte("missing"))); ok || err != nil {
			t.Fatalf("Has missing = %v, %v", ok, err)
		}
		if _, err := s.Size(ctx, hash.Sum([]byte("missing"))); !errors.Is(err, ErrNotFound) {
			t.Fatalf("Size missing = %v, want ErrNotFound", err)
		}
		if err := s.Delete(ctx, hash.Sum([]byte("missing"))); err != nil {
			t.Fatalf("Delete missing = %v, want nil", err)
		}
	})

	small := []byte("hello")
	text := bytes.Repeat([]byte("compressible line of text\n"), 200)
	random := noise(4096)
	magic := append([]byte("WTZ1"), []byte(" content that starts with the magic")...)
	empty := []byte{}
	all := [][]byte{small, text, random, magic, empty}

	t.Run("put get", func(t *testing.T) {
		for _, data := range all {
			h := hash.Sum(data)
			must(t, s.Put(ctx, h, data))
			must(t, s.Put(ctx, h, data)) // idempotent
			got, err := s.Get(ctx, h)
			if err != nil || !bytes.Equal(got, data) {
				t.Fatalf("Get(%d bytes) = %d bytes, %v", len(data), len(got), err)
			}
			if ok, err := s.Has(ctx, h); !ok || err != nil {
				t.Fatalf("Has = %v, %v", ok, err)
			}
		}
		if err := s.Put(ctx, hash.Sum([]byte("other")), small); !errors.Is(err, ErrHashMismatch) {
			t.Fatalf("Put with wrong hash = %v, want ErrHashMismatch", err)
		}
		if ok, _ := s.Has(ctx, hash.Sum([]byte("other"))); ok {
			t.Fatal("mismatched Put persisted bytes")
		}
	})

	t.Run("sizes are stored bytes", func(t *testing.T) {
		n, err := s.Size(ctx, hash.Sum(text))
		if err != nil {
			t.Fatal(err)
		}
		if n >= int64(len(text)) {
			t.Fatalf("compressible object stored as %d bytes (content %d)", n, len(text))
		}
		if n, _ := s.Size(ctx, hash.Sum(small)); n != int64(len(small)) {
			t.Fatalf("small object size = %d, want raw %d", n, len(small))
		}
		if n, _ := ObjectSize(ctx, s, hash.Sum(random)); n != int64(len(random)) {
			t.Fatalf("incompressible object size = %d, want %d", n, len(random))
		}
	})

	t.Run("list and stats", func(t *testing.T) {
		var want []hash.Hash
		var wantBytes int64
		for _, data := range all {
			h := hash.Sum(data)
			want = append(want, h)
			n, err := s.Size(ctx, h)
			must(t, err)
			wantBytes += n
		}
		slices.SortFunc(want, func(a, b hash.Hash) int { return bytes.Compare(a[:], b[:]) })
		var got []hash.Hash
		must(t, s.List(ctx, func(h hash.Hash) error { got = append(got, h); return nil }))
		if !slices.Equal(got, want) {
			t.Fatalf("List = %v\nwant %v (ascending)", got, want)
		}
		n, b, err := s.Stats(ctx)
		if err != nil || n != int64(len(all)) || b != wantBytes {
			t.Fatalf("Stats = %d, %d, %v; want %d, %d", n, b, err, len(all), wantBytes)
		}
		stop := errors.New("stop")
		calls := 0
		err = s.List(ctx, func(hash.Hash) error { calls++; return stop })
		if !errors.Is(err, stop) || calls != 1 {
			t.Fatalf("List stopping = %v after %d calls", err, calls)
		}
	})

	t.Run("delete", func(t *testing.T) {
		h := hash.Sum(small)
		must(t, s.Delete(ctx, h))
		if ok, _ := s.Has(ctx, h); ok {
			t.Fatal("deleted object still present")
		}
		if _, err := s.Get(ctx, h); !errors.Is(err, ErrNotFound) {
			t.Fatalf("Get deleted = %v", err)
		}
		must(t, s.Delete(ctx, h))
	})

	t.Run("canceled context", func(t *testing.T) {
		cctx, cancel := context.WithCancel(ctx)
		cancel()
		if err := s.Put(cctx, hash.Sum([]byte("x")), []byte("x")); !errors.Is(err, context.Canceled) {
			t.Fatalf("Put canceled = %v", err)
		}
		if _, err := s.Get(cctx, hash.Sum(text)); !errors.Is(err, context.Canceled) {
			t.Fatalf("Get canceled = %v", err)
		}
		if err := s.List(cctx, func(hash.Hash) error { return nil }); !errors.Is(err, context.Canceled) {
			t.Fatalf("List canceled = %v", err)
		}
	})

	t.Run("concurrent", func(t *testing.T) {
		var wg sync.WaitGroup
		errs := make(chan error, 64)
		for i := range 16 {
			wg.Add(1)
			go func() {
				defer wg.Done()
				data := []byte(fmt.Sprintf("concurrent object %d", i%4)) // shared hashes too
				h := hash.Sum(data)
				if err := s.Put(ctx, h, data); err != nil {
					errs <- err
					return
				}
				got, err := s.Get(ctx, h)
				if err != nil || !bytes.Equal(got, data) {
					errs <- fmt.Errorf("get %d: %v", i, err)
				}
			}()
		}
		wg.Wait()
		close(errs)
		for err := range errs {
			t.Error(err)
		}
	})
}

func TestDiskStoreConformance(t *testing.T) {
	conformance(t, New(t.TempDir()))
}

func TestS3StoreConformanceFake(t *testing.T) {
	s, fake := newFakeS3Store(t, "", true)
	fake.PageSize = 2 // every List pages through continuation tokens
	conformance(t, s)
}

func TestS3StoreConformanceFakeWithPrefix(t *testing.T) {
	s, fake := newFakeS3Store(t, "/team/wt", true)
	if s.Prefix() != "team/wt/" {
		t.Fatalf("Prefix = %q", s.Prefix())
	}
	conformance(t, s)
	for _, k := range fake.Keys("wt-test") {
		if !strings.HasPrefix(k, "team/wt/objects/") {
			t.Errorf("key %q outside the prefix", k)
		}
	}
}

// TestS3Layout checks the key layout and the WTZ1 encoding on the wire:
// the bucket holds exactly what DiskStore would write to disk.
func TestS3Layout(t *testing.T) {
	ctx := context.Background()
	s, fake := newFakeS3Store(t, "pfx", true)
	disk := New(t.TempDir())
	text := bytes.Repeat([]byte("the quick brown fox jumps over the lazy dog\n"), 100)
	h := hash.Sum(text)
	must(t, s.Put(ctx, h, text))
	must(t, disk.Put(ctx, h, text))

	key := "pfx/objects/" + h.Hex()[:2] + "/" + h.Hex()[2:]
	stored, ok := fake.Object("wt-test", key)
	if !ok {
		t.Fatalf("no object at %s; keys %v", key, fake.Keys("wt-test"))
	}
	if !bytes.HasPrefix(stored, CompressedMagic[:]) {
		t.Fatal("compressible object not stored with the WTZ1 magic")
	}
	onDisk, err := os.ReadFile(disk.objectPath(h))
	must(t, err)
	if !bytes.Equal(stored, onDisk) {
		t.Fatal("S3 and disk stored encodings differ")
	}

	// Raw objects written by another tool (or copied from a disk store
	// with compression off) read back too.
	raw := bytes.Repeat([]byte("raw"), 400)
	rh := hash.Sum(raw)
	fake.SetObject("wt-test", "pfx/objects/"+rh.Hex()[:2]+"/"+rh.Hex()[2:], raw)
	got, err := s.Get(ctx, rh)
	if err != nil || !bytes.Equal(got, raw) {
		t.Fatalf("Get raw = %v", err)
	}

	// A corrupted compressed object is reported, not returned.
	bad := append(slices.Clone(stored[:len(stored)-4]), 0, 1, 2, 3)
	fake.SetObject("wt-test", key, bad)
	if _, err := s.Get(ctx, h); !errors.Is(err, ErrCorrupt) {
		t.Fatalf("Get corrupt = %v, want ErrCorrupt", err)
	}

	// Foreign keys under the prefix are ignored by List.
	fake.SetObject("wt-test", "pfx/objects/zz/not-a-hash", []byte("x"))
	fake.SetObject("wt-test", "pfx/other/file", []byte("x"))
	n, _, err := s.Stats(ctx)
	if err != nil || n != 2 {
		t.Fatalf("Stats with foreign keys = %d, %v; want 2", n, err)
	}
}

func TestS3NoCompression(t *testing.T) {
	ctx := context.Background()
	s, fake := newFakeS3Store(t, "", false)
	text := bytes.Repeat([]byte("abc"), 1000)
	h := hash.Sum(text)
	must(t, s.Put(ctx, h, text))
	stored, _ := fake.Object("wt-test", "objects/"+h.Hex()[:2]+"/"+h.Hex()[2:])
	if !bytes.Equal(stored, text) {
		t.Fatal("with compression off the object must be stored raw")
	}
	if s.Compresses() {
		t.Fatal("Compresses = true")
	}
}

func TestS3PutSkipsExisting(t *testing.T) {
	ctx := context.Background()
	s, fake := newFakeS3Store(t, "", true)
	data := []byte("once")
	must(t, s.Put(ctx, hash.Sum(data), data))
	must(t, s.Put(ctx, hash.Sum(data), data))
	if fake.Puts() != 1 {
		t.Fatalf("PutObject requests = %d, want 1 (second Put is a no-op)", fake.Puts())
	}
}

func TestS3CreatesBucket(t *testing.T) {
	fake := NewFakeS3()
	srv := httptest.NewServer(fake)
	defer srv.Close()
	if _, err := NewS3(context.Background(), S3Options{Endpoint: srv.URL, Bucket: "fresh", AccessKey: "a", SecretKey: "b"}); err != nil {
		t.Fatalf("NewS3 without region: %v", err)
	}
	fake.mu.Lock()
	_, ok := fake.buckets["fresh"]
	fake.mu.Unlock()
	if !ok {
		t.Fatal("bucket was not created")
	}
}

func TestS3Errors(t *testing.T) {
	ctx := context.Background()

	t.Run("bad credentials fail at startup", func(t *testing.T) {
		fake := NewFakeS3()
		fake.AccessKey = "right"
		srv := httptest.NewServer(fake)
		defer srv.Close()
		_, err := NewS3(ctx, S3Options{Endpoint: srv.URL, Bucket: "bkt", Region: "us-east-1", AccessKey: "wrong", SecretKey: "x", MaxRetries: 1})
		if err == nil || !strings.Contains(err.Error(), "Access Denied") {
			t.Fatalf("NewS3 with wrong key = %v", err)
		}
	})

	t.Run("unreachable endpoint", func(t *testing.T) {
		srv := httptest.NewServer(NewFakeS3())
		url := srv.URL
		srv.Close()
		cctx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		if _, err := NewS3(cctx, S3Options{Endpoint: url, Bucket: "bkt", Region: "us-east-1", AccessKey: "a", SecretKey: "b", MaxRetries: 1}); err == nil {
			t.Fatal("NewS3 against a closed port succeeded")
		}
	})

	t.Run("server errors surface", func(t *testing.T) {
		s, fake := newFakeS3Store(t, "", true)
		data := []byte("payload")
		must(t, s.Put(ctx, hash.Sum(data), data))
		fake.FailNext(1)
		if _, err := s.Get(ctx, hash.Sum(data)); err == nil || errors.Is(err, ErrNotFound) {
			t.Fatalf("Get with a 500 = %v, want a non-not-found error", err)
		}
		fake.FailNext(1)
		if ok, err := s.Has(ctx, hash.Sum(data)); err == nil {
			t.Fatalf("Has with a 500 = %v, nil", ok)
		}
		// Recovered.
		if got, err := s.Get(ctx, hash.Sum(data)); err != nil || !bytes.Equal(got, data) {
			t.Fatalf("Get after recovery = %v", err)
		}
	})

	t.Run("ping reports a missing bucket", func(t *testing.T) {
		s, fake := newFakeS3Store(t, "", true)
		must(t, s.Ping(ctx))
		fake.mu.Lock()
		delete(fake.buckets, "wt-test")
		fake.mu.Unlock()
		if err := s.Ping(ctx); err == nil || !strings.Contains(err.Error(), "does not exist") {
			t.Fatalf("Ping on a deleted bucket = %v", err)
		}
		// Writes fail loudly.
		if err := s.Put(ctx, hash.Sum([]byte("x")), []byte("x")); err == nil {
			t.Fatal("Put into a deleted bucket succeeded")
		}
	})
}

func TestParseS3Endpoint(t *testing.T) {
	for _, tc := range []struct {
		in       string
		insecure bool
		host     string
		secure   bool
		ok       bool
	}{
		{"s3.amazonaws.com", false, "s3.amazonaws.com", true, true},
		{"minio:9000", true, "minio:9000", false, true},
		{"http://127.0.0.1:9000", false, "127.0.0.1:9000", false, true},
		{"https://r2.example.com/", false, "r2.example.com", true, true},
		{"ftp://x", false, "", false, false},
		{"host/path", false, "", false, false},
		{"https://h/bucket", false, "", false, false},
		{"", false, "", false, false},
	} {
		host, secure, err := parseS3Endpoint(tc.in, tc.insecure)
		if (err == nil) != tc.ok || host != tc.host || secure != tc.secure {
			t.Errorf("parseS3Endpoint(%q, %v) = %q, %v, %v", tc.in, tc.insecure, host, secure, err)
		}
	}
	if _, err := NewS3(context.Background(), S3Options{Endpoint: "x:1"}); err == nil {
		t.Error("NewS3 without a bucket succeeded")
	}
}

// TestS3StoreConformanceMinIO runs the suite against a real S3 / MinIO
// when WT_S3_TEST_ENDPOINT is set (e.g. http://127.0.0.1:9000). Optional:
// WT_S3_TEST_BUCKET (default wt-test), WT_S3_TEST_ACCESS_KEY and
// WT_S3_TEST_SECRET_KEY (default minioadmin), WT_S3_TEST_REGION.
func TestS3StoreConformanceMinIO(t *testing.T) {
	endpoint := os.Getenv("WT_S3_TEST_ENDPOINT")
	if endpoint == "" {
		t.Skip("set WT_S3_TEST_ENDPOINT to run against a real S3 / MinIO")
	}
	env := func(k, def string) string {
		if v := os.Getenv(k); v != "" {
			return v
		}
		return def
	}
	ctx := context.Background()
	prefix := fmt.Sprintf("wt-objstore-test-%d/", time.Now().UnixNano())
	s, err := NewS3(ctx, S3Options{
		Endpoint:  endpoint,
		Bucket:    env("WT_S3_TEST_BUCKET", "wt-test"),
		Prefix:    prefix,
		Region:    os.Getenv("WT_S3_TEST_REGION"),
		AccessKey: env("WT_S3_TEST_ACCESS_KEY", "minioadmin"),
		SecretKey: env("WT_S3_TEST_SECRET_KEY", "minioadmin"),
		Compress:  true,
	})
	if err != nil {
		t.Fatalf("NewS3(%s): %v", endpoint, err)
	}
	t.Cleanup(func() {
		var all []hash.Hash
		_ = s.List(ctx, func(h hash.Hash) error { all = append(all, h); return nil })
		for _, h := range all {
			_ = s.Delete(ctx, h)
		}
	})
	conformance(t, s)
}
