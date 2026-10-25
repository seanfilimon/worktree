package audit

import (
	"context"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

func TestEachPagesWithoutGapsOrDuplicates(t *testing.T) {
	ctx := context.Background()
	stores := map[string]func(t *testing.T) domain.MetaStore{
		"memory": func(*testing.T) domain.MetaStore { return meta.NewMemory() },
		"bolt": func(t *testing.T) domain.MetaStore {
			b, err := meta.NewBolt(t.TempDir() + "/meta.db")
			if err != nil {
				t.Fatal(err)
			}
			return b
		},
	}
	base := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	for name, mk := range stores {
		t.Run(name, func(t *testing.T) {
			s := mk(t)
			defer s.Close()
			// 23 events; several share instants (a burst of 7 at +5s spans
			// page boundaries with page size 4).
			var want []string
			secs := []int{0, 1, 1, 2, 3, 4, 5, 5, 5, 5, 5, 5, 5, 6, 7, 8, 8, 9, 10, 11, 12, 13, 14}
			for i, sec := range secs {
				action := "iam.x"
				if i%3 == 0 {
					action = "tree.create"
				}
				id := fmt.Sprintf("e%02d", i)
				if err := s.AppendAudit(ctx, &domain.AuditEvent{ID: id, Action: action, TenantID: "t1", At: base.Add(time.Duration(sec) * time.Second)}); err != nil {
					t.Fatal(err)
				}
				want = append(want, id)
			}
			var got []string
			err := each(ctx, s, domain.AuditFilter{}, 4, func(e domain.AuditEvent) error {
				got = append(got, e.ID)
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
			if len(got) != len(want) {
				t.Fatalf("got %d events, want %d: %v", len(got), len(want), got)
			}
			seen := map[string]bool{}
			for _, id := range got {
				if seen[id] {
					t.Fatalf("duplicate %s in %v", id, got)
				}
				seen[id] = true
			}

			// Filters apply: prefix action + until.
			n := 0
			until := base.Add(9 * time.Second)
			err = each(ctx, s, domain.AuditFilter{Action: "iam.*", Until: until}, 3, func(e domain.AuditEvent) error {
				if e.Action != "iam.x" || !e.At.Before(until) {
					t.Fatalf("filter leaked %+v", e)
				}
				n++
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
			wantN := 0
			for i, sec := range secs {
				if i%3 != 0 && sec < 9 {
					wantN++
				}
			}
			if n != wantN {
				t.Fatalf("filtered export = %d events, want %d", n, wantN)
			}

			// fn errors stop the iteration.
			stop := errors.New("stop")
			calls := 0
			if err := Each(ctx, s, domain.AuditFilter{}, func(domain.AuditEvent) error { calls++; return stop }); !errors.Is(err, stop) || calls != 1 {
				t.Fatalf("err = %v after %d calls", err, calls)
			}
		})
	}
}
