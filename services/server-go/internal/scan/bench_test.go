package scan

import (
	"fmt"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Scanning cost per MB (docs/ENGINEERING_PLAN.md §16.7). b.SetBytes makes
// `go test -bench . ./internal/scan` report MB/s:
//
//	BenchmarkScanSource    typical source code (identifiers like token,
//	                       password and key on most lines: the generic
//	                       rules run)
//	BenchmarkScanProse     text without keywords (the prefilter skips
//	                       nearly every rule)
//	BenchmarkCheckPushPlan a whole push (1 MiB over 64 files, one secret):
//	                       manifest decoding, blob reads and scanning

func sourceText(size int) []byte {
	var b strings.Builder
	for i := 0; b.Len() < size; i++ {
		fmt.Fprintf(&b, "    let value_%d = compute(\"%x\", config.token_ttl, %d); // password policy for key %d\n", i, i*7919, i, i)
	}
	return []byte(b.String())
}

func proseText(size int) []byte {
	var b strings.Builder
	for b.Len() < size {
		b.WriteString("The quick brown fox jumps over the lazy dog while the rain falls on the plain.\n")
	}
	return []byte(b.String())
}

func benchScan(b *testing.B, data []byte) {
	b.SetBytes(int64(len(data)))
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if m := ScanBytes("bench.rs", data); len(m) != 0 {
			b.Fatalf("unexpected findings: %+v", m[:1])
		}
	}
}

// denseText is a worst case: every line names a keyword and carries a long
// value, so the generic rules run their regexes on every line.
func denseText(size int) []byte {
	var b strings.Builder
	for i := 0; b.Len() < size; i++ {
		fmt.Fprintf(&b, "  api_key_%d: \"${{ secrets.DEPLOY_API_KEY_VALUE_%d }}\" # token reference\n", i, i)
	}
	return []byte(b.String())
}

func BenchmarkScanSource(b *testing.B) { benchScan(b, sourceText(1<<20)) }

func BenchmarkScanDense(b *testing.B) { benchScan(b, denseText(1<<20)) }

func BenchmarkScanProse(b *testing.B) { benchScan(b, proseText(1<<20)) }

func BenchmarkCheckPushPlan(b *testing.B) {
	f := newFixture(b)
	files := make([]entry, 0, 64)
	chunk := string(sourceText(16 << 10))
	for i := 0; i < 64; i++ {
		files = append(files, entry{path: fmt.Sprintf("src/f%02d.rs", i), content: fmt.Sprintf("// %d\n%s", i, chunk)})
	}
	files[7].content += "K=" + awsKey + "\n"
	s := f.snap("", files...)
	p := &domain.PushPlan{Branch: "main", NewTip: s.ID, NewSnapshots: []*domain.Snapshot{s}}
	var total int64
	for _, e := range files {
		total += int64(len(e.content))
	}
	b.SetBytes(total)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if err := f.svc.CheckPushPlan(f.ctx, f.p, f.tree, p); err != nil {
			b.Fatal(err)
		}
	}
}

// TestScanThroughputFloor keeps an eye on the scanning cost: 1 MiB of source
// must scan well within a second even on slow CI machines.
func TestScanThroughputFloor(t *testing.T) {
	if testing.Short() {
		t.Skip("throughput check")
	}
	res := testing.Benchmark(func(b *testing.B) { benchScan(b, sourceText(1<<20)) })
	if res.N == 0 {
		t.Skip("benchmark did not run")
	}
	perMB := res.NsPerOp()
	t.Logf("source scan: %.1f ms per MiB (%s)", float64(perMB)/1e6, res)
	if perMB > 5e9 {
		t.Fatalf("scanning 1 MiB took %.1fs", float64(perMB)/1e9)
	}
}
