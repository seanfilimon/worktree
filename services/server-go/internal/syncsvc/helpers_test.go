package syncsvc

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"regexp"
	"strings"
	"testing"
	"time"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

func TestToStatus(t *testing.T) {
	s := New(Deps{})
	ctx := context.Background()
	cases := []struct {
		err  error
		code codes.Code
		msg  string // substring
	}{
		{fmt.Errorf("meta: tree abc: %w", domain.ErrNotFound), codes.NotFound, "tree: not found"},
		{fmt.Errorf("x: %w", domain.ErrAlreadyExists), codes.AlreadyExists, "already exists"},
		{fmt.Errorf("x: %w", domain.ErrConflict), codes.FailedPrecondition, "conflicting"},
		{fmt.Errorf("x: %w", domain.ErrUnauthenticated), codes.Unauthenticated, "authentication required"},
		{fmt.Errorf("x: %w", domain.ErrForbidden), codes.PermissionDenied, "permission denied"},
		{fmt.Errorf("%w: name too long", domain.ErrInvalid), codes.InvalidArgument, "name too long"},
		{&domain.PushDenied{Kind: "quota_exceeded", Reason: "full"}, codes.PermissionDenied, "full"},
		{context.Canceled, codes.Canceled, ""},
		{context.DeadlineExceeded, codes.DeadlineExceeded, ""},
		{status.Error(codes.Aborted, "keep me"), codes.Aborted, "keep me"},
		{errors.New("open C:\\secret\\meta.db: disk exploded"), codes.Internal, "tree: internal error"},
	}
	for _, tc := range cases {
		err := s.toStatus(ctx, tc.err, "tree")
		st, _ := status.FromError(err)
		if st.Code() != tc.code || !strings.Contains(st.Message(), tc.msg) {
			t.Errorf("toStatus(%v) = %v %q, want %v containing %q", tc.err, st.Code(), st.Message(), tc.code, tc.msg)
		}
		if strings.Contains(st.Message(), "secret") || strings.Contains(st.Message(), "meta:") {
			t.Errorf("toStatus(%v) leaks internals: %q", tc.err, st.Message())
		}
	}
	if s.toStatus(ctx, nil, "x") != nil {
		t.Fatal("toStatus(nil) != nil")
	}
}

func TestValidateRefName(t *testing.T) {
	for _, ok := range []string{"main", "feature/x", "release-1.2", "user/alice/wip", "v1.0.0", "ünïcode"} {
		if err := validateRefName("branch", ok); err != nil {
			t.Errorf("validateRefName(%q) = %v", ok, err)
		}
	}
	for _, bad := range []string{"", "a b", "a//b", "/a", "a/", "..", "a/../b", "a~1", "a^", "a:b", "a?", "a*", "a[", `a\b`, "tab\t", strings.Repeat("x", 256), "\xff"} {
		if err := validateRefName("branch", bad); status.Code(err) != codes.InvalidArgument {
			t.Errorf("validateRefName(%q) = %v, want InvalidArgument", bad, err)
		}
	}
}

func TestValidateTreeName(t *testing.T) {
	for _, ok := range []string{"web", "my-tree", "a.b_c", "X1"} {
		if err := validateTreeName(ok); err != nil {
			t.Errorf("validateTreeName(%q) = %v", ok, err)
		}
	}
	for _, bad := range []string{"", ".hidden", "-x", "a/b", "a b", strings.Repeat("x", 101)} {
		if err := validateTreeName(bad); err == nil {
			t.Errorf("validateTreeName(%q) = nil", bad)
		}
	}
}

func TestValidateTOMLish(t *testing.T) {
	valid := []string{
		"a = 1",
		"# comment\n[table]\nkey = \"value\" # trailing\n",
		"[[roles]]\nname = \"dev\"\nperms = [\"a\", \"b\"]\n",
		"x = [\n  1,\n  2, # c\n]\n",
		"t = { a = 1, b = [2, 3] }",
		"s = \"\"\"\nmulti\nline = not a key\n\"\"\"\n",
		"'quoted key' = 'v'\n\"k=2\" = 3\r\n",
		"[server]  # header comment\nport = 7420",
		"esc = \"a \\\" quote\"",
	}
	for _, doc := range valid {
		if errs := validateTOMLish([]byte(doc)); errs != nil {
			t.Errorf("validateTOMLish(%q) = %v", doc, errs)
		}
	}
	invalid := map[string]string{
		"":                       "empty",
		"   \n\t\n":              "empty",
		"# only a comment\n":     "defines no",
		"just words":             "line 1",
		"= 1":                    "empty key",
		"a =":                    "missing value",
		"[unclosed":              "unterminated table header",
		"[]":                     "empty table name",
		"[a] junk":               "after table header",
		"a = [1, 2":              "unterminated array",
		"a = \"\"\"\nnever ends": "unterminated multi-line string",
		"a = \"open":             "unterminated string",
		"a = 1]":                 "unbalanced",
		"a = 1\x00":              "NUL",
		"a = \xff":               "UTF-8",
	}
	for doc, want := range invalid {
		errs := validateTOMLish([]byte(doc))
		if len(errs) == 0 || !strings.Contains(strings.Join(errs, "; "), want) {
			t.Errorf("validateTOMLish(%q) = %v, want error containing %q", doc, errs, want)
		}
	}
	// Error count is capped.
	if errs := validateTOMLish([]byte(strings.Repeat("bad line\n", 100))); len(errs) != maxValidationErrors {
		t.Errorf("got %d errors, want cap %d", len(errs), maxValidationErrors)
	}
}

