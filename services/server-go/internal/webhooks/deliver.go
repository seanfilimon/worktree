package webhooks

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// Envelope is the JSON body POSTed to a webhook.
type Envelope struct {
	ID      string    `json:"id"`     // the delivery id (also X-WT-Delivery)
	Event   string    `json:"event"`  // event type (also X-WT-Event)
	Tenant  string    `json:"tenant"` // tenant slug
	Tree    string    `json:"tree,omitempty"`
	Actor   string    `json:"actor,omitempty"`
	At      time.Time `json:"at"`
	Payload any       `json:"payload"`
}

// job is one delivery attempt to run.
type job struct {
	tenantID string
	hookID   string
	key      string // delivery document key
	manual   bool   // redelivery or ping: one attempt, no failure count
}

// pendingRetry is a scheduled retry of a failed delivery.
type pendingRetry struct {
	job
	due time.Time
}

// ---- background loop -----------------------------------------------------

// Start subscribes to every bus event and delivers matching events to the
// hooks in a pool of Options.Workers goroutines; due retries are checked
// every PollInterval and the delivery logs pruned hourly, until ctx is
// cancelled. Deliveries that were pending when the server stopped are
// resumed. It returns once subscribed. Call Configure before Start.
func (s *Service) Start(ctx context.Context) {
	jobs := make(chan job, 256)
	s.mu.Lock()
	s.jobs = jobs
	s.mu.Unlock()
	for range s.o.Workers {
		go func() {
			for {
				select {
				case <-ctx.Done():
					return
				case j := <-jobs:
					if _, err := s.process(ctx, j); err != nil {
						s.d.Logger.WarnContext(ctx, "webhooks: delivery failed", "webhook", j.hookID, "err", err)
					}
				}
			}
		}()
	}
	if err := s.loadPending(ctx); err != nil {
		s.d.Logger.WarnContext(ctx, "webhooks: loading pending deliveries failed", "err", err)
	}
	var events <-chan domain.Event
	cancel := func() {}
	if s.d.Bus != nil {
		events, cancel = s.d.Bus.Subscribe(domain.EventFilter{})
	}
	go func() {
		defer cancel()
		poll := time.NewTicker(s.o.PollInterval)
		defer poll.Stop()
		prune := time.NewTicker(DefaultPruneEvery)
		defer prune.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-events:
				if !ok {
					events = nil
					continue
				}
				if err := s.HandleEvent(ctx, e); err != nil {
					s.d.Logger.WarnContext(ctx, "webhooks: handling event failed", "type", e.Type, "tenant", e.TenantID, "err", err)
				}
			case <-poll.C:
				s.RunDue(ctx)
			case <-prune.C:
				if err := s.Prune(ctx); err != nil {
					s.d.Logger.WarnContext(ctx, "webhooks: pruning delivery logs failed", "err", err)
				}
			}
		}
	}()
}

// dispatch runs j on the worker pool once started, else synchronously.
func (s *Service) dispatch(ctx context.Context, j job) {
	s.mu.Lock()
	jobs := s.jobs
	s.mu.Unlock()
	if jobs != nil {
		select {
		case jobs <- j:
		case <-ctx.Done():
		}
		return
	}
	if _, err := s.process(ctx, j); err != nil {
		s.d.Logger.WarnContext(ctx, "webhooks: delivery failed", "webhook", j.hookID, "err", err)
	}
}

func (s *Service) schedule(j job, deliveryID string, due time.Time) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.pending[deliveryID] = pendingRetry{job: j, due: due}
}

func (s *Service) dropPending(hookID string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for id, p := range s.pending {
		if p.hookID == hookID {
			delete(s.pending, id)
		}
	}
}

// Pending returns the number of scheduled retries.
func (s *Service) Pending() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.pending)
}

// RunDue dispatches every retry that is due and returns how many.
func (s *Service) RunDue(ctx context.Context) int {
	now := s.now()
	var due []job
	s.mu.Lock()
	for id, p := range s.pending {
		if !p.due.After(now) {
			due = append(due, p.job)
			delete(s.pending, id)
		}
	}
	s.mu.Unlock()
	for _, j := range due {
		s.dispatch(ctx, j)
	}
	return len(due)
}

