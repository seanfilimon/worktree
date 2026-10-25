// Package policyhist records and serves the version history of tenant
// policies (docs/ENGINEERING_PLAN.md §14.7).
//
// Every policy write — REST create, update, delete and rollback, and every
// declarative access apply (accesscfg) — appends a domain.PolicyVersion to
// the DocStore collection Collection, keyed by
// DocKey(tenantID, policyID, "%08d" version). Version numbers are the
// history's own sequence (1, 2, 3, ... per policy, never reused); for a
// policy created after history recording was introduced they equal
// Policy.Version for every create, update and rollback, and a delete takes
// the next number.
//
// Policies that existed before history was recorded have no versions. The
// first time such a policy's history is read, or just before its first
// recorded change, a synthetic version 1 is stored: Change "create", the
// policy as it was at that moment, ChangedBy = Policy.CreatedBy,
// ChangedAt = Policy.UpdatedAt and Comment = SyntheticComment.
//
// Each recorded version publishes domain.EventPolicyChanged with the
// payload {"policy_id","name","version","change"} (plus "comment" when
// set).
package policyhist

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// Collection is the DocStore collection of policy versions (§14).
const Collection = "policy_versions"

// Change kinds (PolicyVersion.Change).
const (
	ChangeCreate   = "create"
	ChangeUpdate   = "update"
	ChangeDelete   = "delete"
	ChangeRollback = "rollback"
)

// SyntheticComment marks the version 1 materialized for a policy that
// existed before version history was recorded.
const SyntheticComment = "synthetic version: the policy existed before version history was recorded"

// maxAttempts bounds retries when concurrent writers race for a version
// number.
const maxAttempts = 8

// Store is what the recorder needs: the document collections and the
// tenant's current policies (domain.MetaStore satisfies it).
type Store interface {
	domain.DocStore
	ListPolicies(ctx context.Context, tenantID string) ([]domain.Policy, error)
}

// Recorder appends and reads policy versions. It is cheap to create.
type Recorder struct {
	store Store
	bus   domain.EventBus
	log   *slog.Logger
	now   func() time.Time
}

// New returns a Recorder. bus and log may be nil.
func New(store Store, bus domain.EventBus, log *slog.Logger) *Recorder {
	if log == nil {
		log = slog.Default()
	}
	return &Recorder{store: store, bus: bus, log: log, now: func() time.Time { return time.Now().UTC() }}
}

func key(tenantID, policyID string, version int) string {
	return domain.DocKey(tenantID, policyID, fmt.Sprintf("%08d", version))
}

// Entry describes one policy change to record.
type Entry struct {
	Change  string // ChangeCreate | ChangeUpdate | ChangeDelete | ChangeRollback
	Actor   string // username of whoever made the change
	Comment string
	// Before is the policy before the change (nil for creates). It becomes
	// the synthetic version 1 when the policy has no history yet.
	Before *domain.Policy
	// Policy is the policy after the change (for deletes: as it was).
	Policy domain.Policy
}

// Versions returns the stored versions of a policy, oldest first, without
// synthesizing anything.
func (r *Recorder) Versions(ctx context.Context, tenantID, policyID string) ([]domain.PolicyVersion, error) {
	vs, err := meta.ListDocsT[domain.PolicyVersion](ctx, r.store, Collection, meta.DocPrefix(tenantID, policyID))
	if err != nil {
		return nil, fmt.Errorf("policyhist: list versions: %w", err)
	}
	return vs, nil
}

// History returns the versions of a policy, oldest first. When none are
// stored and current (the live policy) is non-nil, the synthetic version 1
// is stored first. An empty result means the policy is unknown.
func (r *Recorder) History(ctx context.Context, tenantID, policyID string, current *domain.Policy) ([]domain.PolicyVersion, error) {
	vs, err := r.Versions(ctx, tenantID, policyID)
	if err != nil || len(vs) > 0 || current == nil {
		return vs, err
	}
	syn := synthetic(*current)
	if err := r.store.CreateDoc(ctx, Collection, key(tenantID, policyID, 1), syn); err != nil && !errors.Is(err, domain.ErrAlreadyExists) {
		return nil, fmt.Errorf("policyhist: store synthetic version: %w", err)
	}
	return r.Versions(ctx, tenantID, policyID)
}

