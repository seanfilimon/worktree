package policyhist

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

type recBus struct {
	mu     sync.Mutex
	events []domain.Event
}

func (b *recBus) Publish(e domain.Event) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.events = append(b.events, e)
}

func (b *recBus) Subscribe(domain.EventFilter) (<-chan domain.Event, func()) {
	return make(chan domain.Event), func() {}
}

func st(sid, effect string, actions ...string) domain.PolicyStatement {
	return domain.PolicyStatement{Sid: sid, Effect: effect, Actions: actions, Resources: []string{"*"}, Principals: []string{"*"}}
}

func pol(id, name string, stmts ...domain.PolicyStatement) domain.Policy {
	return domain.Policy{ID: id, TenantID: "t1", Name: name, Enabled: true, Statements: stmts, CreatedBy: "ada"}
}

func changeList(vs []domain.PolicyVersion) string {
	var parts []string
	for _, v := range vs {
		parts = append(parts, v.Change)
	}
	return strings.Join(parts, ",")
}

func TestRecordAndHistory(t *testing.T) {
	ctx := context.Background()
	store := meta.NewMemory()
	bus := &recBus{}
	r := New(store, bus, nil)

	p := pol("p1", "guard", st("a", "deny", "tree:write"))
	v, err := r.Record(ctx, Entry{Change: ChangeCreate, Actor: "ada", Policy: p})
	if err != nil || v.Version != 1 || v.ChangedBy != "ada" {
		t.Fatalf("create = %+v, %v", v, err)
	}
	before := p
	p.Enabled = false
	if v, err = r.Record(ctx, Entry{Change: ChangeUpdate, Actor: "bob", Before: &before, Policy: p, Comment: "why"}); err != nil || v.Version != 2 {
		t.Fatalf("update = %+v, %v", v, err)
	}
	vs, err := r.History(ctx, "t1", "p1", &p)
	if err != nil || changeList(vs) != "create,update" || vs[1].Comment != "why" || vs[1].Policy.Enabled {
		t.Fatalf("history = %+v, %v", vs, err)
	}
	if _, err := r.Find(ctx, "t1", "p1", 3, &p); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("find 3 = %v", err)
	}
	if len(bus.events) != 2 || bus.events[1].Type != domain.EventPolicyChanged || bus.events[1].Actor != "bob" {
		t.Fatalf("events = %+v", bus.events)
	}
	if pl := bus.events[1].Payload.(map[string]any); pl["version"] != 2 || pl["comment"] != "why" || pl["policy_id"] != "p1" {
		t.Fatalf("payload = %+v", pl)
	}
	// Other policies and tenants are separate histories.
	if vs, _ := r.Versions(ctx, "t2", "p1"); len(vs) != 0 {
		t.Fatalf("t2 = %+v", vs)
	}
	if _, err := r.Record(ctx, Entry{Change: ChangeCreate, Policy: domain.Policy{ID: "x"}}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("missing tenant = %v", err)
	}
}

func TestSyntheticVersion(t *testing.T) {
	ctx := context.Background()
	r := New(meta.NewMemory(), nil, nil)
	at := time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)
	old := pol("old", "legacy", st("", "deny", "tree:write"))
	old.UpdatedAt = at

	// Reading a policy without history stores version 1 once.
	vs, err := r.History(ctx, "t1", "old", &old)
	if err != nil || len(vs) != 1 || vs[0].Comment != SyntheticComment || vs[0].Change != ChangeCreate || !vs[0].ChangedAt.Equal(at) || vs[0].ChangedBy != "ada" {
		t.Fatalf("synthetic = %+v, %v", vs, err)
	}
	if vs, _ := r.History(ctx, "t1", "old", &old); len(vs) != 1 {
		t.Fatalf("second read = %+v", vs)
	}
	// Unknown and deleted-without-history policies have no history.
	if vs, err := r.History(ctx, "t1", "nope", nil); err != nil || len(vs) != 0 {
		t.Fatalf("unknown = %+v, %v", vs, err)
	}

	// The first recorded change of a policy without history stores the
	// synthetic version from Before first.
	other := pol("other", "o")
	next := other
	next.Description = "changed"
	v, err := r.Record(ctx, Entry{Change: ChangeDelete, Before: &other, Policy: other})
	if err != nil || v.Version != 2 {
		t.Fatalf("delete = %+v, %v", v, err)
	}
	vs, _ = r.Versions(ctx, "t1", "other")
	if changeList(vs) != "create,delete" || vs[0].Comment != SyntheticComment {
		t.Fatalf("versions = %+v", vs)
	}
}

