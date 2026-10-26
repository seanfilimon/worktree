package review

// Tests of the v0.8 conflict report and browser conflict resolution
// (§16.9, resolve.go).

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

const (
	conflictBase   = "1\n2\n3\n4\n5\n"
	conflictMain   = "1\n2\nTHREE main\n4\n5\n"
	conflictSource = "1\n2\nthree feature\n4\n5\n"
)

// conflicted builds base ← main and base ← feature with three conflicts:
// a.txt (content), bin.dat (binary) and gone.txt (modify_delete: feature
// deleted it, main changed it), and opens feature → main as !1.
func conflicted(t *testing.T) (e *menv, base, mainTip, featTip string) {
	t.Helper()
	e = newMergeEnv(t)
	base = e.commit(t, map[string]string{"a.txt": conflictBase, "b.txt": "b\n", "bin.dat": "\x00\x01", "gone.txt": "keep\n", "run.sh*": "#!/bin/sh\n"})
	mainTip = e.commit(t, map[string]string{"a.txt": conflictMain, "b.txt": "b\n", "bin.dat": "\x00\x02", "gone.txt": "keep main\n", "m.txt": "from main\n", "run.sh*": "#!/bin/sh\n"}, base)
	featTip = e.commit(t, map[string]string{"a.txt": conflictSource, "b.txt": "b\n", "bin.dat": "\x00\x03", "f.txt": "from feature\n", "run.sh*": "#!/bin/sh\n"}, base)
	e.branch(t, "main", mainTip)
	e.branch(t, "feature", featTip)
	e.open(t, "feature")
	return e, base, mainTip, featTip
}

func strp(s string) *string { return &s }

func TestConflictReport(t *testing.T) {
	e, base, mainTip, featTip := conflicted(t)
	ctx := context.Background()
	rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if !rep.Resolvable || rep.Reason != "" || rep.Total != 3 || rep.Truncated || rep.SourceTip != featTip ||
		rep.TargetTip != mainTip || rep.MergeBase != base || rep.SourceBranch != "feature" || rep.State != domain.MRStateOpen {
		t.Fatalf("report = %+v", rep)
	}
	paths := []string{}
	for _, f := range rep.Files {
		paths = append(paths, f.Path+":"+f.Kind)
	}
	if !reflect.DeepEqual(paths, []string{"a.txt:content", "bin.dat:binary", "gone.txt:modify_delete"}) {
		t.Fatalf("files = %v", paths)
	}
	a := rep.Files[0]
	if a.Binary || a.TooLarge || a.Base.Text != conflictBase || a.Ours.Text != conflictSource || a.Theirs.Text != conflictMain ||
		!a.Ours.Exists || a.Ours.Size != uint64(len(conflictSource)) || a.Ours.Hash == "" {
		t.Fatalf("a.txt = %+v", a)
	}
	wantMarkers := "1\n2\n<<<<<<< feature\nthree feature\n||||||| base\n3\n=======\nTHREE main\n>>>>>>> main\n4\n5\n"
	if a.Markers != wantMarkers || len(a.Regions) != 1 || a.Regions[0].StartLine != 3 ||
		!reflect.DeepEqual(a.Regions[0].Ours, []string{"three feature"}) {
		t.Fatalf("markers = %q regions = %+v", a.Markers, a.Regions)
	}
	bin := rep.Files[1]
	if !bin.Binary || bin.Ours.Text != "" || bin.Markers != "" || !bin.Theirs.Exists {
		t.Fatalf("bin.dat = %+v", bin)
	}
	gone := rep.Files[2]
	if gone.Ours.Exists || !gone.Theirs.Exists || gone.Theirs.Text != "keep main\n" || gone.Base.Text != "keep\n" || gone.Markers != "" {
		t.Fatalf("gone.txt = %+v", gone)
	}

	// Readers only: tree:read is required.
	e.svc.authz = &fakeAuthz{grants: map[string][]domain.Action{}}
	if _, err := e.svc.Conflicts(ctx, eve, e.tree, 1); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("no tree:read: %v", err)
	}
	e.svc.authz = &fakeAuthz{grants: map[string][]domain.Action{"eve": {domain.ActTreeRead}}}
	if _, err := e.svc.Conflicts(ctx, eve, e.tree, 9); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown MR: %v", err)
	}
}

