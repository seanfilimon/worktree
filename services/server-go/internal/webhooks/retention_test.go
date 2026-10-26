package webhooks

// server-ops (docs/ENGINEERING_PLAN.md §16.11): delivery retention counts.

import (
	"fmt"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestPruneDeliveries(t *testing.T) {
	e := newEnv(t, func(o *Options) { o.Keep = 3; o.Retention = 48 * time.Hour })
	h, _, err := e.svc.Create(e.ctx, e.alice, e.tenantScope(), CreateInput{URL: "https://hooks.example.test/a", Events: []string{"*"}})
	must(t, err)
	h2, _, err := e.svc.Create(e.ctx, e.alice, e.tenantScope(), CreateInput{URL: "https://hooks.example.test/b", Events: []string{"*"}})
	must(t, err)
	put := func(hookID string, i int, age time.Duration) {
		d := domain.WebhookDelivery{ID: fmt.Sprintf("d-%s-%d", hookID, i), WebhookID: hookID, Event: "ping",
			Payload: []byte(`{}`), CreatedAt: e.now().Add(-age)}
		must(t, e.store.PutDoc(e.ctx, CollDeliveries, DeliveryKey(&d), d))
	}
	// h: 5 recent deliveries (2 beyond Keep) and 1 too old; h2: 1 too old.
	put(h.ID, 0, 72*time.Hour)
	for i := 1; i <= 5; i++ {
		put(h.ID, i, time.Duration(6-i)*time.Hour)
	}
	put(h2.ID, 0, 49*time.Hour)

	n, err := e.svc.PruneDeliveries(e.ctx)
	must(t, err)
	if n != 4 {
		t.Fatalf("pruned %d, want 4", n)
	}
	left, err := e.svc.deliveriesOf(e.ctx, h.ID)
	must(t, err)
	if len(left) != 3 || left[0].ID != "d-"+h.ID+"-3" {
		t.Fatalf("left = %+v", left)
	}
	if n, err := e.svc.PruneDeliveries(e.ctx); err != nil || n != 0 {
		t.Fatalf("second pass = %d, %v", n, err)
	}
}
