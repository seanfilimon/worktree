package iam

import (
	"sync"
	"time"
)

// Login rate limiting defaults.
const (
	DefaultMaxLoginFailures   = 10
	DefaultLoginFailureWindow = 5 * time.Minute
	limiterSweepThreshold     = 4096
)

// loginLimiter counts failed logins per key within a fixed window that
// starts at the first failure. It is safe for concurrent use.
type loginLimiter struct {
	mu      sync.Mutex
	max     int
	window  time.Duration
	entries map[string]*limiterEntry
}

type limiterEntry struct {
	start    time.Time
	failures int
}

func newLoginLimiter(max int, window time.Duration) *loginLimiter {
	return &loginLimiter{max: max, window: window, entries: make(map[string]*limiterEntry)}
}

// blocked reports whether key has exhausted its failures for the current
// window.
func (l *loginLimiter) blocked(key string, now time.Time) bool {
	if l == nil {
		return false
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	e, ok := l.entries[key]
	if !ok {
		return false
	}
	if now.Sub(e.start) >= l.window {
		delete(l.entries, key)
		return false
	}
	return e.failures >= l.max
}

// fail records a failed attempt for key.
func (l *loginLimiter) fail(key string, now time.Time) {
	if l == nil {
		return
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	if len(l.entries) >= limiterSweepThreshold {
		l.sweepLocked(now)
	}
	e, ok := l.entries[key]
	if !ok || now.Sub(e.start) >= l.window {
		l.entries[key] = &limiterEntry{start: now, failures: 1}
		return
	}
	e.failures++
}

// reset forgets key (after a successful login).
func (l *loginLimiter) reset(key string) {
	if l == nil {
		return
	}
	l.mu.Lock()
	delete(l.entries, key)
	l.mu.Unlock()
}

func (l *loginLimiter) sweepLocked(now time.Time) {
	for k, e := range l.entries {
		if now.Sub(e.start) >= l.window {
			delete(l.entries, k)
		}
	}
}