func TestConflictReportLimitsAndReasons(t *testing.T) {
	ctx := context.Background()
	t.Run("too large", func(t *testing.T) {
		e := newMergeEnv(t)
		big := strings.Repeat("x\n", MaxConflictText/2+1)
		base := e.commit(t, map[string]string{"big.txt": big})
		mainTip := e.commit(t, map[string]string{"big.txt": "main\n" + big}, base)
		featTip := e.commit(t, map[string]string{"big.txt": "feature\n" + big}, base)
		e.branch(t, "main", mainTip)
		e.branch(t, "feature", featTip)
		e.open(t, "feature")
		rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
		if err != nil {
			t.Fatal(err)
		}
		if len(rep.Files) != 1 || !rep.Files[0].TooLarge || rep.Files[0].Ours.Text != "" || rep.Files[0].Markers != "" {
			t.Fatalf("report = %+v", rep.Files)
		}
	})
	t.Run("no conflicts", func(t *testing.T) {
		e := newMergeEnv(t)
		e.diverged(t)
		rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
		if err != nil {
			t.Fatal(err)
		}
		if rep.Resolvable || !strings.Contains(rep.Reason, "no conflicts") || rep.Total != 0 || len(rep.Files) != 0 {
			t.Fatalf("report = %+v", rep)
		}
		if _, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, ResolveInput{Files: []ResolveFile{{Path: "a.txt", Take: TakeOurs}}}); !errors.Is(err, domain.ErrConflict) {
			t.Fatalf("resolve without conflicts: %v", err)
		}
	})
	t.Run("fast-forward", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"a.txt": "a\n"})
		feat := e.commit(t, map[string]string{"a.txt": "b\n"}, base)
		e.branch(t, "main", base)
		e.branch(t, "feature", feat)
		e.open(t, "feature")
		rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
		if err != nil || rep.Resolvable || rep.MergeBase != base || !strings.Contains(rep.Reason, "fast-forwarded") {
			t.Fatalf("report = %+v, %v", rep, err)
		}
	})
	t.Run("closed", func(t *testing.T) {
		e, _, _, _ := conflicted(t)
		if _, err := e.svc.Close(ctx, alice, e.tree, 1); err != nil {
			t.Fatal(err)
		}
		rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
		if err != nil || rep.Resolvable || rep.Total != 3 || !strings.Contains(rep.Reason, "closed") {
			t.Fatalf("report = %+v, %v", rep, err)
		}
	})
	t.Run("fork source", func(t *testing.T) {
		e, _, _, _ := conflicted(t)
		mr, _ := e.store.GetMergeRequest(ctx, e.tree.ID, 1)
		mr.SourceTreeID = "tree-fork"
		if err := e.store.UpdateMergeRequest(ctx, mr); err != nil {
			t.Fatal(err)
		}
		rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
		if err != nil || rep.Resolvable || !strings.Contains(rep.Reason, "fork") {
			t.Fatalf("report = %+v, %v", rep, err)
		}
	})
	t.Run("truncated", func(t *testing.T) {
		e := newMergeEnv(t)
		bf, mf, ff := map[string]string{}, map[string]string{}, map[string]string{}
		for i := 0; i < MaxConflictFiles+5; i++ {
			p := fmt.Sprintf("f%03d.txt", i)
			bf[p], mf[p], ff[p] = "base\n", "main\n", "feature\n"
		}
		base := e.commit(t, bf)
		mainTip := e.commit(t, mf, base)
		featTip := e.commit(t, ff, base)
		e.branch(t, "main", mainTip)
		e.branch(t, "feature", featTip)
		e.open(t, "feature")
		rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
		if err != nil || !rep.Truncated || rep.Total != MaxConflictFiles+5 || len(rep.Files) != MaxConflictFiles {
			t.Fatalf("total=%d files=%d truncated=%v err=%v", rep.Total, len(rep.Files), rep.Truncated, err)
		}
	})
}

