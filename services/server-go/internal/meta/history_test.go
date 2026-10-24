package meta

import (
	"context"
	"errors"
	"fmt"
	"math/rand/v2"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// buildDAG stores one snapshot per spec entry "id:parent1,parent2" (or
// just "id" for a root). The i-th entry is created at at(i) so timestamps
// increase in spec order.
func buildDAG(t testing.TB, s domain.MetaStore, specs ...string) {
	t.Helper()
	ctx := context.Background()
	for i, spec := range specs {
		id, ps, _ := strings.Cut(spec, ":")
		parents := []string{}
		if ps != "" {
			parents = strings.Split(ps, ",")
		}
		if err := s.PutSnapshot(ctx, &domain.Snapshot{ID: id, TreeID: "tr", Parents: parents, CreatedAt: at(i)}); err != nil {
			t.Fatalf("PutSnapshot(%s): %v", spec, err)
		}
	}
}

func snapIDs(ss []domain.Snapshot) []string {
	out := []string{}
	for _, s := range ss {
		out = append(out, s.ID)
	}
	return out
}

// checkTopo asserts every snapshot appears after all of its parents that
// are also in the list.
func checkTopo(t *testing.T, ss []domain.Snapshot) {
	t.Helper()
	pos := map[string]int{}
	for i, s := range ss {
		if _, dup := pos[s.ID]; dup {
			t.Fatalf("duplicate %s in result", s.ID)
		}
		pos[s.ID] = i
	}
	for i, s := range ss {
		for _, p := range s.Parents {
			if j, ok := pos[p]; ok && j > i {
				t.Fatalf("%s (pos %d) listed before its parent %s (pos %d)", s.ID, i, p, j)
			}
		}
	}
}

// hideViewer wraps a store so that it does not implement SnapshotViewer,
// exercising History's per-call GetSnapshot path.
type hideViewer struct{ domain.MetaStore }

func runHistoryConformance(t *testing.T, newStore storeFactory) {
	variants := map[string]func(domain.MetaStore) domain.History{
		"viewer":   NewHistory,
		"noViewer": func(s domain.MetaStore) domain.History { return NewHistory(hideViewer{s}) },
	}
	for vname, mk := range variants {
		t.Run(vname, func(t *testing.T) {
			t.Run("Linear", func(t *testing.T) { testHistoryLinear(t, newStore(t), mk) })
			t.Run("Merge", func(t *testing.T) { testHistoryMerge(t, newStore(t), mk) })
			t.Run("CrissCross", func(t *testing.T) { testHistoryCrissCross(t, newStore(t), mk) })
			t.Run("SkewedClocks", func(t *testing.T) { testHistorySkew(t, newStore(t), mk) })
			t.Run("Dangling", func(t *testing.T) { testHistoryDangling(t, newStore(t), mk) })
			t.Run("RandomDAG", func(t *testing.T) { testHistoryRandom(t, newStore(t), mk) })
		})
	}
	t.Run("Chain10k", func(t *testing.T) { testHistoryChainPerf(t, newStore(t), 10_000) })
	t.Run("WideMerges", func(t *testing.T) { testHistoryWide(t, newStore(t)) })
	t.Run("Canceled", func(t *testing.T) {
		s := newStore(t)
		buildDAG(t, s, "a")
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		h := NewHistory(s)
		_, err := h.Log(ctx, "a", 0)
		wantErr(t, err, context.Canceled)
		_, err = h.Missing(ctx, "a", nil)
		wantErr(t, err, context.Canceled)
		_, err = h.IsAncestor(ctx, "a", "a")
		wantErr(t, err, context.Canceled)
	})
}

func testHistoryLinear(t *testing.T, s domain.MetaStore, mk func(domain.MetaStore) domain.History) {
	ctx := context.Background()
	buildDAG(t, s, "a", "b:a", "c:b", "d:c")
	h := mk(s)

	anc := []struct {
		a, d string
		want bool
	}{
		{"a", "d", true}, {"b", "d", true}, {"d", "d", true}, {"a", "a", true},
		{"d", "a", false}, {"c", "b", false},
		{"", "d", true}, {"", "", true}, {"a", "", false},
		{"unknown", "d", false},
	}
	for _, c := range anc {
		got, err := h.IsAncestor(ctx, c.a, c.d)
		must(t, err)
		if got != c.want {
			t.Errorf("IsAncestor(%q, %q) = %v, want %v", c.a, c.d, got, c.want)
		}
	}
	_, err := h.IsAncestor(ctx, "a", "unknown")
	wantErr(t, err, domain.ErrNotFound)

	log, err := h.Log(ctx, "d", 0)
	must(t, err)
	wantEqual(t, "Log(d)", snapIDs(log), []string{"d", "c", "b", "a"})
	if !log[0].CreatedAt.Equal(at(3)) || log[0].TreeID != "tr" || !slices.Equal(log[0].Parents, []string{"c"}) {
		t.Fatalf("Log returned incomplete snapshot: %+v", log[0])
	}
	log, err = h.Log(ctx, "d", 2)
	must(t, err)
	wantEqual(t, "Log(d, 2)", snapIDs(log), []string{"d", "c"})
	log, err = h.Log(ctx, "b", 10)
	must(t, err)
	wantEqual(t, "Log(b, 10)", snapIDs(log), []string{"b", "a"})
	log, err = h.Log(ctx, "", 0)
	must(t, err)
	if log == nil || len(log) != 0 {
		t.Fatalf("Log(\"\") = %#v, want empty non-nil", log)
	}
	_, err = h.Log(ctx, "unknown", 0)
	wantErr(t, err, domain.ErrNotFound)

	miss := []struct {
		tip   string
		haves []string
		want  []string
	}{
		{"d", nil, []string{"a", "b", "c", "d"}},
		{"d", []string{"b"}, []string{"c", "d"}},
		{"d", []string{"d"}, []string{}},
		{"b", []string{"d"}, []string{}}, // receiver is ahead
		{"d", []string{"unknown", ""}, []string{"a", "b", "c", "d"}},
		{"d", []string{"a", "c", "a"}, []string{"d"}},
		{"", []string{"a"}, []string{}},
	}
	for _, c := range miss {
		got, err := h.Missing(ctx, c.tip, c.haves)
		must(t, err)
		wantEqual(t, fmt.Sprintf("Missing(%q, %v)", c.tip, c.haves), snapIDs(got), c.want)
	}
	_, err = h.Missing(ctx, "unknown", nil)
	wantErr(t, err, domain.ErrNotFound)
}

func testHistoryMerge(t *testing.T, s domain.MetaStore, mk func(domain.MetaStore) domain.History) {
	ctx := context.Background()
	//      b
	//    /   \
	//  a       d - e
	//    \   /
	//      c
	buildDAG(t, s, "a", "b:a", "c:a", "d:b,c", "e:d")
	h := mk(s)

	for _, c := range []struct {
		a, d string
		want bool
	}{
		{"c", "e", true}, {"b", "d", true}, {"a", "e", true},
		{"b", "c", false}, {"c", "b", false}, {"e", "d", false},
	} {
		got, err := h.IsAncestor(ctx, c.a, c.d)
		must(t, err)
		if got != c.want {
			t.Errorf("IsAncestor(%s, %s) = %v, want %v", c.a, c.d, got, c.want)
		}
	}
	log, err := h.Log(ctx, "e", 0)
	must(t, err)
	wantEqual(t, "Log(e)", snapIDs(log), []string{"e", "d", "c", "b", "a"})

	for _, c := range []struct {
		haves []string
		want  []string
	}{
		{nil, []string{"a", "b", "c", "d", "e"}},
		{[]string{"b"}, []string{"c", "d", "e"}},
		{[]string{"c"}, []string{"b", "d", "e"}},
		{[]string{"b", "c"}, []string{"d", "e"}},
		{[]string{"d"}, []string{"e"}},
	} {
		got, err := h.Missing(ctx, "e", c.haves)
		must(t, err)
		wantEqual(t, fmt.Sprintf("Missing(e, %v)", c.haves), snapIDs(got), c.want)
	}
}

func testHistoryCrissCross(t *testing.T, s domain.MetaStore, mk func(domain.MetaStore) domain.History) {
	ctx := context.Background()
	//  a - b - d - f
	//    X   X
	//  . - c - e /
	// d = merge(b, c), e = merge(c, b), f = merge(d, e)
	buildDAG(t, s, "a", "b:a", "c:a", "d:b,c", "e:c,b", "f:d,e")
	h := mk(s)

	got, err := h.Missing(ctx, "f", []string{"d"})
	must(t, err)
	wantEqual(t, "Missing(f, d)", snapIDs(got), []string{"e", "f"})
	got, err = h.Missing(ctx, "f", []string{"b"})
	must(t, err)
	wantEqual(t, "Missing(f, b)", snapIDs(got), []string{"c", "d", "e", "f"})
	got, err = h.Missing(ctx, "f", []string{"d", "e"})
	must(t, err)
	wantEqual(t, "Missing(f, d+e)", snapIDs(got), []string{"f"})
	got, err = h.Missing(ctx, "e", []string{"d"})
	must(t, err)
	wantEqual(t, "Missing(e, d)", snapIDs(got), []string{"e"})

	log, err := h.Log(ctx, "f", 0)
	must(t, err)
	wantEqual(t, "Log(f)", snapIDs(log), []string{"f", "e", "d", "c", "b", "a"})

	for _, c := range []struct {
		a, d string
		want bool
	}{{"d", "e", false}, {"e", "d", false}, {"b", "e", true}, {"c", "d", true}, {"a", "f", true}} {
		ok, err := h.IsAncestor(ctx, c.a, c.d)
		must(t, err)
		if ok != c.want {
			t.Errorf("IsAncestor(%s, %s) = %v, want %v", c.a, c.d, ok, c.want)
		}
	}

	// Deterministic across calls.
	first, err := h.Missing(ctx, "f", nil)
	must(t, err)
	for i := 0; i < 5; i++ {
		again, err := h.Missing(ctx, "f", nil)
		must(t, err)
		wantEqual(t, "Missing determinism", snapIDs(again), snapIDs(first))
	}
	wantEqual(t, "Missing(f)", snapIDs(first), []string{"a", "b", "c", "d", "e", "f"})
}

func testHistorySkew(t *testing.T, s domain.MetaStore, mk func(domain.MetaStore) domain.History) {
	ctx := context.Background()
	// Children with timestamps older than their parents.
	put := func(id string, sec int, parents ...string) {
		if parents == nil {
			parents = []string{}
		}
		must(t, s.PutSnapshot(ctx, &domain.Snapshot{ID: id, TreeID: "tr", Parents: parents, CreatedAt: at(sec)}))
	}
	put("root", 100)
	put("x", 50, "root")
	put("y", 10, "x")
	put("side", 200, "root")
	put("m", 5, "y", "side")
	h := mk(s)

	got, err := h.Missing(ctx, "m", nil)
	must(t, err)
	checkTopo(t, got)
	wantEqual(t, "Missing with skew", snapIDs(got), []string{"root", "x", "y", "side", "m"})

	log, err := h.Log(ctx, "m", 0)
	must(t, err)
	// Best-effort date order: the tip first, every snapshot exactly once,
	// and each one after the child it was reached through.
	wantEqual(t, "Log with skew", snapIDs(log), []string{"m", "side", "root", "y", "x"})
}

func testHistoryDangling(t *testing.T, s domain.MetaStore, mk func(domain.MetaStore) domain.History) {
	ctx := context.Background()
	buildDAG(t, s, "a", "b:a,ghost", "c:b")
	h := mk(s)
	_, err := h.Log(ctx, "c", 0)
	wantErr(t, err, domain.ErrNotFound)
	_, err = h.Missing(ctx, "c", nil)
	wantErr(t, err, domain.ErrNotFound)
	_, err = h.IsAncestor(ctx, "zzz-not-there", "c") // unknown ancestor: false, no walk
	must(t, err)
	// Bounded Log that stops before the dangling edge succeeds.
	log, err := h.Log(ctx, "c", 1)
	must(t, err)
	wantEqual(t, "Log(c, 1)", snapIDs(log), []string{"c"})
	// Missing that never needs the dangling edge succeeds.
	got, err := h.Missing(ctx, "c", []string{"b"})
	if err == nil {
		t.Fatalf("Missing(c, b) walked haves through a dangling parent without error: %v", snapIDs(got))
	}
}

// testHistoryRandom cross-checks every History method against brute force
// on a random DAG with monotone timestamps.
func testHistoryRandom(t *testing.T, s domain.MetaStore, mk func(domain.MetaStore) domain.History) {
	ctx := context.Background()
	rng := rand.New(rand.NewPCG(42, 7))
	const n = 250
	ids := make([]string, n)
	parents := make([][]string, n)
	var specs []string
	for i := 0; i < n; i++ {
		ids[i] = fmt.Sprintf("n%03d", i)
		if i > 0 {
			k := 1 + rng.IntN(3)
			if rng.IntN(4) == 0 {
				k = 1
			}
			seen := map[int]bool{}
			for j := 0; j < k; j++ {
				// Bias towards recent nodes for realistic depth.
				p := i - 1 - rng.IntN(min(i, 12))
				if !seen[p] {
					seen[p] = true
					parents[i] = append(parents[i], ids[p])
				}
			}
		}
		specs = append(specs, ids[i]+":"+strings.Join(parents[i], ","))
	}
	buildDAG(t, s, specs...)
	index := map[string]int{}
	for i, id := range ids {
		index[id] = i
	}
	reach := func(from string) map[string]bool {
		out := map[string]bool{}
		stack := []string{from}
		for len(stack) > 0 {
			x := stack[len(stack)-1]
			stack = stack[:len(stack)-1]
			if out[x] {
				continue
			}
			out[x] = true
			stack = append(stack, parents[index[x]]...)
		}
		return out
	}
	h := mk(s)
	for trial := 0; trial < 40; trial++ {
		tip := ids[rng.IntN(n)]
		var haves []string
		for j := rng.IntN(4); j > 0; j-- {
			haves = append(haves, ids[rng.IntN(n)])
		}
		r := reach(tip)
		hv := map[string]bool{}
		for _, x := range haves {
			for k := range reach(x) {
				hv[k] = true
			}
		}
		// Expected Missing: monotone timestamps => sorted by creation.
		var want []string
		for _, id := range ids {
			if r[id] && !hv[id] {
				want = append(want, id)
			}
		}
		if want == nil {
			want = []string{}
		}
		got, err := h.Missing(ctx, tip, haves)
		must(t, err)
		checkTopo(t, got)
		wantEqual(t, fmt.Sprintf("Missing(%s, %v)", tip, haves), snapIDs(got), want)

		// Expected Log: reachable sorted newest first.
		var wantLog []string
		for i := n - 1; i >= 0; i-- {
			if r[ids[i]] {
				wantLog = append(wantLog, ids[i])
			}
		}
		log, err := h.Log(ctx, tip, 0)
		must(t, err)
		wantEqual(t, "Log("+tip+")", snapIDs(log), wantLog)
		lim := 1 + rng.IntN(10)
		log, err = h.Log(ctx, tip, lim)
		must(t, err)
		wantEqual(t, "Log("+tip+", limit)", snapIDs(log), wantLog[:min(lim, len(wantLog))])

		other := ids[rng.IntN(n)]
		ok, err := h.IsAncestor(ctx, other, tip)
		must(t, err)
		if ok != r[other] {
			t.Fatalf("IsAncestor(%s, %s) = %v, want %v", other, tip, ok, r[other])
		}
	}
}

// buildChain stores a linear history of n snapshots c00000..c(n-1).
func buildChain(t testing.TB, s domain.MetaStore, n int) []string {
	t.Helper()
	ctx := context.Background()
	ids := make([]string, n)
	for i := range ids {
		ids[i] = fmt.Sprintf("c%06d", i)
		parents := []string{}
		if i > 0 {
			parents = []string{ids[i-1]}
		}
		if err := s.PutSnapshot(ctx, &domain.Snapshot{ID: ids[i], TreeID: "tr", Parents: parents, CreatedAt: at(i)}); err != nil {
			t.Fatal(err)
		}
	}
	return ids
}

func timed(t *testing.T, name string, limit time.Duration, fn func()) {
	t.Helper()
	start := time.Now()
	fn()
	d := time.Since(start)
	t.Logf("%s: %v", name, d)
	if d > limit {
		t.Errorf("%s took %v, want < %v", name, d, limit)
	}
}

func testHistoryChainPerf(t *testing.T, s domain.MetaStore, n int) {
	ctx := context.Background()
	ids := buildChain(t, s, n)
	tip, root := ids[n-1], ids[0]
	h := NewHistory(s)
	const limit = 5 * time.Second

	timed(t, "IsAncestor(root, tip)", limit, func() {
		ok, err := h.IsAncestor(ctx, root, tip)
		must(t, err)
		if !ok {
			t.Fatal("root is not an ancestor of tip")
		}
	})
	timed(t, "IsAncestor(tip, root)", limit, func() {
		ok, err := h.IsAncestor(ctx, tip, root)
		must(t, err)
		if ok {
			t.Fatal("tip is an ancestor of root")
		}
	})
	timed(t, "Log(tip, 0)", limit, func() {
		log, err := h.Log(ctx, tip, 0)
		must(t, err)
		if len(log) != n || log[0].ID != tip || log[n-1].ID != root {
			t.Fatalf("Log returned %d entries", len(log))
		}
	})
	timed(t, "Log(tip, 50)", limit/10, func() {
		log, err := h.Log(ctx, tip, 50)
		must(t, err)
		if len(log) != 50 {
			t.Fatalf("Log returned %d entries", len(log))
		}
	})
	timed(t, "Missing(tip, nil)", limit, func() {
		got, err := h.Missing(ctx, tip, nil)
		must(t, err)
		if len(got) != n || got[0].ID != root || got[n-1].ID != tip {
			t.Fatalf("Missing returned %d entries", len(got))
		}
	})
	timed(t, "Missing(tip, [tip~10])", limit, func() {
		got, err := h.Missing(ctx, tip, []string{ids[n-11]})
		must(t, err)
		wantEqual(t, "Missing tail", snapIDs(got), ids[n-10:])
	})
}

// testHistoryWide builds many parallel lines merged together, to catch
// quadratic behavior in merge-heavy graphs.
func testHistoryWide(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	var specs []string
	specs = append(specs, "root")
	prevMerge := "root"
	for round := 0; round < 20; round++ {
		var heads []string
		for lane := 0; lane < 20; lane++ {
			id := fmt.Sprintf("r%02d-l%02d", round, lane)
			specs = append(specs, id+":"+prevMerge)
			heads = append(heads, id)
		}
		m := fmt.Sprintf("m%02d", round)
		specs = append(specs, m+":"+strings.Join(heads, ","))
		prevMerge = m
	}
	buildDAG(t, s, specs...)
	h := NewHistory(s)
	got, err := h.Missing(ctx, prevMerge, nil)
	must(t, err)
	if len(got) != len(specs) {
		t.Fatalf("Missing returned %d, want %d", len(got), len(specs))
	}
	checkTopo(t, got)
	got, err = h.Missing(ctx, prevMerge, []string{"m18"})
	must(t, err)
	if len(got) != 21 || got[len(got)-1].ID != "m19" {
		t.Fatalf("Missing(m19, m18) = %v", snapIDs(got))
	}
	ok, err := h.IsAncestor(ctx, "r00-l07", prevMerge)
	must(t, err)
	if !ok {
		t.Fatal("lane head not an ancestor of final merge")
	}
	log, err := h.Log(ctx, prevMerge, 0)
	must(t, err)
	if len(log) != len(specs) || log[0].ID != prevMerge || log[len(log)-1].ID != "root" {
		t.Fatalf("Log length %d", len(log))
	}
}

func TestHistory_Chain100kMemory(t *testing.T) {
	if testing.Short() {
		t.Skip("large history test skipped in -short mode")
	}
	testHistoryChainPerf(t, NewMemory(), 100_000)
}

func TestHistory_ErrorsAreWrapped(t *testing.T) {
	s := NewMemory()
	buildDAG(t, s, "a:missing")
	_, err := NewHistory(s).Log(context.Background(), "a", 0)
	if !errors.Is(err, domain.ErrNotFound) || !strings.Contains(err.Error(), "missing") {
		t.Fatalf("dangling parent error = %v", err)
	}
}
