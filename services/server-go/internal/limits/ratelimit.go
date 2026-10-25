package limits

import (
	"context"
	"math"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Rate-limiter tuning.
const (
	// bucketIdle is how long an untouched bucket is kept.
	bucketIdle = 10 * time.Minute
	// sweepEvery bounds how often idle buckets are swept.
	sweepEvery = time.Minute
	// tenantRateTTL caches a tenant's RequestsPerMinute override.
	tenantRateTTL = 30 * time.Second
)

// Decision is the outcome of one rate-limit check.
type Decision struct {
	Allowed bool
	// Limit is the budget per minute (0 = rate limiting disabled; the
	// other fields are then meaningless).
	Limit int
	// Remaining is the number of whole requests left in the bucket.
	Remaining int
	// RetryAfter is how long until the next request would be allowed
	// (only when !Allowed).
	RetryAfter time.Duration
}

type bucket struct {
	tokens float64
	last   time.Time
	limit  int
}

type tenantRate struct {
	rpm int
	at  time.Time
}

// rateLimiter is a token bucket per key: capacity = limit, refilled at
// limit/60 tokens per second.
type rateLimiter struct {
	now func() time.Time

	mu        sync.Mutex
	buckets   map[string]*bucket
	lastSweep time.Time
	tenants   map[string]tenantRate
}

func newRateLimiter(now func() time.Time) *rateLimiter {
	return &rateLimiter{now: now, buckets: map[string]*bucket{}, tenants: map[string]tenantRate{}}
}

func (r *rateLimiter) take(key string, limit int) Decision {
	now := r.now()
	r.mu.Lock()
	defer r.mu.Unlock()
	if now.Sub(r.lastSweep) >= sweepEvery {
		for k, b := range r.buckets {
			if now.Sub(b.last) > bucketIdle {
				delete(r.buckets, k)
			}
		}
		r.lastSweep = now
	}
	b := r.buckets[key]
	if b == nil || b.limit != limit {
		b = &bucket{tokens: float64(limit), last: now, limit: limit}
		r.buckets[key] = b
	}
	perSec := float64(limit) / 60
	if el := now.Sub(b.last).Seconds(); el > 0 {
		b.tokens = math.Min(float64(limit), b.tokens+el*perSec)
	}
	b.last = now
	if b.tokens >= 1 {
		b.tokens--
		return Decision{Allowed: true, Limit: limit, Remaining: int(b.tokens)}
	}
	wait := time.Duration((1 - b.tokens) / perSec * float64(time.Second))
	return Decision{Allowed: false, Limit: limit, Remaining: 0, RetryAfter: wait}
}

// RateKey identifies the bucket of a caller: the account, the service
// account, or the remote IP for anonymous callers.
func RateKey(p *domain.Principal, ip string) string {
	switch {
	case p == nil:
		return "ip:" + ip
	case p.ServiceAccountID != "":
		return "sa:" + p.ServiceAccountID
	case p.AccountID != "":
		return "acct:" + p.AccountID
	default:
		return "user:" + p.Username
	}
}

// Allow charges one request to the caller's bucket. The budget is the
// principal tenant's RequestsPerMinute when set, else the server default;
// anonymous callers always use the server default. A zero budget allows
// everything (Decision.Limit 0).
func (l *Limiter) Allow(ctx context.Context, p *domain.Principal, ip string) Decision {
	limit := l.d.RateLimit
	if p != nil && p.TenantID != "" {
		if rpm := l.tenantRPM(ctx, p.TenantID); rpm > 0 {
			limit = rpm
		}
	}
	if limit <= 0 {
		return Decision{Allowed: true}
	}
	return l.rate.take(RateKey(p, ip), limit)
}

// tenantRPM returns the tenant's RequestsPerMinute override (0 = none),
// cached for tenantRateTTL.
func (l *Limiter) tenantRPM(ctx context.Context, tenantID string) int {
	now := l.d.Now()
	r := l.rate
	r.mu.Lock()
	c, ok := r.tenants[tenantID]
	r.mu.Unlock()
	if ok && now.Sub(c.at) < tenantRateTTL {
		return c.rpm
	}
	rpm := 0
	if t, err := l.d.Store.GetTenant(ctx, tenantID); err == nil && t.Limits != nil {
		rpm = t.Limits.RequestsPerMinute
	}
	r.mu.Lock()
	if len(r.tenants) > 10000 {
		r.tenants = map[string]tenantRate{}
	}
	r.tenants[tenantID] = tenantRate{rpm: rpm, at: now}
	r.mu.Unlock()
	return rpm
}

// ForgetTenant drops the cached rate override of a tenant (call after
// changing its limits).
func (l *Limiter) ForgetTenant(tenantID string) {
	l.rate.mu.Lock()
	delete(l.rate.tenants, tenantID)
	l.rate.mu.Unlock()
}