// loadPending schedules the stored deliveries that still need an attempt:
// retries (NextRetryAt) and deliveries never attempted.
func (s *Service) loadPending(ctx context.Context) error {
	hooks, err := meta.ListDocsT[hookDoc](ctx, s.d.Store, CollHooks, "")
	if err != nil {
		return err
	}
	now := s.now()
	for i := range hooks {
		h := &hooks[i]
		if !h.Active {
			continue
		}
		list, err := s.deliveriesOf(ctx, h.ID)
		if err != nil {
			return err
		}
		for k := range list {
			d := &list[k]
			if d.Succeeded {
				continue
			}
			j := job{tenantID: h.TenantID, hookID: h.ID, key: DeliveryKey(d)}
			switch {
			case d.NextRetryAt != nil:
				s.schedule(j, d.ID, *d.NextRetryAt)
			case d.Attempts == 0 && !d.Redelivery && d.Event != EventPing:
				s.schedule(j, d.ID, now)
			}
		}
	}
	return nil
}

// ---- event handling ----------------------------------------------------

// HandleEvent delivers e to every matching hook (Start calls it for each
// bus event; tests call it directly). tree.deleted also removes the hooks
// of the deleted tree.
func (s *Service) HandleEvent(ctx context.Context, e domain.Event) error {
	var errs []error
	if e.Type == domain.EventTreeDeleted && e.TreeID != "" && e.TenantID != "" {
		if err := s.deleteTreeHooks(ctx, e.TenantID, e.TreeID); err != nil {
			errs = append(errs, err)
		}
	}
	if e.TenantID == "" || e.Type == EventPing {
		return errors.Join(errs...)
	}
	hooks, err := s.hooksOfTenant(ctx, e.TenantID)
	if err != nil {
		return errors.Join(append(errs, err)...)
	}
	var matching []*hookDoc
	for i := range hooks {
		h := &hooks[i]
		if h.Active && (h.TreeID == "" || h.TreeID == e.TreeID) && Matches(h.Events, e.Type) {
			matching = append(matching, h)
		}
	}
	if len(matching) == 0 {
		return errors.Join(errs...)
	}

	tenantSlug := e.TenantID
	if t, err := s.d.Store.GetTenant(ctx, e.TenantID); err == nil {
		tenantSlug = t.Slug
	}
	var tree *domain.Tree
	treeName := ""
	if e.TreeID != "" {
		if t, err := s.d.Store.GetTree(ctx, e.TreeID); err == nil {
			tree, treeName = t, t.Name
		} else if !errors.Is(err, domain.ErrNotFound) {
			return errors.Join(append(errs, err)...)
		} else {
			treeName = payloadName(e.Payload) // tree.deleted carries {name}
		}
	}
	at := e.At
	if at.IsZero() {
		at = s.now()
	}
	principals := map[string]*domain.Principal{}
	for _, h := range matching {
		if !s.mayRead(ctx, principals, h, e, tree) {
			s.d.Logger.DebugContext(ctx, "webhooks: creator may not read the event; skipped",
				"webhook", h.ID, "type", e.Type, "creator", h.CreatedBy)
			continue
		}
		env := Envelope{Event: e.Type, Tenant: tenantSlug, Tree: treeName, Actor: e.Actor, At: at.UTC(), Payload: e.Payload}
		d, err := s.newDelivery(ctx, h.ID, env, false)
		if err != nil {
			errs = append(errs, err)
			continue
		}
		s.dispatch(ctx, job{tenantID: h.TenantID, hookID: h.ID, key: DeliveryKey(d)})
	}
	return errors.Join(errs...)
}

func payloadName(p any) string {
	raw, err := json.Marshal(p)
	if err != nil {
		return ""
	}
	var v struct {
		Name string `json:"name"`
	}
	_ = json.Unmarshal(raw, &v)
	return v.Name
}

// mayRead re-checks that the hook's creator may read e: tree:read for
// tree events (on the tenant when the tree is gone), tenant:read for
// tenant events. Creators that no longer exist or are disabled read
// nothing.
func (s *Service) mayRead(ctx context.Context, cache map[string]*domain.Principal, h *hookDoc, e domain.Event, tree *domain.Tree) bool {
	if s.d.Authz == nil {
		return true
	}
	ck := h.CreatorID + "\x00" + h.CreatedBy
	p, ok := cache[ck]
	if !ok {
		p = s.creator(ctx, h)
		cache[ck] = p
	}
	if p == nil {
		return false
	}
	act := domain.ActTenantRead
	if e.TreeID != "" {
		act = domain.ActTreeRead
	}
	return s.d.Authz.Authorize(ctx, p, act, e.TenantID, tree) == nil
}