func TestConcurrentRecords(t *testing.T) {
	ctx := context.Background()
	r := New(meta.NewMemory(), nil, nil)
	p := pol("p", "p")
	var wg sync.WaitGroup
	errs := make(chan error, 6)
	for i := 0; i < 6; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := r.Record(ctx, Entry{Change: ChangeUpdate, Before: &p, Policy: p})
			errs <- err
		}()
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatal(err)
		}
	}
	vs, _ := r.Versions(ctx, "t1", "p")
	if len(vs) != 7 {
		t.Fatalf("versions = %d", len(vs))
	}
	for i, v := range vs {
		if v.Version != i+1 {
			t.Fatalf("versions not dense: %+v", vs)
		}
	}
}

func TestDeleted(t *testing.T) {
	ctx := context.Background()
	r := New(meta.NewMemory(), nil, nil)
	clock := time.Date(2026, 5, 1, 0, 0, 0, 0, time.UTC)
	r.now = func() time.Time { clock = clock.Add(time.Minute); return clock }
	a, b, c := pol("a", "A"), pol("b", "B"), pol("c", "C")
	for _, p := range []domain.Policy{a, b, c} {
		if _, err := r.Record(ctx, Entry{Change: ChangeCreate, Policy: p}); err != nil {
			t.Fatal(err)
		}
	}
	for _, p := range []domain.Policy{a, b} {
		if _, err := r.Record(ctx, Entry{Change: ChangeDelete, Before: &p, Policy: p}); err != nil {
			t.Fatal(err)
		}
	}
	// b is restored again.
	if _, err := r.Record(ctx, Entry{Change: ChangeRollback, Policy: b}); err != nil {
		t.Fatal(err)
	}
	if _, err := r.Record(ctx, Entry{Change: ChangeDelete, Policy: domain.Policy{ID: "z", TenantID: "t2", Name: "Z"}}); err != nil {
		t.Fatal(err)
	}
	del, err := r.Deleted(ctx, "t1")
	if err != nil || len(del) != 1 || del[0].PolicyID != "a" || del[0].Version != 2 {
		t.Fatalf("deleted = %+v, %v", del, err)
	}
}

