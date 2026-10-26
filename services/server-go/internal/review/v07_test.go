package review

// Tests of the v0.7 deeper review features (§15.7): path owners, review
// requests, drafts, suggested changes and storage quotas.

import (
	"context"
	"errors"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// v7env is a merge env with accounts, a team and tree:admin for carol.
type v7env struct {
	*menv
	authz *fakeAuthz
}

func newV7Env(t *testing.T) *v7env {
	t.Helper()
	e := newMergeEnv(t)
	ctx := context.Background()
	authz := &fakeAuthz{grants: map[string][]domain.Action{
		"alice": developer, "bob": developer, "dave": developer,
		"carol": append(slices.Clone(maintainer), domain.ActTreeAdmin),
		"eve":   {domain.ActTreeRead},
	}}
	e.svc.authz = authz
	for _, p := range []*domain.Principal{alice, bob, carol, dave, eve} {
		if err := e.store.CreateAccount(ctx, &domain.Account{ID: p.AccountID, Username: p.Username, Email: p.Username + "@x.test", TenantID: p.TenantID}); err != nil {
			t.Fatal(err)
		}
	}
	team := &domain.Team{ID: "team-core", TenantID: e.tree.TenantID, Slug: "core", Name: "Core"}
	if err := e.store.CreateTeam(ctx, team); err != nil {
		t.Fatal(err)
	}
	if err := e.store.PutTeamMember(ctx, &domain.TeamMember{TeamID: team.ID, AccountID: dave.AccountID, Role: "member"}); err != nil {
		t.Fatal(err)
	}
	return &v7env{menv: e, authz: authz}
}

// events returns the published events of type typ.
func (e *v7env) events(typ string) []domain.Event {
	e.bus.mu.Lock()
	defer e.bus.mu.Unlock()
	var out []domain.Event
	for _, ev := range e.bus.events {
		if ev.Type == typ {
			out = append(out, ev)
		}
	}
	return out
}

func TestNormalizeOwner(t *testing.T) {
	for in, want := range map[string]string{
		"user:alice": "user:alice", "team:Core": "team:core", "@bob": "user:bob", "bob": "user:bob",
		"@acme/core": "team:core", "acme/Core": "team:core", " user:x.y-z ": "user:x.y-z",
	} {
		got, err := NormalizeOwner(in)
		if err != nil || got != want {
			t.Errorf("NormalizeOwner(%q) = %q, %v; want %q", in, got, err, want)
		}
	}
	for _, bad := range []string{"", "@", "user:", "team:", "a b", "user:a:b", "@acme/", strings.Repeat("x", 101)} {
		if _, err := NormalizeOwner(bad); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("NormalizeOwner(%q) err = %v, want ErrInvalid", bad, err)
		}
	}
}

func TestParseOwnersFileAndOwnersOf(t *testing.T) {
	rules, errs := ParseOwnersFile([]byte("# owners\n\n**  @alice\ndocs/  @acme/docs user:bob # trailing\n*.md\nsrc/** @bad!name\nvendor\n**/*.go @carol @carol\n"))
	wantErrs := []string{`OWNERS:5: no owners for "*.md"`, `OWNERS:6: "@bad!name" is not a valid owner`, `OWNERS:7: no owners for "vendor"`}
	if !reflect.DeepEqual(errs, wantErrs) {
		t.Fatalf("errs = %q", errs)
	}
	want := []domain.PathOwnerRule{
		{Pattern: "**", Owners: []string{"user:alice"}},
		{Pattern: "docs/", Owners: []string{"team:docs", "user:bob"}},
		{Pattern: "**/*.go", Owners: []string{"user:carol"}},
	}
	if !reflect.DeepEqual(rules, want) {
		t.Fatalf("rules = %+v", rules)
	}
	for path, owners := range map[string][]string{
		"README":         {"user:alice"},
		"docs/a/b.txt":   {"team:docs", "user:bob"},
		"docs/x.go":      {"user:carol"}, // last match wins
		"main.go":        {"user:carol"},
		"other/file.txt": {"user:alice"},
	} {
		if got := OwnersOf(rules, path); !reflect.DeepEqual(got, owners) {
			t.Errorf("OwnersOf(%s) = %v, want %v", path, got, owners)
		}
	}
	unown := append(slices.Clone(rules), domain.PathOwnerRule{Pattern: "gen/**", Owners: []string{}})
	if got := OwnersOf(unown, "gen/x.txt"); got != nil {
		t.Errorf("an empty rule un-owns: %v", got)
	}
	if got := OwnersOf(nil, "a"); got != nil {
		t.Errorf("no rules: %v", got)
	}
}

