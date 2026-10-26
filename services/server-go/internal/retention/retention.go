// Package retention prunes old operational data in the background
// (docs/ENGINEERING_PLAN.md §16.11): policy versions
// (--policy-version-retention), audit events (--audit-retention), webhook
// deliveries (--webhook-delivery-retention / --webhook-delivery-keep) and
// scheduled backups (--backup-max-age / --backup-keep).
//
// A pass runs when the server starts and then every Interval (default 1 h);
// server admins can trigger one (POST /api/v1/admin/retention/run). The
// garbage collector keeps pruning audit events on its own schedule too.
package retention

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strconv"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/audit"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/policyhist"
)

// DefaultInterval is the period of background passes.
const DefaultInterval = time.Hour

// ActionRun is the audit action of a pass that deleted something.
const ActionRun = "retention.run"

// ErrRunning is returned by Run while another pass is running.
var ErrRunning = errors.New("retention: a pass is already running")

// Rule is an age and/or count limit (zero = no limit).
type Rule struct {
	MaxAge time.Duration
	Keep   int
}

// Config are the limits of every category.
type Config struct {
	PolicyVersions    Rule
	Audit             time.Duration
	WebhookDeliveries Rule
	Backups           Rule
}

// DeliveryPruner prunes webhook deliveries (*webhooks.Service).
type DeliveryPruner interface {
	PruneDeliveries(ctx context.Context) (int, error)
}

// BackupPruner prunes scheduled backups (*backup.Service).
type BackupPruner interface {
	PruneAudited(ctx context.Context) ([]string, error)
}

// Deps are the collaborators. Store is required; Webhooks and Backups are
// optional.
type Deps struct {
	Store    domain.MetaStore
	Bus      domain.EventBus
	Audit    domain.AuditLog
	Logger   *slog.Logger
	Webhooks DeliveryPruner
	Backups  BackupPruner
}

// Report is the outcome of one pass.
type Report struct {
	At                      time.Time `json:"at"`
	PolicyVersionsPruned    int       `json:"policy_versions_pruned"`
	AuditPruned             int       `json:"audit_pruned"`
	WebhookDeliveriesPruned int       `json:"webhook_deliveries_pruned"`
	BackupsPruned           int       `json:"backups_pruned"`
	TookMS                  int64     `json:"took_ms"`
}

// RuleInfo describes an age and count limit (0 = none).
type RuleInfo struct {
	MaxAge        string `json:"max_age"`
	MaxAgeSeconds int64  `json:"max_age_seconds"`
	Keep          int    `json:"keep"`
}

// AgeInfo describes an age limit (0 = none).
type AgeInfo struct {
	MaxAge        string `json:"max_age"`
	MaxAgeSeconds int64  `json:"max_age_seconds"`
}

// ConfigInfo is the configuration as reported by the API.
type ConfigInfo struct {
	PolicyVersions    RuleInfo `json:"policy_versions"`
	Audit             AgeInfo  `json:"audit"`
	WebhookDeliveries RuleInfo `json:"webhook_deliveries"`
	Backups           RuleInfo `json:"backups"`
}

// Status is the response of GET /api/v1/admin/retention.
type Status struct {
	// Interval is the period of background passes (Go duration).
	Interval string     `json:"interval"`
	Config   ConfigInfo `json:"config"`
	// Running reports whether a pass is running now.
	Running    bool       `json:"running"`
	LastRunAt  *time.Time `json:"last_run_at,omitempty"`
	LastError  string     `json:"last_error,omitempty"`
	LastReport *Report    `json:"last_report,omitempty"`
}

// Service runs retention passes.
type Service struct {
	d        Deps
	cfg      Config
	interval time.Duration
	now      func() time.Time

	run sync.Mutex // held during a pass

	mu      sync.Mutex
	last    *Report
	lastErr string
}

// New returns a Service; interval 0 = DefaultInterval.
func New(d Deps, cfg Config, interval time.Duration) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	if interval <= 0 {
		interval = DefaultInterval
	}
	return &Service{d: d, cfg: cfg, interval: interval, now: func() time.Time { return time.Now().UTC() }}
}

func ruleInfo(r Rule) RuleInfo {
	return RuleInfo{MaxAge: r.MaxAge.String(), MaxAgeSeconds: int64(r.MaxAge.Seconds()), Keep: r.Keep}
}