// Find returns version v of a policy from its history (see History);
// domain.ErrNotFound when there is no such version.
func (r *Recorder) Find(ctx context.Context, tenantID, policyID string, v int, current *domain.Policy) (*domain.PolicyVersion, error) {
	vs, err := r.History(ctx, tenantID, policyID, current)
	if err != nil {
		return nil, err
	}
	for i := range vs {
		if vs[i].Version == v {
			return &vs[i], nil
		}
	}
	return nil, fmt.Errorf("policy %s version %d: %w", policyID, v, domain.ErrNotFound)
}

func synthetic(p domain.Policy) domain.PolicyVersion {
	at := p.UpdatedAt
	if at.IsZero() {
		at = p.CreatedAt
	}
	return domain.PolicyVersion{
		TenantID: p.TenantID, PolicyID: p.ID, Version: 1, Change: ChangeCreate, Policy: p,
		ChangedBy: p.CreatedBy, ChangedAt: at, Comment: SyntheticComment,
	}
}

// Record appends a version for e and publishes policy.changed. A policy
// without history first gets its synthetic version 1 from e.Before.
func (r *Recorder) Record(ctx context.Context, e Entry) (*domain.PolicyVersion, error) {
	pol := e.Policy
	if pol.TenantID == "" || pol.ID == "" {
		return nil, fmt.Errorf("policyhist: policy tenant and id are required: %w", domain.ErrInvalid)
	}
	for attempt := 0; attempt < maxAttempts; attempt++ {
		vs, err := r.Versions(ctx, pol.TenantID, pol.ID)
		if err != nil {
			return nil, err
		}
		next := 1
		if len(vs) > 0 {
			next = vs[len(vs)-1].Version + 1
		} else if e.Change != ChangeCreate && e.Before != nil {
			syn := synthetic(*e.Before)
			if err := r.store.CreateDoc(ctx, Collection, key(pol.TenantID, pol.ID, 1), syn); err != nil {
				if errors.Is(err, domain.ErrAlreadyExists) {
					continue
				}
				return nil, fmt.Errorf("policyhist: store synthetic version: %w", err)
			}
			next = 2
		}
		pv := domain.PolicyVersion{
			TenantID: pol.TenantID, PolicyID: pol.ID, Version: next, Change: e.Change, Policy: pol,
			ChangedBy: e.Actor, ChangedAt: r.now(), Comment: e.Comment,
		}
		if err := r.store.CreateDoc(ctx, Collection, key(pol.TenantID, pol.ID, next), pv); err != nil {
			if errors.Is(err, domain.ErrAlreadyExists) {
				continue
			}
			return nil, fmt.Errorf("policyhist: store version: %w", err)
		}
		r.publish(pv)
		return &pv, nil
	}
	return nil, fmt.Errorf("policyhist: policy %s: too many concurrent changes: %w", pol.ID, domain.ErrConflict)
}

func (r *Recorder) publish(pv domain.PolicyVersion) {
	if r.bus == nil {
		return
	}
	payload := map[string]any{"policy_id": pv.PolicyID, "name": pv.Policy.Name, "version": pv.Version, "change": pv.Change}
	if pv.Comment != "" {
		payload["comment"] = pv.Comment
	}
	r.bus.Publish(domain.Event{
		Type: domain.EventPolicyChanged, TenantID: pv.TenantID, TreeID: pv.Policy.TreeID,
		Actor: pv.ChangedBy, Payload: payload, At: pv.ChangedAt,
	})
}

// Deleted returns, for every policy of the tenant whose latest version is
// a delete, that delete version (its Policy is the policy as it was),
// newest deletion first.
func (r *Recorder) Deleted(ctx context.Context, tenantID string) ([]domain.PolicyVersion, error) {
	all, err := meta.ListDocsT[domain.PolicyVersion](ctx, r.store, Collection, meta.DocPrefix(tenantID))
	if err != nil {
		return nil, fmt.Errorf("policyhist: list versions: %w", err)
	}
	latest := map[string]domain.PolicyVersion{}
	for _, v := range all { // key order: per policy, oldest first
		latest[v.PolicyID] = v
	}
	out := []domain.PolicyVersion{}
	for _, v := range latest {
		if v.Change == ChangeDelete {
			out = append(out, v)
		}
	}
	slices.SortFunc(out, func(a, b domain.PolicyVersion) int {
		if c := b.ChangedAt.Compare(a.ChangedAt); c != 0 {
			return c
		}
		return strings.Compare(a.PolicyID, b.PolicyID)
	})
	return out, nil
}