func TestOwnersConfigAndFile(t *testing.T) {
	e := newV7Env(t)
	ctx := context.Background()
	cfg, err := e.svc.Owners(ctx, eve, e.tree)
	if err != nil || cfg.Source != OwnersSourceNone || len(cfg.Rules) != 0 {
		t.Fatalf("no owners: %+v %v", cfg, err)
	}
	// OWNERS file on the default branch.
	base := e.commit(t, map[string]string{"OWNERS": "** @bob\ndocs/** @acme/core\nbad\n", "a.txt": fiveLines})
	e.branch(t, "main", base)
	cfg, err = e.svc.Owners(ctx, eve, e.tree)
	if err != nil || cfg.Source != OwnersSourceFile || len(cfg.Rules) != 2 || len(cfg.Errors) != 1 {
		t.Fatalf("file owners: %+v %v", cfg, err)
	}

	// Server configuration: tree:admin, existing principals.
	rules := []domain.PathOwnerRule{{Pattern: "**", Owners: []string{"@carol", "team:core"}}}
	if _, err := e.svc.PutOwners(ctx, alice, e.tree, rules); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("developer PutOwners: %v", err)
	}
	if _, err := e.svc.PutOwners(ctx, carol, e.tree, []domain.PathOwnerRule{{Pattern: "**", Owners: []string{"user:nobody"}}}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("unknown user: %v", err)
	}
	if _, err := e.svc.PutOwners(ctx, carol, e.tree, []domain.PathOwnerRule{{Pattern: "**", Owners: []string{"team:ghosts"}}}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("unknown team: %v", err)
	}
	if _, err := e.svc.PutOwners(ctx, carol, e.tree, []domain.PathOwnerRule{{Pattern: " ", Owners: nil}}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty pattern: %v", err)
	}
	cfg, err = e.svc.PutOwners(ctx, carol, e.tree, rules)
	if err != nil || cfg.Source != OwnersSourceConfig || cfg.UpdatedBy != "carol" ||
		!reflect.DeepEqual(cfg.Rules[0].Owners, []string{"user:carol", "team:core"}) {
		t.Fatalf("PutOwners: %+v %v", cfg, err)
	}
	if !slices.Contains(e.audit.actions(), "owners.update") {
		t.Errorf("audit: %v", e.audit.actions())
	}
	// Removing the configuration falls back to the file.
	cfg, err = e.svc.PutOwners(ctx, carol, e.tree, nil)
	if err != nil || cfg.Source != OwnersSourceFile {
		t.Fatalf("clear: %+v %v", cfg, err)
	}
	// Tree deletion removes the configuration.
	if _, err := e.svc.PutOwners(ctx, carol, e.tree, rules); err != nil {
		t.Fatal(err)
	}
	e.svc.DeleteTreeDocs(ctx, e.tree.ID)
	if cfg, _ := e.svc.Owners(ctx, eve, e.tree); cfg.Source != OwnersSourceFile {
		t.Fatalf("after tree deletion: %+v", cfg)
	}
}

// ownedEnv: main has OWNERS (docs/** → team core, ** → bob); feature
// changes a.txt and docs/guide.md.
func ownedEnv(t *testing.T) (*v7env, string, string) {
	t.Helper()
	e := newV7Env(t)
	files := map[string]string{"OWNERS": "** @bob\ndocs/** @acme/core\n", "a.txt": fiveLines, "docs/guide.md": "guide\n", "c.txt": "c\n"}
	base := e.commit(t, files)
	changed := map[string]string{"OWNERS": files["OWNERS"], "a.txt": "1\n2\nTHREE\n4\n5\n", "docs/guide.md": "guide v2\n", "c.txt": "c\n"}
	feat := e.commit(t, changed, base)
	e.branch(t, "main", base)
	e.branch(t, "feature", feat)
	return e, base, feat
}