func goodResolution() ResolveInput {
	return ResolveInput{Files: []ResolveFile{
		{Path: "a.txt", Content: strp("1\n2\nthree, resolved\n4\n5\n")},
		{Path: "bin.dat", Take: TakeTheirs},
		{Path: "gone.txt", Delete: true},
	}}
}

func TestResolveConflicts(t *testing.T) {
	e, _, mainTip, featTip := conflicted(t)
	ctx := context.Background()
	in := goodResolution()
	in.SourceTip, in.TargetTip = featTip, mainTip
	res, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, in)
	if err != nil {
		t.Fatal(err)
	}
	snap := res.Snapshot
	if !reflect.DeepEqual(snap.Parents, []string{featTip, mainTip}) || snap.BranchName != "feature" || snap.Author != "alice" ||
		snap.PushedBy != alice.AccountID || snap.Message != "Merge branch 'main' into feature (!1, conflicts resolved)" ||
		len(snap.Signature) != 0 {
		t.Fatalf("snapshot = %+v", snap)
	}
	if !reflect.DeepEqual(res.Paths, []string{"a.txt", "bin.dat", "gone.txt"}) || res.MergeRequest.Number != 1 {
		t.Fatalf("result = %+v", res)
	}
	if e.tip(t, "feature") != snap.ID || e.tip(t, "main") != mainTip {
		t.Fatal("only the source branch moves")
	}
	want := map[string]string{
		"a.txt": "1\n2\nthree, resolved\n4\n5\n", "b.txt": "b\n", "bin.dat": "\x00\x02",
		"m.txt": "from main\n", "f.txt": "from feature\n", "run.sh*": "#!/bin/sh\n",
	}
	if got := e.files(t, snap.ID); !reflect.DeepEqual(got, want) {
		t.Fatalf("files = %#v", got)
	}
	types := e.bus.types()
	if !slices.Contains(types, domain.EventConflictsSolved) || !slices.Contains(types, domain.EventBranchPushed) {
		t.Fatalf("events = %v", types)
	}
	var solved domain.Event
	for _, ev := range e.bus.events {
		if ev.Type == domain.EventConflictsSolved {
			solved = ev
		}
	}
	payload := solved.Payload.(map[string]any)
	if payload["snapshot_id"] != snap.ID || payload["resolved_by"] != "alice" || payload["number"] != 1 {
		t.Fatalf("payload = %#v", payload)
	}
	if !slices.Contains(e.audit.actions(), "mr.resolve_conflicts") {
		t.Fatalf("audit = %v", e.audit.actions())
	}

	// The merge request now fast-forwards cleanly.
	mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
	if err != nil || !mb.FastForward || len(mb.Conflicts) != 0 || mb.Default != domain.MergeFastForward {
		t.Fatalf("mergeability = %+v, %v", mb, err)
	}
	rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
	if err != nil || rep.Resolvable || rep.Total != 0 {
		t.Fatalf("after: %+v, %v", rep, err)
	}
	mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
	if err != nil || mr.MergedSnapshot != snap.ID {
		t.Fatalf("merge: %+v, %v", mr, err)
	}
}