// creator resolves the principal that created h (nil = gone or disabled).
func (s *Service) creator(ctx context.Context, h *hookDoc) *domain.Principal {
	if h.CreatorID != "" {
		a, err := s.d.Store.GetAccount(ctx, h.CreatorID)
		if err != nil || a.Disabled {
			return nil
		}
		return &domain.Principal{AccountID: a.ID, Username: a.Username, TenantID: a.TenantID, IsAdmin: a.IsAdmin}
	}
	rest, ok := strings.CutPrefix(h.CreatedBy, "sa:")
	if !ok {
		return nil
	}
	slug, name, ok := strings.Cut(rest, "/")
	if !ok {
		return nil
	}
	ten, err := s.d.Store.GetTenantBySlug(ctx, slug)
	if err != nil {
		return nil
	}
	sa, err := s.d.Store.GetServiceAccountByName(ctx, ten.ID, name)
	if err != nil || sa.Disabled {
		return nil
	}
	return &domain.Principal{Username: h.CreatedBy, TenantID: sa.TenantID,
		Kind: domain.PrincipalServiceAccount, ServiceAccountID: sa.ID}
}

// newDelivery stores a new delivery of env to hookID (env.ID is set to
// the delivery id) and prunes the hook's log.
func (s *Service) newDelivery(ctx context.Context, hookID string, env Envelope, redelivery bool) (*domain.WebhookDelivery, error) {
	env.ID = uuid.NewString()
	body, err := json.Marshal(env)
	if err != nil {
		return nil, err
	}
	d := &domain.WebhookDelivery{
		ID:         env.ID,
		WebhookID:  hookID,
		Event:      env.Event,
		Payload:    body,
		CreatedAt:  s.now(),
		Redelivery: redelivery,
	}
	if err := s.d.Store.CreateDoc(ctx, CollDeliveries, DeliveryKey(d), d); err != nil {
		return nil, err
	}
	if err := s.prune(ctx, hookID); err != nil {
		s.d.Logger.WarnContext(ctx, "webhooks: pruning delivery log failed", "webhook", hookID, "err", err)
	}
	return d, nil
}

// ---- attempts ----------------------------------------------------------------

// result is the outcome of one HTTP attempt.
type result struct {
	status   int
	body     string
	err      string
	duration time.Duration
}

func (r result) ok() bool { return r.err == "" && r.status >= 200 && r.status < 300 }

// attempt POSTs body to h.
func (s *Service) attempt(ctx context.Context, h *hookDoc, event, deliveryID string, body []byte) result {
	ctx, cancel := context.WithTimeout(ctx, s.o.Timeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, h.URL, bytes.NewReader(body))
	if err != nil {
		return result{err: "invalid request: " + err.Error()}
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("User-Agent", "W0rkTree-Hookshot/"+s.o.Version)
	req.Header.Set("X-WT-Event", event)
	req.Header.Set("X-WT-Delivery", deliveryID)
	req.Header.Set("X-WT-Hook-ID", h.ID)
	if h.Secret != "" {
		req.Header.Set("X-WT-Signature", Sign(h.Secret, body))
	}
	start := time.Now()
	resp, err := s.o.Client.Do(req)
	if err != nil {
		return result{err: err.Error(), duration: time.Since(start)}
	}
	defer resp.Body.Close()
	buf, _ := io.ReadAll(io.LimitReader(resp.Body, MaxResponse))
	_, _ = io.CopyN(io.Discard, resp.Body, 64<<10) // let the connection be reused
	return result{status: resp.StatusCode, body: strings.ToValidUTF8(string(buf), "�"), duration: time.Since(start)}
}

// process runs one attempt of job j and records it: the delivery's
// status, the retry schedule, the hook's last status and failure count,
// and deactivation (publishing webhook.failing) at FailureLimit.
func (s *Service) process(ctx context.Context, j job) (*domain.WebhookDelivery, error) {
	store := context.WithoutCancel(ctx)
	h, err := meta.GetDocT[hookDoc](store, s.d.Store, CollHooks, HookKey(j.tenantID, j.hookID))
	if errors.Is(err, domain.ErrNotFound) {
		return nil, nil // deleted meanwhile
	}
	if err != nil {
		return nil, err
	}
	d, err := meta.GetDocT[domain.WebhookDelivery](store, s.d.Store, CollDeliveries, j.key)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, nil // pruned meanwhile
	}
	if err != nil {
		return nil, err
	}
	if !j.manual && !h.Active {
		d.NextRetryAt = nil
		d.Error = "webhook is inactive"
		return d, s.d.Store.PutDoc(store, CollDeliveries, j.key, d)
	}

	res := s.attempt(ctx, h, d.Event, d.ID, d.Payload)
	if ctx.Err() != nil {
		return d, nil // shutting down: the delivery stays pending for the next start
	}
	now := s.now()
	d.Attempts++
	d.StatusCode, d.Response, d.Error = res.status, res.body, res.err
	if d.Error == "" && !res.ok() {
		d.Error = fmt.Sprintf("receiver answered HTTP %d", res.status)
	}
	d.DurationMS = res.duration.Milliseconds()
	d.Succeeded = res.ok()
	d.DeliveredAt = &now
	d.NextRetryAt = nil
	if !d.Succeeded && !j.manual && d.Attempts <= len(s.o.Backoff) {
		next := now.Add(s.o.Backoff[d.Attempts-1])
		d.NextRetryAt = &next
	}
	if err := s.d.Store.PutDoc(store, CollDeliveries, j.key, d); err != nil {
		return d, err
	}

	deactivated := false
	var final hookDoc
	err = meta.UpdateDocT(store, s.d.Store, CollHooks, HookKey(j.tenantID, j.hookID), func(cur *hookDoc) (*hookDoc, error) {
		if cur == nil {
			return nil, nil
		}
		cur.LastDeliveryAt = &now
		cur.LastStatus = res.status
		switch {
		case d.Succeeded:
			cur.FailureCount = 0
		case !j.manual:
			cur.FailureCount++
			if cur.Active && cur.FailureCount >= s.o.FailureLimit {
				cur.Active = false
				cur.UpdatedAt = now
				deactivated = true
			}
		}
		final = *cur
		return cur, nil
	})
	if err != nil {
		return d, err
	}
	if d.NextRetryAt != nil && !deactivated {
		s.schedule(j, d.ID, *d.NextRetryAt)
	}
	if deactivated {
		s.dropPending(j.hookID)
		s.d.Logger.WarnContext(ctx, "webhooks: deactivated after repeated failures",
			"webhook", final.ID, "url", final.URL, "failures", final.FailureCount)
		if s.d.Bus != nil {
			s.d.Bus.Publish(domain.Event{
				Type: domain.EventWebhookFailing, TenantID: final.TenantID, TreeID: final.TreeID, Actor: "system",
				Payload: map[string]any{
					"webhook_id": final.ID, "url": final.URL, "failure_count": final.FailureCount,
					"last_status": final.LastStatus, "last_error": d.Error,
				},
				At: now,
			})
		}
	}
	return d, nil
}