func TestOwnersReviewRequestsAndApproval(t *testing.T) {
	e, base, feat := ownedEnv(t)
	ctx := context.Background()
	e.protect(t, domain.BranchProtection{Pattern: "main", RequireOwnerApproval: true})

	mr := e.open(t, "feature")
	if !reflect.DeepEqual(mr.Reviewers, []string{"user:bob", "team:core"}) {
		t.Fatalf("owners requested on open: %v", mr.Reviewers)
	}
	reqs := e.events(domain.EventMRReviewRequested)
	if len(reqs) != 1 {
		t.Fatalf("review_requested events: %+v", reqs)
	}
	want := map[string]any{"number": mr.Number, "reviewers": []string{"user:bob", "team:core"}, "requested_by": "alice"}
	if !reflect.DeepEqual(reqs[0].Payload, want) {
		t.Fatalf("payload = %#v", reqs[0].Payload)
	}

	own, err := e.svc.MROwners(ctx, eve, e.tree, mr.Number)
	if err != nil {
		t.Fatal(err)
	}
	if !own.Required || own.Satisfied || own.Source != OwnersSourceFile ||
		!reflect.DeepEqual(own.Missing, []string{"a.txt", "docs/guide.md"}) || len(own.Paths) != 2 {
		t.Fatalf("owners = %+v", own)
	}
	c := mustChecks(t, e.menv)
	if c.Mergeable || !c.Owners.Required || len(c.Owners.Missing) != 2 {
		t.Fatalf("checks = %+v", c)
	}
	_, err = e.svc.Merge(ctx, carol, e.tree, mr.Number)
	var be *BlockedError
	if !errors.As(err, &be) || be.Code != CodeMergeBlocked || be.Blockers[0].Kind != BlockerOwners ||
		!strings.Contains(be.Message, "docs/guide.md (team:core)") {
		t.Fatalf("merge without owner approval: %v", err)
	}

	// bob owns a.txt; dave (team core) owns docs/**.
	if _, err := e.svc.Approve(ctx, bob, e.tree, mr.Number); err != nil {
		t.Fatal(err)
	}
	own, _ = e.svc.MROwners(ctx, eve, e.tree, mr.Number)
	if !reflect.DeepEqual(own.Missing, []string{"docs/guide.md"}) || !reflect.DeepEqual(own.Paths[0].ApprovedBy, []string{"bob"}) {
		t.Fatalf("after bob: %+v", own)
	}
	if _, err := e.svc.Approve(ctx, dave, e.tree, mr.Number); err != nil {
		t.Fatal(err)
	}
	if own, _ = e.svc.MROwners(ctx, eve, e.tree, mr.Number); !own.Satisfied {
		t.Fatalf("after dave: %+v", own)
	}
	if c := mustChecks(t, e.menv); !c.Mergeable {
		t.Fatalf("checks after owner approvals: %+v", c.Blockers)
	}
	// A new source tip makes the owner approvals stale.
	tip2 := e.commit(t, map[string]string{"OWNERS": "** @bob\ndocs/** @acme/core\n", "a.txt": "1\n2\nTHREE\n4\n5\n", "docs/guide.md": "guide v3\n", "c.txt": "c\n"}, feat)
	e.branch(t, "feature", tip2)
	if own, _ = e.svc.MROwners(ctx, eve, e.tree, mr.Number); own.Satisfied {
		t.Fatalf("stale approvals must not count: %+v", own)
	}
	_ = base
}