// Track runs apply (which writes config-managed policies of tenantID) and
// then records a version for every config-managed policy it created,
// changed or deleted, attributed to actor with comment. apply's error is
// returned unchanged and nothing is recorded when it fails. Recording
// problems are logged, never returned: the policies are already written.
func (r *Recorder) Track(ctx context.Context, actor, tenantID, comment string, apply func() error) error {
	before, listErr := r.store.ListPolicies(ctx, tenantID)
	if err := apply(); err != nil {
		return err
	}
	if listErr != nil {
		r.log.ErrorContext(ctx, "policyhist: cannot record policy versions of an access apply", slog.String("tenant_id", tenantID), slog.Any("error", listErr))
		return nil
	}
	after, err := r.store.ListPolicies(ctx, tenantID)
	if err != nil {
		r.log.ErrorContext(ctx, "policyhist: cannot record policy versions of an access apply", slog.String("tenant_id", tenantID), slog.Any("error", err))
		return nil
	}
	for _, e := range changes(before, after) {
		e.Actor, e.Comment = actor, comment
		if _, err := r.Record(ctx, e); err != nil {
			r.log.ErrorContext(ctx, "policyhist: recording a policy version failed", slog.String("policy_id", e.Policy.ID), slog.Any("error", err))
		}
	}
	return nil
}

// changes compares two listings of a tenant's policies and returns the
// creates, updates and deletes of config-managed policies, by name.
func changes(before, after []domain.Policy) []Entry {
	managed := func(p *domain.Policy) bool { return p != nil && p.ManagedBy == domain.ManagedByConfig }
	prev := make(map[string]*domain.Policy, len(before))
	for i := range before {
		prev[before[i].ID] = &before[i]
	}
	var out []Entry
	seen := map[string]bool{}
	for i := range after {
		cur := &after[i]
		seen[cur.ID] = true
		old := prev[cur.ID]
		if !managed(cur) && !managed(old) {
			continue
		}
		switch {
		case old == nil:
			out = append(out, Entry{Change: ChangeCreate, Policy: *cur})
		case old.Version != cur.Version || old.ManagedBy != cur.ManagedBy || !SameContent(old, cur):
			out = append(out, Entry{Change: ChangeUpdate, Before: old, Policy: *cur})
		}
	}
	for i := range before {
		old := &before[i]
		if !seen[old.ID] && managed(old) {
			out = append(out, Entry{Change: ChangeDelete, Before: old, Policy: *old})
		}
	}
	slices.SortStableFunc(out, func(a, b Entry) int { return strings.Compare(a.Policy.Name, b.Policy.Name) })
	return out
}

// SameContent reports whether two policies have the same name,
// description, enabled flag and statements (compared canonically).
func SameContent(a, b *domain.Policy) bool {
	if a.Name != b.Name || a.Description != b.Description || a.Enabled != b.Enabled || len(a.Statements) != len(b.Statements) {
		return false
	}
	for i := range a.Statements {
		if !sameStatement(a.Statements[i], b.Statements[i]) {
			return false
		}
	}
	return true
}

func sameStatement(a, b domain.PolicyStatement) bool {
	ja, err1 := json.Marshal(canon(a))
	jb, err2 := json.Marshal(canon(b))
	return err1 == nil && err2 == nil && bytes.Equal(ja, jb)
}

// canon copies a statement into a form that compares equal across a store
// round trip (empty slices as nil, times in UTC, lower-case effect).
func canon(st domain.PolicyStatement) domain.PolicyStatement {
	orNil := func(s []string) []string {
		if len(s) == 0 {
			return nil
		}
		return s
	}
	utc := func(t *time.Time) *time.Time {
		if t == nil {
			return nil
		}
		u := t.UTC()
		return &u
	}
	st.Sid = strings.TrimSpace(st.Sid)
	st.Effect = strings.ToLower(strings.TrimSpace(st.Effect))
	st.Actions, st.Resources, st.Principals = orNil(st.Actions), orNil(st.Resources), orNil(st.Principals)
	c := &st.Conditions
	c.IPRanges = orNil(c.IPRanges)
	c.NotBefore, c.NotAfter = utc(c.NotBefore), utc(c.NotAfter)
	if c.TimeWindow != nil {
		tw := *c.TimeWindow
		tw.Days = orNil(tw.Days)
		c.TimeWindow = &tw
	}
	return st
}