func TestDiffManifests(t *testing.T) {
	h1, h2 := hashToPB(hash.Sum([]byte("1"))), hashToPB(hash.Sum([]byte("2")))
	m := func(entries ...*syncpb.ManifestEntry) *syncpb.ManifestRecord {
		return &syncpb.ManifestRecord{Entries: entries}
	}
	e := func(p string, h *syncpb.ContentHash, exec bool) *syncpb.ManifestEntry {
		return &syncpb.ManifestEntry{Path: p, BlobHash: h, Executable: exec}
	}
	old := m(e("a", h1, false), e("b", h1, false), e("c", h1, false), e("d", h1, false))
	cur := m(e("a", h1, false), e("b", h2, false), e("c", h1, true), e("e", h1, false))
	if got, want := diffManifests(old, cur), []string{"b", "c", "d", "e"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("diff = %v, want %v", got, want)
	}
	if got := diffManifests(m(), cur); !reflect.DeepEqual(got, []string{"a", "b", "c", "e"}) {
		t.Fatalf("diff from empty = %v", got)
	}
	if got := diffManifests(old, old); len(got) != 0 {
		t.Fatalf("self diff = %v", got)
	}
}

func TestDecodeManifestPaths(t *testing.T) {
	h := hashToPB(hash.Sum(nil))
	for _, p := range []string{"", "/abs", "a/../b", "./a", "a//b", "a/", `a\b`, "..", "../x"} {
		data := mustMarshal(t, &syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{{Path: p, BlobHash: h}}})
		if _, err := decodeManifest(data); !errors.Is(err, errInvalidManifest) {
			t.Errorf("path %q accepted", p)
		}
	}
	short := mustMarshal(t, &syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{{Path: "a", BlobHash: &syncpb.ContentHash{Value: []byte{1}}}}})
	if _, err := decodeManifest(short); err == nil {
		t.Error("short blob hash accepted")
	}
	dup := mustMarshal(t, &syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{{Path: "a", BlobHash: h}, {Path: "a", BlobHash: h}}})
	if _, err := decodeManifest(dup); err == nil {
		t.Error("duplicate path accepted")
	}
	if m, err := decodeManifest(nil); err != nil || len(m.GetEntries()) != 0 {
		t.Errorf("empty manifest = %v, %v", m, err)
	}
}

func mustMarshal(t *testing.T, m proto.Message) []byte {
	t.Helper()
	data, err := proto.Marshal(m)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	return data
}

func TestConversions(t *testing.T) {
	if _, err := hashFromPB(nil); err == nil {
		t.Error("nil hash accepted")
	}
	if _, err := hashFromPB(&syncpb.ContentHash{Value: make([]byte, 33)}); err == nil {
		t.Error("33-byte hash accepted")
	}
	want := hash.Sum([]byte("x"))
	if got, err := hashFromPB(hashToPB(want)); err != nil || got != want {
		t.Errorf("hash round trip = %v, %v", got, err)
	}

	at := time.Date(2026, 3, 4, 5, 6, 7, 8, time.UTC)
	snap := &domain.Snapshot{
		ID: "s2", TreeID: "t", BranchName: "main", Parents: []string{"s1", "s0"}, ManifestHash: want,
		Author: "alice", Message: "msg", CreatedAt: at, AutoGenerated: true,
	}
	back, err := snapshotFromPB(snapshotToPB(snap))
	if err != nil {
		t.Fatalf("snapshotFromPB: %v", err)
	}
	if !reflect.DeepEqual(&back, snap) {
		t.Fatalf("round trip = %+v, want %+v", back, snap)
	}
	if !tsFromPB(nil).IsZero() || tsToPB(time.Time{}) != nil {
		t.Error("zero timestamp handling")
	}

	bad := []*syncpb.SnapshotRecord{
		{},
		{Id: &syncpb.SnapshotId{Value: "has space"}, ManifestHash: hashToPB(want)},
		{Id: &syncpb.SnapshotId{Value: "s"}},
		{Id: &syncpb.SnapshotId{Value: "s"}, ManifestHash: hashToPB(want), Parents: []*syncpb.SnapshotId{{Value: "s"}}},
		{Id: &syncpb.SnapshotId{Value: "s"}, ManifestHash: hashToPB(want), Parents: []*syncpb.SnapshotId{{Value: "p"}, {Value: "p"}}},
		{Id: &syncpb.SnapshotId{Value: "s"}, ManifestHash: hashToPB(want), Parents: []*syncpb.SnapshotId{{Value: ""}}},
	}
	for i, r := range bad {
		if _, err := snapshotFromPB(r); err == nil {
			t.Errorf("bad record %d accepted", i)
		}
	}
}

func TestNewID(t *testing.T) {
	re := regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`)
	seen := map[string]bool{}
	for i := 0; i < 1000; i++ {
		id := newID()
		if !re.MatchString(id) || seen[id] {
			t.Fatalf("bad or duplicate id %q", id)
		}
		seen[id] = true
	}
}