func TestOwnersRequestedOnSourcePush(t *testing.T) {
	e, _, feat := ownedEnv(t)
	ctx := context.Background()
	// Only c.txt owned by carol at first (server configuration).
	if _, err := e.svc.PutOwners(ctx, carol, e.tree, []domain.PathOwnerRule{{Pattern: "c.txt", Owners: []string{"user:carol"}}}); err != nil {
		t.Fatal(err)
	}
	mr := e.open(t, "feature")
	if len(mr.Reviewers) != 0 || len(e.events(domain.EventMRReviewRequested)) != 0 {
		t.Fatalf("nothing owned is changed yet: %v", mr.Reviewers)
	}
	tip := e.commit(t, map[string]string{"OWNERS": "x", "a.txt": "1\n", "docs/guide.md": "g\n", "c.txt": "c changed\n"}, feat)
	e.branch(t, "feature", tip)
	e.svc.SourcePushed(ctx, e.tree.ID, "feature", "bob")
	got, _ := e.store.GetMergeRequest(ctx, e.tree.ID, mr.Number)
	if !reflect.DeepEqual(got.Reviewers, []string{"user:carol"}) {
		t.Fatalf("reviewers after push: %v", got.Reviewers)
	}
	reqs := e.events(domain.EventMRReviewRequested)
	if len(reqs) != 1 || reqs[0].Payload.(map[string]any)["requested_by"] != "bob" {
		t.Fatalf("events: %+v", reqs)
	}
	// Pushing again requests nobody new; other branches are ignored.
	e.svc.SourcePushed(ctx, e.tree.ID, "feature", "bob")
	e.svc.SourcePushed(ctx, e.tree.ID, "main", "bob")
	if n := len(e.events(domain.EventMRReviewRequested)); n != 1 {
		t.Fatalf("duplicate requests: %d", n)
	}
	// The author is never requested.
	if _, err := e.svc.PutOwners(ctx, carol, e.tree, []domain.PathOwnerRule{{Pattern: "**", Owners: []string{"user:alice"}}}); err != nil {
		t.Fatal(err)
	}
	e.svc.SourcePushed(ctx, e.tree.ID, "feature", "alice")
	if n := len(e.events(domain.EventMRReviewRequested)); n != 1 {
		t.Fatalf("author requested: %d", n)
	}
}

func TestDraftsAndReviewers(t *testing.T) {
	e, _, _ := ownedEnv(t)
	ctx := context.Background()
	mr, err := e.svc.OpenWith(ctx, alice, e.tree, OpenInput{Title: "WIP", Source: "feature", Target: "main", Draft: true, Reviewers: []string{"@carol"}})
	if err != nil {
		t.Fatal(err)
	}
	if !mr.Draft || !reflect.DeepEqual(mr.Reviewers, []string{"user:carol"}) {
		t.Fatalf("draft: %+v", mr)
	}
	c := mustChecks(t, e.menv)
	if c.Mergeable || c.Blockers[0].Kind != BlockerDraft {
		t.Fatalf("draft checks: %+v", c.Blockers)
	}
	_, err = e.svc.Merge(ctx, carol, e.tree, mr.Number)
	var be *BlockedError
	if !errors.As(err, &be) || be.Code != CodeDraft {
		t.Fatalf("merging a draft: %v", err)
	}
	if _, err := e.svc.OpenWith(ctx, alice, e.tree, OpenInput{Title: "x", Source: "feature", Target: "main", Reviewers: []string{"user:alice"}}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("author as reviewer: %v", err)
	}

	// Reviewers: author or mr:approve; existing users and teams.
	if _, err := e.svc.RequestReviewers(ctx, eve, e.tree, mr.Number, []string{"bob"}); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("eve requests: %v", err)
	}
	if _, err := e.svc.RequestReviewers(ctx, alice, e.tree, mr.Number, []string{"ghost"}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("unknown reviewer: %v", err)
	}
	got, err := e.svc.RequestReviewers(ctx, bob, e.tree, mr.Number, []string{"team:core", "user:carol"})
	if err != nil || !reflect.DeepEqual(got.Reviewers, []string{"user:carol", "team:core"}) {
		t.Fatalf("request: %+v %v", got, err)
	}
	reqs := e.events(domain.EventMRReviewRequested)
	last := reqs[len(reqs)-1].Payload.(map[string]any)
	if !reflect.DeepEqual(last["reviewers"], []string{"team:core"}) || last["requested_by"] != "bob" {
		t.Fatalf("only new reviewers are announced: %#v", last)
	}
	got, err = e.svc.RemoveReviewers(ctx, alice, e.tree, mr.Number, []string{"@carol"})
	if err != nil || !reflect.DeepEqual(got.Reviewers, []string{"team:core"}) {
		t.Fatalf("remove: %+v %v", got, err)
	}

	// Ready: author or mr:merge; requests the owners; mr.ready.
	if _, err := e.svc.Ready(ctx, bob, e.tree, mr.Number); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("bob marks ready: %v", err)
	}
	got, err = e.svc.Ready(ctx, alice, e.tree, mr.Number)
	if err != nil || got.Draft || !reflect.DeepEqual(got.Reviewers, []string{"team:core", "user:bob"}) {
		t.Fatalf("ready: %+v %v", got, err)
	}
	if len(e.events(domain.EventMRReady)) != 1 {
		t.Fatalf("mr.ready: %v", e.bus.types())
	}
	again, err := e.svc.Ready(ctx, alice, e.tree, mr.Number)
	if err != nil || again.Draft || len(e.events(domain.EventMRReady)) != 1 {
		t.Fatalf("ready twice: %v", err)
	}
	if c := mustChecks(t, e.menv); !c.Mergeable {
		t.Fatalf("ready MR: %+v", c.Blockers)
	}
}

