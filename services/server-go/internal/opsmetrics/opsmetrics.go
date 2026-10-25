// Package opsmetrics holds the process-wide operational counters of
// docs/ENGINEERING_PLAN.md §13.3 (sync, authorization, merge, GC and rate
// limiting). They are incremented by the gRPC interceptors, the HTTP layer,
// the gc and limits packages and rendered by httpapi's /metrics endpoint.
//
// The counters are package-level (like the default Prometheus registry) so
// that every layer can count without extra wiring; they only ever grow.
package opsmetrics

import (
	"context"
	"sort"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Counter is a monotonically increasing counter with an optional label.
type Counter struct {
	Name  string // metric name
	Help  string
	Label string // label name; "" = unlabelled

	mu     sync.Mutex
	values map[string]uint64
}

// NewCounter returns a counter; label "" makes it unlabelled.
func NewCounter(name, help, label string) *Counter {
	return &Counter{Name: name, Help: help, Label: label, values: map[string]uint64{}}
}

// Inc adds one for the given label value ("" for unlabelled counters).
func (c *Counter) Inc(value string) { c.Add(value, 1) }

// Add adds n for the given label value.
func (c *Counter) Add(value string, n uint64) {
	c.mu.Lock()
	c.values[value] += n
	c.mu.Unlock()
}

// Get returns the current value for a label value.
func (c *Counter) Get(value string) uint64 {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.values[value]
}

// Sample is one labelled value of a counter.
type Sample struct {
	Label string
	Value uint64
}

// Samples returns the values sorted by label. An unlabelled counter always
// yields one sample (possibly zero).
func (c *Counter) Samples() []Sample {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.Label == "" {
		return []Sample{{Value: c.values[""]}}
	}
	out := make([]Sample, 0, len(c.values))
	for k, v := range c.values {
		out = append(out, Sample{Label: k, Value: v})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Label < out[j].Label })
	return out
}

// The §13.3 counters.
var (
	SyncPushes = NewCounter("wt_sync_pushes_total",
		"gRPC pushes by result (accepted, rejected, error).", "result")
	SyncPulls = NewCounter("wt_sync_pulls_total",
		"gRPC pulls served.", "")
	AuthzDecisions = NewCounter("wt_authz_decisions_total",
		"Authorization decisions by outcome (allow, deny).", "decision")
	MRMerges = NewCounter("wt_mr_merges_total",
		"Merge requests merged.", "")
	GCRuns = NewCounter("wt_gc_runs_total",
		"Garbage-collection runs (including dry runs).", "")
	GCDeletedObjects = NewCounter("wt_gc_deleted_objects_total",
		"Objects deleted by garbage collection.", "")
	RateLimited = NewCounter("wt_rate_limited_total",
		"Requests rejected by the rate limiter.", "")
)

// All lists every counter in rendering order.
func All() []*Counter {
	return []*Counter{SyncPushes, SyncPulls, AuthzDecisions, MRMerges, GCRuns, GCDeletedObjects, RateLimited}
}

// CountMerges counts EventMRMerged events from bus until the returned
// cancel function is called.
func CountMerges(bus domain.EventBus) (cancel func()) {
	if bus == nil {
		return func() {}
	}
	ch, unsub := bus.Subscribe(domain.EventFilter{Types: []string{domain.EventMRMerged}})
	ctx, stop := context.WithCancel(context.Background())
	go func() {
		for {
			select {
			case <-ctx.Done():
				return
			case _, ok := <-ch:
				if !ok {
					return
				}
				MRMerges.Inc("")
			}
		}
	}()
	return func() { stop(); unsub() }
}