// ---- manual deliveries -------------------------------------------------------

// Redeliver sends the request body of delivery did again as a new
// delivery (one attempt, synchronously) and returns it. Inactive hooks
// may be redelivered to (to check a receiver before reactivating).
func (s *Service) Redeliver(ctx context.Context, sc Scope, id, did string) (*domain.WebhookDelivery, error) {
	h, err := s.loadHook(ctx, sc, id)
	if err != nil {
		return nil, err
	}
	old, err := s.findDelivery(ctx, id, did)
	if err != nil {
		return nil, err
	}
	var env struct {
		Envelope
		Payload json.RawMessage `json:"payload"`
	}
	if err := json.Unmarshal(old.Payload, &env); err != nil {
		return nil, errorf(domain.ErrConflict, "delivery %q has no replayable request body", did)
	}
	e := env.Envelope
	e.Payload = env.Payload
	d, err := s.newDelivery(ctx, h.ID, e, true)
	if err != nil {
		return nil, err
	}
	return s.runManual(ctx, h, d)
}

// Ping sends a test "ping" delivery to hook id of sc on behalf of p.
func (s *Service) Ping(ctx context.Context, p *domain.Principal, sc Scope, id string) (*domain.WebhookDelivery, error) {
	h, err := s.loadHook(ctx, sc, id)
	if err != nil {
		return nil, err
	}
	env := Envelope{
		Event:  EventPing,
		Tenant: sc.Tenant.Slug,
		At:     s.now(),
		Payload: map[string]any{
			"message": "ping from W0rkTree",
			"webhook": h.public(),
		},
	}
	if sc.Tree != nil {
		env.Tree = sc.Tree.Name
	}
	if p != nil {
		env.Actor = p.Username
	}
	d, err := s.newDelivery(ctx, h.ID, env, false)
	if err != nil {
		return nil, err
	}
	return s.runManual(ctx, h, d)
}

func (s *Service) runManual(ctx context.Context, h *hookDoc, d *domain.WebhookDelivery) (*domain.WebhookDelivery, error) {
	out, err := s.process(ctx, job{tenantID: h.TenantID, hookID: h.ID, key: DeliveryKey(d), manual: true})
	if err != nil {
		return nil, err
	}
	if out == nil {
		return nil, errorf(domain.ErrNotFound, "webhook %q not found", h.ID)
	}
	return out, nil
}