func TestApplyLines(t *testing.T) {
	for _, tc := range []struct {
		text       string
		start, end int
		repl, want string
	}{
		{"a\nb\nc\n", 2, 2, "B", "a\nB\nc\n"},
		{"a\nb\nc\n", 1, 3, "x\ny\n", "x\ny\n"},
		{"a\nb\nc\n", 2, 3, "", "a\n"},
		{"a\nb\nc", 3, 3, "C\n", "a\nb\nC"},
		{"a\r\nb\r\n", 1, 1, "A\nA2", "A\r\nA2\r\nb\r\n"},
		{"a\nb\n", 3, 3, "x", "a\nb\n"}, // out of range: unchanged
	} {
		if got := ApplyLines(tc.text, tc.start, tc.end, tc.repl); got != tc.want {
			t.Errorf("ApplyLines(%q, %d, %d, %q) = %q, want %q", tc.text, tc.start, tc.end, tc.repl, got, tc.want)
		}
	}
}

// suggestEnv: feature changes a.txt; alice opens feature → main.
func suggestEnv(t *testing.T) (*v7env, *domain.MergeRequest, string) {
	t.Helper()
	e := newV7Env(t)
	base := e.commit(t, map[string]string{"a.txt": fiveLines, "b.bin": "\x00\x01"})
	feat := e.commit(t, map[string]string{"a.txt": "1\n2\n3\n4\nFIVE\n", "b.bin": "\x00\x01"}, base)
	e.branch(t, "main", base)
	e.branch(t, "feature", feat)
	return e, e.open(t, "feature"), feat
}

