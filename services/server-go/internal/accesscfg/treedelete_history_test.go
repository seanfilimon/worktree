package accesscfg

// hardening (§15.10): policy versions recorded when a tree deletion
// removes its config-managed policies.

import (
	"context"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/policyhist"
)

func TestTreeDeletionRecordsPolicyVersions(t *testing.T) {
	e := newEnv(t)
	tdoc := Document{Policies: []domain.Policy{
		{Name: "freeze", Enabled: true, Statements: []domain.PolicyStatement{deny(l("branch:push"), l("wt:branch/acme/web/main"), l("*"))}},
		{Name: "secret", Enabled: true, Statements: []domain.PolicyStatement{deny(l("tree:read"), l("wt:path/acme/web/secret/**"), l("role:viewer"))}},
	}}
	if res := e.apply("olga", e.web, tdoc, false); !res.Accepted {
		t.Fatalf("tree apply = %+v", res)
	}
	// An unrelated tenant policy must not get a version from the deletion.
	e.apply("olga", nil, baseDoc(), false)
	pols := e.policies()
	freeze, secret, tenantPol := pols["web/freeze"], pols["web/secret"], pols["protect-main"]
	if freeze.ID == "" || secret.ID == "" || tenantPol.ID == "" {
		t.Fatalf("policies = %v", pols)
	}

	ch, cancel := e.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventPolicyChanged}})
	defer cancel()

	ctx, stop := context.WithCancel(e.ctx)
	defer stop()
	done := make(chan struct{})
	go func() { e.svc.Run(ctx); close(done) }()
	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, ok := e.policies()["web/freeze"]; !ok {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("Run did not clean up the deleted tree")
		}
		e.bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: e.acme.ID, TreeID: e.web.ID, Actor: "olga",
			Payload: map[string]any{"name": "web"}})
		time.Sleep(20 * time.Millisecond)
	}
	stop()
	<-done

	rec := policyhist.New(e.store, nil, nil)
	for _, p := range []domain.Policy{freeze, secret} {
		vs, err := rec.Versions(e.ctx, e.acme.ID, p.ID)
		e.must(err)
		if len(vs) != 2 || vs[0].Change != policyhist.ChangeCreate {
			t.Fatalf("%s versions = %+v", p.Name, vs)
		}
		last := vs[1]
		if last.Change != policyhist.ChangeDelete || last.Version != 2 || last.ChangedBy != "olga" ||
			last.Comment != "tree web deleted" || last.Policy.Name != p.Name || last.Policy.TreeID != e.web.ID {
			t.Fatalf("%s delete version = %+v", p.Name, last)
		}
	}
	if vs, err := rec.Versions(e.ctx, e.acme.ID, tenantPol.ID); err != nil || len(vs) != 1 {
		t.Fatalf("tenant policy versions = %+v, %v", vs, err)
	}

	// GET /tenants/{t}/policies/deleted lists them (Recorder.Deleted).
	del, err := rec.Deleted(e.ctx, e.acme.ID)
	e.must(err)
	got := map[string]string{}
	for _, v := range del {
		got[v.PolicyID] = v.Policy.Name
	}
	if len(del) != 2 || got[freeze.ID] != "web/freeze" || got[secret.ID] != "web/secret" {
		t.Fatalf("deleted = %+v", del)
	}

	// policy.changed was published for each delete.
	seen := map[string]bool{}
	timeout := time.After(2 * time.Second)
	for len(seen) < 2 {
		select {
		case ev := <-ch:
			m, _ := ev.Payload.(map[string]any)
			if m["change"] == policyhist.ChangeDelete && ev.Actor == "olga" && ev.TreeID == e.web.ID {
				seen[m["name"].(string)] = true
			}
		case <-timeout:
			t.Fatalf("policy.changed delete events = %v", seen)
		}
	}
}

func TestTreeDeletedWithoutActor(t *testing.T) {
	e := newEnv(t)
	e.apply("olga", e.web, Document{Policies: []domain.Policy{{Name: "f", Enabled: true,
		Statements: []domain.PolicyStatement{deny(l("tree:write"), l("wt:tree/acme/web"), l("*"))}}}}, false)
	id := e.policies()["web/f"].ID
	e.must(e.svc.TreeDeleted(e.ctx, e.acme.ID, e.web.ID))
	vs, err := policyhist.New(e.store, nil, nil).Versions(e.ctx, e.acme.ID, id)
	e.must(err)
	if len(vs) != 2 || vs[1].Change != policyhist.ChangeDelete || vs[1].ChangedBy != "system" || vs[1].Comment != "tree "+e.web.ID+" deleted" {
		t.Fatalf("versions = %+v", vs)
	}
	if eventTreeName(struct {
		Name string `json:"name"`
	}{"x"}) != "x" || eventTreeName(nil) != "" {
		t.Fatal("eventTreeName")
	}
}
