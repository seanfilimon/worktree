package merge3

import (
	"errors"
	"fmt"
	"slices"
	"testing"
)

// dag returns a ParentsFunc over an explicit parent map; unknown ids are
// roots, as in the Rust SDK's merge_base.
func dag(edges map[string][]string) ParentsFunc {
	return func(id string) ([]string, error) { return edges[id], nil }
}

func baseOf(t *testing.T, a, b string, parents ParentsFunc) string {
	t.Helper()
	base, ok, err := MergeBase(a, b, parents)
	if err != nil {
		t.Fatal(err)
	}
	if !ok {
		return "<none>"
	}
	return base
}

func TestMergeBaseLinearAndFork(t *testing.T) {
	g := dag(map[string][]string{"c1": nil, "c2": {"c1"}, "c3": {"c2"}, "c4": {"c3"}})
	eq(t, baseOf(t, "c4", "c2", g), "c2")
	eq(t, baseOf(t, "c2", "c4", g), "c2")
	eq(t, baseOf(t, "c3", "c3", g), "c3")
	eq(t, baseOf(t, "c1", "c4", g), "c1")

	g = dag(map[string][]string{"r": nil, "m1": {"r"}, "m2": {"m1"}, "f1": {"m1"}, "f2": {"f1"}})
	eq(t, baseOf(t, "m2", "f2", g), "m1")
	eq(t, baseOf(t, "f2", "m2", g), "m1")
}

func TestMergeBaseAfterPreviousMerge(t *testing.T) {
	g := dag(map[string][]string{"r": nil, "m1": {"r"}, "f1": {"r"}, "M": {"m1", "f1"}, "m3": {"M"}, "f2": {"f1"}})
	eq(t, baseOf(t, "m3", "f2", g), "f1")
	eq(t, baseOf(t, "f2", "m3", g), "f1")
}

func TestMergeBaseCrissCross(t *testing.T) {
	g := dag(map[string][]string{"r": nil, "x1": {"r"}, "y1": {"r"}, "mx": {"x1", "y1"}, "my": {"y1", "x1"}, "x2": {"mx"}, "y2": {"my"}})
	eq(t, baseOf(t, "x2", "y2", g), "x1")
	eq(t, baseOf(t, "y2", "x2", g), "x1")

	g = dag(map[string][]string{"r": nil, "x1": {"r"}, "y1": {"r"}, "y0": {"y1"}, "mx": {"x1", "y0"}, "my": {"y0", "x1"}})
	eq(t, baseOf(t, "mx", "my", g), "x1")
	g = dag(map[string][]string{"r": nil, "x1": {"r"}, "y1": {"r"}, "xa": {"x1"}, "mx": {"xa", "y1"}, "my": {"y1", "x1"}})
	eq(t, baseOf(t, "mx", "my", g), "y1")
}

func TestMergeBaseEdgeCases(t *testing.T) {
	eq(t, baseOf(t, "a2", "b2", dag(map[string][]string{"a1": nil, "a2": {"a1"}, "b1": nil, "b2": {"b1"}})), "<none>")

	g := dag(map[string][]string{"c2": {"c1"}})
	eq(t, baseOf(t, "c2", "c1", g), "c1")
	eq(t, baseOf(t, "ghost", "ghost", g), "ghost")
	eq(t, baseOf(t, "ghost", "c2", g), "<none>")
	eq(t, baseOf(t, "x", "y", dag(nil)), "<none>")

	g = dag(map[string][]string{"ra": nil, "rb": nil, "j": {"ra", "rb"}, "p": {"j"}, "q": {"j"}, "s": {"rb"}})
	eq(t, baseOf(t, "p", "q", g), "j")
	eq(t, baseOf(t, "p", "s", g), "rb")

	g = dag(map[string][]string{"r": nil, "a": {"r"}, "b": {"r"}, "c": {"r"}, "o": {"a", "b", "c"}, "c2": {"c"}})
	eq(t, baseOf(t, "o", "c2", g), "c")
	eq(t, baseOf(t, "o", "b", g), "b")

	g1 := dag(map[string][]string{"r": nil, "x": {"r"}, "y": {"r"}, "m": {"x", "y"}, "t": {"y"}})
	g2 := dag(map[string][]string{"r": nil, "x": {"r"}, "y": {"r"}, "m": {"y", "x"}, "t": {"y"}})
	eq(t, baseOf(t, "m", "t", g1), "y")
	eq(t, baseOf(t, "m", "t", g2), "y")
}

func TestMergeBaseDeepHistoryAndLookups(t *testing.T) {
	const n = 100_000
	edges := make(map[string][]string, n+1)
	edges["c0"] = nil
	for i := 1; i < n; i++ {
		edges[fmt.Sprintf("c%d", i)] = []string{fmt.Sprintf("c%d", i-1)}
	}
	edges["side"] = []string{"c10"}
	calls := map[string]int{}
	counting := func(id string) ([]string, error) {
		calls[id]++
		return edges[id], nil
	}
	eq(t, baseOf(t, fmt.Sprintf("c%d", n-1), "side", counting), "c10")
	for id, c := range calls {
		if c != 1 {
			t.Fatalf("%s looked up %d times", id, c)
		}
	}
}

func TestMergeBaseErrors(t *testing.T) {
	boom := errors.New("boom")
	_, _, err := MergeBase("a", "b", func(id string) ([]string, error) {
		if id == "b" {
			return nil, boom
		}
		return nil, nil
	})
	if !errors.Is(err, boom) {
		t.Fatalf("err = %v", err)
	}
}

func TestMergeBaseBruteForceAgreement(t *testing.T) {
	r := newRng(15)
	for iter := 0; iter < 300; iter++ {
		n := 2 + r.Intn(14)
		ids := make([]string, n)
		edges := map[string][]string{}
		for i := range ids {
			ids[i] = fmt.Sprintf("n%02d", i)
			for j := 0; j < i; j++ {
				if r.Intn(4) == 0 {
					edges[ids[i]] = append(edges[ids[i]], ids[j])
				}
			}
		}
		a, b := ids[r.Intn(n)], ids[r.Intn(n)]
		got := baseOf(t, a, b, dag(edges))
		want := bruteBase(a, b, edges)
		if got != want {
			t.Fatalf("merge base of %s, %s in %v = %s, want %s", a, b, edges, got, want)
		}
	}
}

// bruteBase reads the definition literally.
func bruteBase(a, b string, edges map[string][]string) string {
	anc := func(x string) map[string]int {
		dist := map[string]int{x: 0}
		queue := []string{x}
		for len(queue) > 0 {
			id := queue[0]
			queue = queue[1:]
			for _, p := range edges[id] {
				if _, ok := dist[p]; !ok {
					dist[p] = dist[id] + 1
					queue = append(queue, p)
				}
			}
		}
		return dist
	}
	da, db := anc(a), anc(b)
	var common []string
	for id := range da {
		if _, ok := db[id]; ok {
			common = append(common, id)
		}
	}
	var best []string
	for _, c := range common {
		dominated := false
		for _, other := range common {
			if other != c {
				if _, ok := anc(other)[c]; ok {
					dominated = true
					break
				}
			}
		}
		if !dominated {
			best = append(best, c)
		}
	}
	if len(best) == 0 {
		return "<none>"
	}
	slices.SortFunc(best, func(x, y string) int {
		if da[x] != da[y] {
			return da[x] - da[y]
		}
		if x < y {
			return -1
		}
		return 1
	})
	return best[0]
}
