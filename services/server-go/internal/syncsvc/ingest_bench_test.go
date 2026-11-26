package syncsvc

// Benchmarks of pack ingestion (UploadPack's server side) and of the
// object encoding it relies on. Added with the v0.8 performance pass; they
// cover what push_bench_test.go cannot see: the pack verification scan,
// hashing and zstd re-encoding of every object, and the store writes.

import (
	"bytes"
	"context"
	"fmt"
	"math/rand"
	"os"
	"path/filepath"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// sourceLike returns n bytes of random words (compresses ~3x, like code).
func sourceLike(r *rand.Rand, n int) []byte {
	var b bytes.Buffer
	for b.Len() < n {
		fmt.Fprintf(&b, "w%04d ", r.Intn(5000))
		if r.Intn(12) == 0 {
			b.WriteByte('\n')
		}
	}
	return b.Bytes()[:n]
}

// writeBenchPack writes a zstd pack of `objects` whole blobs of `size`
// bytes and returns its path and the total object bytes.
func writeBenchPack(b *testing.B, objects, size int) (string, int64) {
	b.Helper()
	r := rand.New(rand.NewSource(1))
	path := filepath.Join(b.TempDir(), "bench.pack")
	f, err := os.Create(path)
	if err != nil {
		b.Fatal(err)
	}
	pw := newPackWriter(f, true)
	var total int64
	for i := 0; i < objects; i++ {
		data := sourceLike(r, size)
		if _, err := pw.add(hash.Sum(data), 1, data, nil, nil); err != nil {
			b.Fatal(err)
		}
		total += int64(len(data))
	}
	if _, err := pw.finish(); err != nil {
		b.Fatal(err)
	}
	if err := f.Close(); err != nil {
		b.Fatal(err)
	}
	return path, total
}

// BenchmarkIngestPack: 500 x 100 KiB blobs (the §7 "initial push" pack)
// into a fresh store per iteration.
func BenchmarkIngestPack(b *testing.B) {
	path, total := writeBenchPack(b, 500, 100<<10)
	run := func(b *testing.B, newStore func(b *testing.B) objstore.Store) {
		b.SetBytes(total)
		for i := 0; i < b.N; i++ {
			b.StopTimer()
			s := New(Deps{Objects: newStore(b)})
			b.StartTimer()
			resp, err := s.ingestPack(context.Background(), nil, nil, path)
			if err != nil {
				b.Fatal(err)
			}
			if resp.ObjectsStored != 500 {
				b.Fatalf("stored %d objects", resp.ObjectsStored)
			}
		}
	}
	b.Run("store=memory", func(b *testing.B) {
		run(b, func(*testing.B) objstore.Store { return newMemObjects() })
	})
	b.Run("store=disk", func(b *testing.B) {
		run(b, func(b *testing.B) objstore.Store { return objstore.New(b.TempDir()) })
	})
}

// BenchmarkEncodeObjectParallel: WTZ1 encoding of 100 KiB objects from
// all CPUs at once (upload streams, ingest workers and downloads share
// the process-wide encoder).
func BenchmarkEncodeObjectParallel(b *testing.B) {
	data := sourceLike(rand.New(rand.NewSource(2)), 100<<10)
	b.SetBytes(int64(len(data)))
	b.RunParallel(func(pb *testing.PB) {
		for pb.Next() {
			if _, err := objstore.EncodeObject(data, true); err != nil {
				b.Fatal(err)
			}
		}
	})
}