func TestSuggestionsApply(t *testing.T) {
	e, mr, feat := suggestEnv(t)
	ctx := context.Background()
	suggest := func(p *domain.Principal, in SuggestionInput) (*domain.ReviewComment, error) {
		return e.svc.AddComment(ctx, p, e.tree, mr.Number, CommentInput{Suggestion: &in})
	}
	for name, in := range map[string]SuggestionInput{
		"missing file":  {Path: "nope.txt", StartLine: 1, EndLine: 1},
		"out of range":  {Path: "a.txt", StartLine: 5, EndLine: 6},
		"bad range":     {Path: "a.txt", StartLine: 3, EndLine: 2},
		"binary":        {Path: "b.bin", StartLine: 1, EndLine: 1},
		"foreign snap":  {Path: "a.txt", StartLine: 1, EndLine: 1, SnapshotID: "nope"},
		"no path given": {StartLine: 1, EndLine: 1},
	} {
		if _, err := suggest(bob, in); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("%s: %v", name, err)
		}
	}
	c, err := suggest(bob, SuggestionInput{Path: "a.txt", StartLine: 2, EndLine: 3, Replacement: "two\nthree"})
	if err != nil {
		t.Fatal(err)
	}
	if c.Path != "a.txt" || c.Line != 2 || c.SnapshotID != feat || c.Body != "" || c.Suggestion.SnapshotID != feat {
		t.Fatalf("comment = %+v", c)
	}

	// eve cannot write; a protected source refuses the push.
	if _, err := e.svc.ApplySuggestion(ctx, eve, e.tree, mr.Number, c.ID, ""); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("eve applies: %v", err)
	}
	e.protect(t, domain.BranchProtection{Pattern: "feature", NoDirectPush: true})
	var denied *domain.PushDenied
	if _, err := e.svc.ApplySuggestion(ctx, alice, e.tree, mr.Number, c.ID, ""); !errors.As(err, &denied) {
		t.Fatalf("protected source: %v", err)
	}
	if err := e.store.DeleteProtection(ctx, e.tree.ID, "feature"); err != nil {
		t.Fatal(err)
	}

	res, err := e.svc.ApplySuggestion(ctx, alice, e.tree, mr.Number, c.ID, "")
	if err != nil {
		t.Fatal(err)
	}
	snap := res.Snapshot
	if e.tip(t, "feature") != snap.ID || !reflect.DeepEqual(snap.Parents, []string{feat}) || snap.Author != "alice" ||
		snap.PushedBy != alice.AccountID || snap.BranchName != "feature" || !strings.Contains(snap.Message, "Apply suggestion to a.txt") {
		t.Fatalf("snapshot = %+v", snap)
	}
	if got := e.files(t, snap.ID)["a.txt"]; got != "1\ntwo\nthree\n4\nFIVE\n" {
		t.Fatalf("a.txt = %q", got)
	}
	if !res.Comment.Resolved || res.Comment.Suggestion.AppliedSnapshot != snap.ID || res.Comment.Suggestion.AppliedBy != "alice" {
		t.Fatalf("comment = %+v", res.Comment)
	}
	applied := e.events(domain.EventSuggestionApplied)
	if len(applied) != 1 || applied[0].Payload.(map[string]any)["snapshot_id"] != snap.ID {
		t.Fatalf("events: %v", e.bus.types())
	}
	pushed := e.events(domain.EventBranchPushed)
	if len(pushed) != 1 || pushed[0].Payload.(map[string]any)["new_tip"] != snap.ID {
		t.Fatalf("branch.pushed: %+v", pushed)
	}
	if _, err := e.svc.ApplySuggestion(ctx, alice, e.tree, mr.Number, c.ID, ""); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("apply twice: %v", err)
	}
	if _, err := e.svc.ApplySuggestion(ctx, alice, e.tree, mr.Number, "999", ""); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown comment: %v", err)
	}
	plain, _ := e.svc.AddComment(ctx, bob, e.tree, mr.Number, CommentInput{Body: "hi"})
	if _, err := e.svc.ApplySuggestion(ctx, alice, e.tree, mr.Number, plain.ID, ""); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("no suggestion: %v", err)
	}
}

