package textdiff

import (
	"reflect"
	"testing"
)

// v0.6 blame workstream (§14.4): exported line matching.

func TestLinesAndMatches(t *testing.T) {
	a := Lines([]byte("a\nb\nc\nd"))
	if !reflect.DeepEqual(a, []string{"a\n", "b\n", "c\n", "d"}) {
		t.Fatalf("Lines = %q", a)
	}
	b := Lines([]byte("a\nX\nc\nd\ne\n"))
	// "d" (no newline) and "d\n" are different lines.
	if got, want := Matches(a, b), [][2]int{{0, 0}, {2, 2}}; !reflect.DeepEqual(got, want) {
		t.Fatalf("Matches = %v, want %v", got, want)
	}
	if got := Matches(nil, b); len(got) != 0 {
		t.Fatalf("Matches(nil, b) = %v", got)
	}
	if got, want := Matches(b, b), [][2]int{{0, 0}, {1, 1}, {2, 2}, {3, 3}, {4, 4}}; !reflect.DeepEqual(got, want) {
		t.Fatalf("Matches(b, b) = %v", got)
	}
}
