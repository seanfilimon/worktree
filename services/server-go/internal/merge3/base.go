package merge3

// ParentsFunc returns the parent ids of a snapshot. Unknown ids may be
// reported with (nil, nil), which treats them as roots; any error aborts the
// walk and is returned unchanged.
type ParentsFunc func(id string) ([]string, error)

// MergeBase returns the best common ancestor of snapshots a and b: an
// ancestor reachable from both that is not an ancestor of another common
// ancestor; ties are broken by the smallest breadth-first distance from a,
// then by id. ok is false when the histories are unrelated.
//
// Every snapshot is its own ancestor, so when one of a/b is an ancestor of
// the other it is the result. Same semantics as the Rust SDK's merge_base.
// Runs in O(V + E) parent lookups (each id is looked up once) without
// recursion.
func MergeBase(a, b string, parents ParentsFunc) (base string, ok bool, err error) {
	cache := map[string][]string{}
	parentsOf := func(id string) ([]string, error) {
		if ps, ok := cache[id]; ok {
			return ps, nil
		}
		ps, err := parents(id)
		if err != nil {
			return nil, err
		}
		cache[id] = ps
		return ps, nil
	}

	fromA, err := bfsDistances(a, parentsOf)
	if err != nil {
		return "", false, err
	}
	fromB, err := bfsDistances(b, parentsOf)
	if err != nil {
		return "", false, err
	}
	var common []string
	for id := range fromA {
		if _, ok := fromB[id]; ok {
			common = append(common, id)
		}
	}

	// Every proper ancestor of a common ancestor is itself common and is
	// dominated; one multi-source walk marks them all. Parents were all
	// looked up by the walks above, so this cannot fail.
	dominated := map[string]bool{}
	var stack []string
	for _, id := range common {
		stack = append(stack, cache[id]...)
	}
	for len(stack) > 0 {
		id := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		if dominated[id] {
			continue
		}
		dominated[id] = true
		stack = append(stack, cache[id]...)
	}

	for _, id := range common {
		if dominated[id] {
			continue
		}
		if !ok || fromA[id] < fromA[base] || (fromA[id] == fromA[base] && id < base) {
			base, ok = id, true
		}
	}
	return base, ok, nil
}

// bfsDistances returns the shortest parent-edge distance from start to each
// of its ancestors (start itself at distance 0).
func bfsDistances(start string, parentsOf ParentsFunc) (map[string]int, error) {
	dist := map[string]int{start: 0}
	queue := []string{start}
	for len(queue) > 0 {
		id := queue[0]
		queue = queue[1:]
		ps, err := parentsOf(id)
		if err != nil {
			return nil, err
		}
		next := dist[id] + 1
		for _, p := range ps {
			if _, seen := dist[p]; !seen {
				dist[p] = next
				queue = append(queue, p)
			}
		}
	}
	return dist, nil
}
