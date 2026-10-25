package objstore

// Tests of the WTZ1 object encoding (docs/ENGINEERING_PLAN.md §14.6).

import (
	"bytes"
	"context"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// vectorContent is shared with crates/worktree-sdk/src/store.rs (1296 bytes).
var vectorContent = []byte(strings.Repeat("W0rkTree WTZ1 compatibility vector: the quick brown fox jumps over the lazy dog.\n", 16))

const vectorHash = "bcde4961df8b572156be61dd0a297a62f829f582348332d65ddfa88ef34a3c18"

// rustVector is vectorContent as encoded by the Rust SDK (zstd crate,
// level 3, no frame checksum); the SDK's store tests decode goVector.
const rustVector = "57545a3128b52ffd601004c50200c4045730726b547265652057545a3120636f6d7061746962696c69747920766563746f723a2074686520717569636b2062726f776e20666f78206a756d7073206f7665726c617a7920646f672e0a0200bc50e1a320046506"

// goVector is vectorContent as encoded by EncodeObject when the vector was
// recorded (klauspost/compress v1.18.0, with frame checksum).
const goVector = "57545a3128b52ffd641004c50200c4045730726b547265652057545a3120636f6d7061746962696c69747920766563746f723a2074686520717569636b2062726f776e20666f78206a756d7073206f7665726c617a7920646f672e0a0200bc50e1a320046506b07d279f"

func mustHex(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func noise(n int) []byte {
	out := make([]byte, n)
	x := uint64(0x9e3779b97f4a7c15)
	for i := range out {
		x ^= x << 13
		x ^= x >> 7
		x ^= x << 17
		out[i] = byte(x >> 24)
	}
	return out
}

func TestRustEncodedVector(t *testing.T) {
	if hash.Sum(vectorContent).Hex() != vectorHash {
		t.Fatalf("vector hash = %s", hash.Sum(vectorContent).Hex())
	}
	for name, enc := range map[string]string{"rust": rustVector, "go": goVector} {
		stored := mustHex(t, enc)
		got, err := DecodeObject(stored)
		if err != nil || !bytes.Equal(got, vectorContent) {
			t.Fatalf("%s vector: %q, %v", name, got, err)
		}
		// Dropped into a store by hand, the file is served decoded.
		s := New(t.TempDir())
		h, _ := hash.FromHex(vectorHash)
		must(t, os.MkdirAll(s.fanoutDir(h), 0o755))
		must(t, os.WriteFile(s.objectPath(h), stored, 0o644))
		got, err = s.Get(context.Background(), h)
		if err != nil || !bytes.Equal(got, vectorContent) {
			t.Fatalf("%s vector via Get: %v", name, err)
		}
		if n, _ := s.Size(context.Background(), h); n != int64(len(stored)) {
			t.Fatalf("%s vector size = %d, want stored %d", name, n, len(stored))
		}
	}
	// The current encoder still produces a decodable WTZ1 file.
	enc, err := EncodeObject(vectorContent, true)
	if err != nil || !hasMagic(enc) {
		t.Fatalf("encode: %v", err)
	}
	if got, err := DecodeObject(enc); err != nil || !bytes.Equal(got, vectorContent) {
		t.Fatalf("round trip: %v", err)
	}
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func TestEncodeThresholds(t *testing.T) {
	small := bytes.Repeat([]byte("a"), CompressMinSize-1)
	if out, _ := EncodeObject(small, true); !bytes.Equal(out, small) {
		t.Fatal("objects below 512 bytes must stay raw")
	}
	at := bytes.Repeat([]byte("a"), CompressMinSize)
	if out, _ := EncodeObject(at, true); !hasMagic(out) {
		t.Fatal("a compressible 512-byte object must be compressed")
	}
	random := noise(4096)
	if out, _ := EncodeObject(random, true); !bytes.Equal(out, random) {
		t.Fatal("incompressible content must stay raw")
	}
	if out, _ := EncodeObject(at, false); !bytes.Equal(out, at) {
		t.Fatal("compression off must store raw")
	}
	// Content starting with the magic is always compressed, in both modes.
	tricky := []byte("WTZ1 looks like a header")
	for _, on := range []bool{true, false} {
		out, err := EncodeObject(tricky, on)
		if err != nil || !hasMagic(out) || len(out) <= len(tricky) {
			t.Fatalf("magic content (compress=%v): %q %v", on, out, err)
		}
		if got, _ := DecodeObject(out); !bytes.Equal(got, tricky) {
			t.Fatalf("magic content round trip: %q", got)
		}
	}
}

func TestCompressedStoreSemantics(t *testing.T) {
	ctx := context.Background()
	dir := t.TempDir()
	s := New(dir)
	if !s.Compresses() {
		t.Fatal("New must compress by default")
	}
	h := hash.Sum(vectorContent)
	must(t, s.Put(ctx, h, vectorContent))
	raw, err := os.ReadFile(s.objectPath(h))
	must(t, err)
	if !hasMagic(raw) || len(raw) >= len(vectorContent)/2 {
		t.Fatalf("stored %d bytes, magic=%v", len(raw), hasMagic(raw))
	}
	got, err := s.Get(ctx, h)
	if err != nil || !bytes.Equal(got, vectorContent) {
		t.Fatalf("Get: %v", err)
	}
	if ok, err := s.Has(ctx, h); !ok || err != nil {
		t.Fatalf("Has = %v, %v", ok, err)
	}
	n, err := s.Size(ctx, h)
	if err != nil || n != int64(len(raw)) {
		t.Fatalf("Size = %d, %v (stored %d)", n, err, len(raw))
	}
	if n, err := ObjectSize(ctx, s, h); err != nil || n != int64(len(raw)) {
		t.Fatalf("ObjectSize = %d, %v", n, err)
	}
	// Put of an existing object is a no-op, also with another setting.
	must(t, NewWithOptions(dir, Options{Compress: false}).Put(ctx, h, vectorContent))

	// Mixed store: a raw writer and a compressing writer share the root.
	plain := NewWithOptions(dir, Options{})
	other := append(append([]byte{}, vectorContent...), "one more\n"...)
	ho := hash.Sum(other)
	must(t, plain.Put(ctx, ho, other))
	if b, _ := os.ReadFile(plain.objectPath(ho)); !bytes.Equal(b, other) {
		t.Fatal("compression off must write raw files")
	}
	tiny := []byte("tiny")
	ht := hash.Sum(tiny)
	must(t, s.Put(ctx, ht, tiny))
	for _, r := range []*DiskStore{s, plain} {
		for want, hh := range map[string]hash.Hash{string(vectorContent): h, string(other): ho, "tiny": ht} {
			got, err := r.Get(ctx, hh)
			if err != nil || string(got) != want {
				t.Fatalf("mixed Get %s: %v", hh, err)
			}
		}
	}
	count, size, err := s.Stats(ctx)
	must(t, err)
	if count != 3 || size != int64(len(raw)+len(other)+len(tiny)) {
		t.Fatalf("Stats = %d objects, %d bytes", count, size)
	}
	var listed []hash.Hash
	must(t, s.List(ctx, func(x hash.Hash) error { listed = append(listed, x); return nil }))
	if len(listed) != 3 {
		t.Fatalf("List = %v", listed)
	}
}

func TestLegacyRawObjectWithMagicPrefix(t *testing.T) {
	ctx := context.Background()
	s := New(t.TempDir())
	content := []byte("WTZ1 but stored raw by an old server")
	h := hash.Sum(content)
	must(t, os.MkdirAll(s.fanoutDir(h), 0o755))
	must(t, os.WriteFile(s.objectPath(h), content, 0o644))
	got, err := s.Get(ctx, h)
	if err != nil || !bytes.Equal(got, content) {
		t.Fatalf("Get = %q, %v", got, err)
	}
}

func TestCorruptCompressedObject(t *testing.T) {
	ctx := context.Background()
	s := New(t.TempDir())
	h := hash.Sum(vectorContent)
	must(t, s.Put(ctx, h, vectorContent))
	p := s.objectPath(h)
	must(t, os.WriteFile(p, []byte("WTZ1garbage"), 0o644))
	if _, err := s.Get(ctx, h); !errors.Is(err, ErrCorrupt) {
		t.Fatalf("Get of a broken frame: %v, want ErrCorrupt", err)
	}
	// Has/Size/List still see the file (fsck reports it).
	if ok, _ := s.Has(ctx, h); !ok {
		t.Fatal("Has")
	}
	if _, err := os.Stat(filepath.Dir(p)); err != nil {
		t.Fatal(err)
	}
}

func TestConcurrentCompressedPuts(t *testing.T) {
	ctx := context.Background()
	s := New(t.TempDir())
	h := hash.Sum(vectorContent)
	errs := make(chan error, 16)
	for i := 0; i < 16; i++ {
		go func() {
			if err := s.Put(ctx, h, vectorContent); err != nil {
				errs <- err
				return
			}
			got, err := s.Get(ctx, h)
			if err == nil && !bytes.Equal(got, vectorContent) {
				err = errors.New("content differs")
			}
			errs <- err
		}()
	}
	for i := 0; i < 16; i++ {
		if err := <-errs; err != nil {
			t.Fatal(err)
		}
	}
}