func TestTrack(t *testing.T) {
	ctx := context.Background()
	store := meta.NewMemory()
	r := New(store, nil, nil)
	mk := func(name, managedBy string) *domain.Policy {
		p := &domain.Policy{TenantID: "t1", Name: name, Enabled: true, ManagedBy: managedBy, Statements: []domain.PolicyStatement{}}
		if err := store.CreatePolicy(ctx, p); err != nil {
			t.Fatal(err)
		}
		return p
	}
	keep := mk("keep", domain.ManagedByConfig)
	change := mk("change", domain.ManagedByConfig)
	gone := mk("gone", domain.ManagedByConfig)
	adopt := mk("adopt", "")
	rest := mk("rest", "")

	err := r.Track(ctx, "olga", "t1", "applied", func() error {
		c := *change
		c.Description = "new"
		if err := store.UpdatePolicy(ctx, &c); err != nil {
			return err
		}
		a := *adopt
		a.ManagedBy = domain.ManagedByConfig
		if err := store.UpdatePolicy(ctx, &a); err != nil {
			return err
		}
		if err := store.DeletePolicy(ctx, "t1", gone.ID); err != nil {
			return err
		}
		mk("fresh", domain.ManagedByConfig)
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	all, _ := meta.ListDocsT[domain.PolicyVersion](ctx, store, Collection, "")
	got := map[string]string{}
	for _, v := range all {
		got[v.Policy.Name] += v.Change + ";"
		if v.ChangedBy != "olga" && v.Comment != SyntheticComment {
			t.Fatalf("attribution = %+v", v)
		}
	}
	want := map[string]string{
		"change": "create;update;", // synthetic + update
		"adopt":  "create;update;",
		"gone":   "create;delete;",
		"fresh":  "create;",
	}
	for name, w := range want {
		if got[name] != w {
			t.Errorf("%s: %q, want %q (all %v)", name, got[name], w, got)
		}
	}
	if got[keep.Name] != "" || got[rest.Name] != "" {
		t.Fatalf("unchanged policies recorded: %v", got)
	}

	// A failing apply records nothing and returns its error.
	boom := errors.New("boom")
	if err := r.Track(ctx, "olga", "t1", "", func() error { mk("never", domain.ManagedByConfig); return boom }); !errors.Is(err, boom) {
		t.Fatalf("err = %v", err)
	}
	if vs, _ := meta.ListDocsT[domain.PolicyVersion](ctx, store, Collection, ""); len(vs) != len(all) {
		t.Fatalf("failed apply recorded versions")
	}
}

func TestDiffSemantics(t *testing.T) {
	from := pol("p", "n",
		st("keep", "deny", "tree:write"),
		st("edit", "deny", "tree:write"),
		st("", "allow", "tree:read"), // anonymous #0
		st("gone", "deny", "x:y"),
		st("", "deny", "tree:admin"), // anonymous #1
		st("dup", "deny", "a:1"),
		st("dup", "deny", "a:2"),
	)
	to := pol("p", "renamed",
		st("new", "deny", "mr:merge"),           // inserted before everything: positions of sid-less ones unaffected
		st("", "allow", "tree:read"),            // anonymous #0 unchanged
		st("keep", "DENY", "tree:write"),        // effect case is canonical
		st("edit", "deny", "tree:write", "b:c"), // changed
		st("", "deny", "tree:admin", "iam:*"),   // anonymous #1 changed
		st("dup", "deny", "a:1"),
		st("dup", "deny", "a:3"), // second occurrence changed
		st("", "deny", "extra"),  // anonymous #2 added
	)
	to.Description = "d"
	changes, fields := Diff(&from, &to)
	var got []string
	for _, c := range changes {
		got = append(got, c.Op+":"+c.Sid+":"+itoa(c.Index))
	}
	want := "add:new:0,change:edit:3,change::4,change:dup:6,add::7,remove:gone:3"
	if strings.Join(got, ",") != want {
		t.Fatalf("changes = %s\nwant      %s", strings.Join(got, ","), want)
	}
	for _, c := range changes {
		switch c.Op {
		case OpAdd, OpRemove:
			if c.Statement == nil || c.Before != nil || c.After != nil {
				t.Fatalf("add/remove shape: %+v", c)
			}
		case OpChange:
			if c.Statement != nil || c.Before == nil || c.After == nil {
				t.Fatalf("change shape: %+v", c)
			}
		}
	}
	if len(fields) != 2 || fields[0].Field != "name" || fields[0].Before != "n" || fields[0].After != "renamed" || fields[1].Field != "description" {
		t.Fatalf("fields = %+v", fields)
	}

	// Empty slices, nil slices and UTC offsets compare equal.
	berlin := time.FixedZone("CET", 3600)
	at := time.Date(2026, 1, 1, 12, 0, 0, 0, berlin)
	atUTC := at.UTC()
	a := domain.PolicyStatement{Effect: "deny", Actions: []string{"x:y"}, Resources: []string{}, Principals: nil,
		Conditions: domain.PolicyConditions{IPRanges: []string{}, NotBefore: &at}}
	b := domain.PolicyStatement{Effect: " deny", Actions: []string{"x:y"}, Conditions: domain.PolicyConditions{NotBefore: &atUTC}}
	pa, pb := pol("p", "n", a), pol("p", "n", b)
	if c, f := Diff(&pa, &pb); len(c) != 0 || len(f) != 0 || !SameContent(&pa, &pb) {
		t.Fatalf("canonical diff = %+v %+v", c, f)
	}

	// nil sides are the empty policy.
	c, f := Diff(nil, &pa)
	if len(c) != 1 || c[0].Op != OpAdd || len(f) != 3 || f[2].Before != nil || f[2].After != true {
		t.Fatalf("from empty = %+v %+v", c, f)
	}
	c, f = Diff(&pa, nil)
	if len(c) != 1 || c[0].Op != OpRemove || len(f) != 3 || f[0].After != nil {
		t.Fatalf("to empty = %+v %+v", c, f)
	}
	if c, f := Diff(nil, nil); c == nil || f == nil || len(c)+len(f) != 0 {
		t.Fatalf("nil/nil = %+v %+v", c, f)
	}
}

func itoa(n int) string {
	if n < 10 {
		return string(rune('0' + n))
	}
	return itoa(n/10) + string(rune('0'+n%10))
}