// Status returns the configuration and the last pass.
func (s *Service) Status() *Status {
	st := &Status{
		Interval: s.interval.String(),
		Config: ConfigInfo{
			PolicyVersions:    ruleInfo(s.cfg.PolicyVersions),
			Audit:             AgeInfo{MaxAge: s.cfg.Audit.String(), MaxAgeSeconds: int64(s.cfg.Audit.Seconds())},
			WebhookDeliveries: ruleInfo(s.cfg.WebhookDeliveries),
			Backups:           ruleInfo(s.cfg.Backups),
		},
	}
	if s.run.TryLock() {
		s.run.Unlock()
	} else {
		st.Running = true
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.last != nil {
		r := *s.last
		at := r.At
		st.LastReport, st.LastRunAt = &r, &at
	}
	st.LastError = s.lastErr
	return st
}

// Run performs one pass over every category. Errors of one category do
// not stop the others; they are joined into the returned error (the
// report still counts what was deleted).
func (s *Service) Run(ctx context.Context) (*Report, error) {
	if !s.run.TryLock() {
		return nil, ErrRunning
	}
	defer s.run.Unlock()
	started := s.now()
	rep := &Report{At: started}
	var errs []error

	if r := s.cfg.PolicyVersions; r.MaxAge > 0 || r.Keep > 0 {
		n, err := policyhist.New(s.d.Store, s.d.Bus, s.d.Logger).Prune(ctx, r.MaxAge, r.Keep)
		rep.PolicyVersionsPruned = n
		if err != nil {
			errs = append(errs, fmt.Errorf("policy versions: %w", err))
		}
	}
	if s.cfg.Audit > 0 {
		n, err := s.d.Store.PruneAudit(ctx, started.Add(-s.cfg.Audit))
		rep.AuditPruned = n
		if err != nil {
			errs = append(errs, fmt.Errorf("audit events: %w", err))
		}
	}
	if s.d.Webhooks != nil {
		n, err := s.d.Webhooks.PruneDeliveries(ctx)
		rep.WebhookDeliveriesPruned = n
		if err != nil {
			errs = append(errs, fmt.Errorf("webhook deliveries: %w", err))
		}
	}
	if s.d.Backups != nil {
		ids, err := s.d.Backups.PruneAudited(ctx)
		rep.BackupsPruned = len(ids)
		if err != nil {
			errs = append(errs, fmt.Errorf("backups: %w", err))
		}
	}
	rep.TookMS = s.now().Sub(started).Milliseconds()
	err := errors.Join(errs...)

	s.mu.Lock()
	s.last = rep
	s.lastErr = ""
	if err != nil {
		s.lastErr = err.Error()
	}
	s.mu.Unlock()

	total := rep.PolicyVersionsPruned + rep.AuditPruned + rep.WebhookDeliveriesPruned + rep.BackupsPruned
	if total > 0 {
		s.d.Logger.Info("retention pass", slog.Int("policy_versions", rep.PolicyVersionsPruned),
			slog.Int("audit", rep.AuditPruned), slog.Int("webhook_deliveries", rep.WebhookDeliveriesPruned),
			slog.Int("backups", rep.BackupsPruned), slog.Int64("took_ms", rep.TookMS))
		if s.d.Audit != nil {
			s.d.Audit.Record(context.WithoutCancel(ctx), audit.System(ActionRun, "server", "", map[string]string{
				"policy_versions_pruned":    strconv.Itoa(rep.PolicyVersionsPruned),
				"audit_pruned":              strconv.Itoa(rep.AuditPruned),
				"webhook_deliveries_pruned": strconv.Itoa(rep.WebhookDeliveriesPruned),
				"backups_pruned":            strconv.Itoa(rep.BackupsPruned),
			}))
		}
	}
	return rep, err
}

// Start runs a pass now and then every interval until ctx is done or stop
// is called.
func (s *Service) Start(ctx context.Context) (stop func()) {
	ctx, cancel := context.WithCancel(ctx)
	done := make(chan struct{})
	go func() {
		defer close(done)
		t := time.NewTicker(s.interval)
		defer t.Stop()
		for {
			if _, err := s.Run(ctx); err != nil && !errors.Is(err, ErrRunning) && ctx.Err() == nil {
				s.d.Logger.Warn("retention pass failed", slog.Any("error", err))
			}
			select {
			case <-ctx.Done():
				return
			case <-t.C:
			}
		}
	}()
	return func() {
		cancel()
		<-done
	}
}