func TestResolveConflictsValidation(t *testing.T) {
	ctx := context.Background()
	e, _, mainTip, featTip := conflicted(t)
	try := func(p *domain.Principal, in ResolveInput) error {
		_, err := e.svc.ResolveConflicts(ctx, p, e.tree, 1, in)
		return err
	}
	if err := try(eve, goodResolution()); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("reader: %v", err)
	}
	invalid := map[string]ResolveInput{
		"empty":     {},
		"missing":   {Files: goodResolution().Files[:2]},
		"unknown":   {Files: append(goodResolution().Files, ResolveFile{Path: "b.txt", Take: TakeOurs})},
		"twice":     {Files: append(goodResolution().Files, ResolveFile{Path: "a.txt", Take: TakeOurs})},
		"two ways":  {Files: []ResolveFile{{Path: "a.txt", Take: TakeOurs, Delete: true}, {Path: "bin.dat", Take: TakeOurs}, {Path: "gone.txt", Delete: true}}},
		"no way":    {Files: []ResolveFile{{Path: "a.txt"}, {Path: "bin.dat", Take: TakeOurs}, {Path: "gone.txt", Delete: true}}},
		"bad take":  {Files: []ResolveFile{{Path: "a.txt", Take: "mine"}, {Path: "bin.dat", Take: TakeOurs}, {Path: "gone.txt", Delete: true}}},
		"markers":   {Files: []ResolveFile{{Path: "a.txt", Content: strp("<<<<<<< feature\nx\n=======\ny\n>>>>>>> main\n")}, {Path: "bin.dat", Take: TakeOurs}, {Path: "gone.txt", Delete: true}}},
		"not utf8":  {Files: []ResolveFile{{Path: "a.txt", Content: strp("\xff")}, {Path: "bin.dat", Take: TakeOurs}, {Path: "gone.txt", Delete: true}}},
		"exec take": {Files: []ResolveFile{{Path: "a.txt", Take: TakeOurs, Executable: new(bool)}, {Path: "bin.dat", Take: TakeOurs}, {Path: "gone.txt", Delete: true}}},
		"message":   {Files: goodResolution().Files, Message: "a\x00b"},
		"no path":   {Files: []ResolveFile{{Path: "", Delete: true}}},
	}
	for name, in := range invalid {
		if err := try(alice, in); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("%s: err = %v", name, err)
		}
	}
	// Markers may be kept on request.
	kept := goodResolution()
	kept.Files[0].Content = strp("<<<<<<< feature\nx\n=======\ny\n>>>>>>> main\n")
	kept.AllowMarkers = true
	kept.SourceTip = "stale"
	var oe *OutdatedResolveError
	if err := try(alice, kept); !errors.As(err, &oe) || !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("stale source tip: %v", err)
	}
	kept.SourceTip, kept.TargetTip = featTip, "stale"
	if err := try(alice, kept); !errors.As(err, &oe) {
		t.Fatalf("stale target tip: %v", err)
	}
	if e.tip(t, "feature") != featTip {
		t.Fatal("refused resolutions must not move the source")
	}
	kept.TargetTip = mainTip
	if err := try(alice, kept); err != nil {
		t.Fatalf("allow_markers: %v", err)
	}
}

func TestResolveConflictsProtectionAndChecks(t *testing.T) {
	ctx := context.Background()
	t.Run("source protection", func(t *testing.T) {
		e, _, _, _ := conflicted(t)
		e.protect(t, domain.BranchProtection{Pattern: "feature", NoDirectPush: true})
		_, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, goodResolution())
		var denied *domain.PushDenied
		if !errors.As(err, &denied) || denied.Kind != "branch_protection" {
			t.Fatalf("protected source: %v", err)
		}
	})
	t.Run("push checks", func(t *testing.T) {
		e, _, mainTip, featTip := conflicted(t)
		q := &quotaChecker{}
		e.svc.SetPushChecks(q)
		if _, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, goodResolution()); err != nil {
			t.Fatal(err)
		}
		if len(q.plans) != 1 {
			t.Fatalf("plans = %d", len(q.plans))
		}
		plan := q.plans[0]
		if plan.Branch != "feature" || plan.OldTip != featTip || !reflect.DeepEqual(plan.NewSnapshots[0].Parents, []string{featTip, mainTip}) ||
			!reflect.DeepEqual(plan.ChangedPaths, []string{"a.txt", "bin.dat", "m.txt"}) || len(plan.NewObjects) != 2 {
			t.Fatalf("plan = %+v", plan)
		}
	})
	t.Run("quota", func(t *testing.T) {
		e, _, _, featTip := conflicted(t)
		e.svc.SetQuota(&quotaChecker{deny: true})
		_, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, goodResolution())
		var qe *QuotaError
		if !errors.As(err, &qe) {
			t.Fatalf("over quota: %v", err)
		}
		if e.tip(t, "feature") != featTip {
			t.Fatal("source moved")
		}
	})
	t.Run("signing", func(t *testing.T) {
		e, _, _, _ := conflicted(t)
		e.svc.SetPushChecks(denyChecker{kind: "signature_required"})
		_, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, goodResolution())
		var denied *domain.PushDenied
		if !errors.As(err, &denied) || denied.Kind != "signature_required" {
			t.Fatalf("signing: %v", err)
		}
	})
	t.Run("concurrent push", func(t *testing.T) {
		e, base, _, _ := conflicted(t)
		e.store.beforeCAS = func() { e.branch(t, "feature", base) }
		_, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, goodResolution())
		var oe *OutdatedResolveError
		if !errors.As(err, &oe) {
			t.Fatalf("moved source: %v", err)
		}
	})
	t.Run("take sides", func(t *testing.T) {
		e, _, _, _ := conflicted(t)
		in := ResolveInput{Files: []ResolveFile{
			{Path: "a.txt", Take: TakeBase},
			{Path: "bin.dat", Take: TakeOurs},
			{Path: "gone.txt", Take: TakeOurs}, // ours deleted it
		}}
		res, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, in)
		if err != nil {
			t.Fatal(err)
		}
		got := e.files(t, res.Snapshot.ID)
		if got["a.txt"] != conflictBase || got["bin.dat"] != "\x00\x03" || got["gone.txt"] != "" || len(got) != 6 {
			t.Fatalf("files = %#v", got)
		}
	})
}