func TestSuggestionsOutdatedAndMerged(t *testing.T) {
	e, mr, feat := suggestEnv(t)
	ctx := context.Background()
	add := func(start, end int, repl string) *domain.ReviewComment {
		t.Helper()
		c, err := e.svc.AddComment(ctx, bob, e.tree, mr.Number, CommentInput{Body: "try", Suggestion: &SuggestionInput{Path: "a.txt", StartLine: start, EndLine: end, Replacement: repl}})
		if err != nil {
			t.Fatal(err)
		}
		return c
	}
	onLine1 := add(1, 1, "one")
	onLine3 := add(3, 3, "three")
	same := add(5, 5, "FIVE")
	// The source changes line 3 after the suggestions were made.
	tip := e.commit(t, map[string]string{"a.txt": "1\n2\nTHREE!\n4\nFIVE\n", "b.bin": "\x00\x01"}, feat)
	e.branch(t, "feature", tip)

	_, err := e.svc.ApplySuggestion(ctx, alice, e.tree, mr.Number, onLine3.ID, "")
	var oe *OutdatedError
	if !errors.As(err, &oe) || !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("outdated: %v", err)
	}
	if _, err := e.svc.ApplySuggestion(ctx, alice, e.tree, mr.Number, same.ID, ""); !errors.Is(err, domain.ErrConflict) || errors.As(err, &oe) {
		t.Fatalf("already contained: %v", err)
	}
	// Line 1 is far from the change: merged line-wise.
	res, err := e.svc.ApplySuggestion(ctx, alice, e.tree, mr.Number, onLine1.ID, "custom message")
	if err != nil {
		t.Fatal(err)
	}
	if got := e.files(t, res.Snapshot.ID)["a.txt"]; got != "one\n2\nTHREE!\n4\nFIVE\n" || res.Snapshot.Message != "custom message" {
		t.Fatalf("merged = %q (%s)", got, res.Snapshot.Message)
	}
}

// quotaChecker refuses plans that add a snapshot when deny is set.
type quotaChecker struct {
	deny  bool
	plans []*domain.PushPlan
}

func (q *quotaChecker) CheckPushPlan(_ context.Context, _ *domain.Principal, _ *domain.Tree, plan *domain.PushPlan) error {
	q.plans = append(q.plans, plan)
	if q.deny {
		return &domain.PushDenied{Kind: "quota_exceeded", Reason: "max_storage_bytes"}
	}
	return nil
}

func TestQuotaOnServerMergesAndSuggestions(t *testing.T) {
	e := newV7Env(t)
	ctx := context.Background()
	q := &quotaChecker{deny: true}
	e.svc.SetQuota(q)
	_, mainTip, featTip := e.diverged(t)
	for _, m := range []string{domain.MergeCommit, domain.MergeSquash} {
		_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: m})
		var qe *QuotaError
		if !errors.As(err, &qe) || !errors.Is(err, domain.ErrConflict) || qe.Limit != "max_storage_bytes" {
			t.Fatalf("%s over quota: %v", m, err)
		}
	}
	if e.tip(t, "main") != mainTip {
		t.Fatal("the target must not move")
	}
	last := q.plans[len(q.plans)-1]
	if last.Branch != "main" || last.OldTip != mainTip || !reflect.DeepEqual(last.NewSnapshots[0].Parents, []string{mainTip, featTip}) {
		t.Fatalf("plan = %+v", last)
	}
	// Fast-forwards add no objects: no quota check.
	q.plans = nil
	q.deny = false
	if _, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: domain.MergeSquash}); err != nil {
		t.Fatal(err)
	}
	if len(q.plans) != 1 {
		t.Fatalf("plans = %d", len(q.plans))
	}

	// Suggestions run the push checks (quota only when none are set).
	e2, mr, _ := suggestEnv(t)
	q2 := &quotaChecker{deny: true}
	e2.svc.SetQuota(q2)
	c, err := e2.svc.AddComment(ctx, bob, e2.tree, mr.Number, CommentInput{Suggestion: &SuggestionInput{Path: "a.txt", StartLine: 1, EndLine: 1, Replacement: "x"}})
	if err != nil {
		t.Fatal(err)
	}
	var qe *QuotaError
	if _, err := e2.svc.ApplySuggestion(ctx, alice, e2.tree, mr.Number, c.ID, ""); !errors.As(err, &qe) {
		t.Fatalf("suggestion over quota: %v", err)
	}
	other := &quotaChecker{}
	e2.svc.SetPushChecks(other)
	if _, err := e2.svc.ApplySuggestion(ctx, alice, e2.tree, mr.Number, c.ID, ""); err != nil {
		t.Fatal(err)
	}
	if len(other.plans) != 1 || !reflect.DeepEqual(other.plans[0].ChangedPaths, []string{"a.txt"}) || len(other.plans[0].NewObjects) != 2 {
		t.Fatalf("push check plan = %+v", other.plans)
	}
}