// denyChecker refuses every plan with a PushDenied of kind.
type denyChecker struct{ kind string }

func (d denyChecker) CheckPushPlan(context.Context, *domain.Principal, *domain.Tree, *domain.PushPlan) error {
	return &domain.PushDenied{Kind: d.kind, Reason: "denied"}
}

func TestResolveConflictsExecutableAndAddAdd(t *testing.T) {
	ctx := context.Background()
	e := newMergeEnv(t)
	base := e.commit(t, map[string]string{"x": "x\n"})
	mainTip := e.commit(t, map[string]string{"x": "x\n", "new.sh": "main\n"}, base)
	featTip := e.commit(t, map[string]string{"x": "x\n", "new.sh*": "feature\n"}, base)
	e.branch(t, "main", mainTip)
	e.branch(t, "feature", featTip)
	e.open(t, "feature")
	rep, err := e.svc.Conflicts(ctx, eve, e.tree, 1)
	if err != nil || len(rep.Files) != 1 || rep.Files[0].Kind != "add_add" || rep.Files[0].Base.Exists ||
		!strings.Contains(rep.Files[0].Markers, "<<<<<<< feature\nfeature\n") || !rep.Files[0].Ours.Executable {
		t.Fatalf("report = %+v, %v", rep, err)
	}
	// Content keeps ours' executable bit unless told otherwise.
	res, err := e.svc.ResolveConflicts(ctx, alice, e.tree, 1, ResolveInput{Files: []ResolveFile{{Path: "new.sh", Content: strp("both\n")}}})
	if err != nil {
		t.Fatal(err)
	}
	if got := e.files(t, res.Snapshot.ID); got["new.sh*"] != "both\n" {
		t.Fatalf("files = %#v", got)
	}
}

func TestHasConflictMarkers(t *testing.T) {
	for text, want := range map[string]bool{
		"":                                      false,
		"plain\n":                               false,
		"<<<<<<< a\nx\n=======\ny\n>>>>>>> b\n": true,
		"<<<<<<< a\r\nx\r\n=======\r\ny\r\n>>>>>>>": true,
		"<<<<<<< only the opening marker\n":         false,
		"=======\n>>>>>>> b\n":                      false,
		"docs: the ======= line\n":                  false,
	} {
		if got := HasConflictMarkers(text); got != want {
			t.Errorf("HasConflictMarkers(%q) = %v", text, got)
		}
	}
}

func TestFileDirectoryClash(t *testing.T) {
	entries := func(paths ...string) []*syncpb.ManifestEntry {
		out := make([]*syncpb.ManifestEntry, len(paths))
		for i, p := range paths {
			out[i] = &syncpb.ManifestEntry{Path: p}
		}
		return out
	}
	if got := fileDirectoryClash(entries("a", "a/b")); got != "a" {
		t.Fatalf("a vs a/b: %q", got)
	}
	if got := fileDirectoryClash(entries("x/y", "x/y/z/w")); got != "x/y" {
		t.Fatalf("nested: %q", got)
	}
	if got := fileDirectoryClash(entries("a.txt", "a/b", "ab")); got != "" {
		t.Fatalf("no clash: %q", got)
	}
}
