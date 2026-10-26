package config

import (
	"slices"
	"strings"
	"testing"
	"time"
)

// hardening (§15.10): for tests. --webhook-retry-schedule /
// WT_WEBHOOK_RETRY_SCHEDULE.
func TestWebhookRetrySchedule(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil || cfg.WebhookRetrySchedule != nil || cfg.WebhookPollInterval() != 0 {
		t.Fatalf("default = %v (%s), %v", cfg.WebhookRetrySchedule, cfg.WebhookPollInterval(), err)
	}
	cfg, err = load(nil, env(map[string]string{"WT_WEBHOOK_RETRY_SCHEDULE": "1s, 2s"}))
	if err != nil || !slices.Equal(cfg.WebhookRetrySchedule, []time.Duration{time.Second, 2 * time.Second}) {
		t.Fatalf("env = %v, %v", cfg.WebhookRetrySchedule, err)
	}
	if got := cfg.WebhookPollInterval(); got != 250*time.Millisecond {
		t.Fatalf("poll interval = %s", got)
	}
	cfg, err = load([]string{"--webhook-retry-schedule", "100ms,3s,50ms"}, env(map[string]string{"WT_WEBHOOK_RETRY_SCHEDULE": "1s"}))
	if err != nil || !slices.Equal(cfg.WebhookRetrySchedule, []time.Duration{100 * time.Millisecond, 3 * time.Second, 50 * time.Millisecond}) {
		t.Fatalf("flag over env = %v, %v", cfg.WebhookRetrySchedule, err)
	}
	if got := cfg.WebhookPollInterval(); got != 50*time.Millisecond {
		t.Fatalf("poll interval floor = %s", got)
	}
	cfg.WebhookRetrySchedule = []time.Duration{time.Hour}
	if got := cfg.WebhookPollInterval(); got != 5*time.Second {
		t.Fatalf("poll interval cap = %s", got)
	}

	bad := []string{"", ",", "soon", "1s,x", "0s", "-1s", "25h", "1s,1s,1s,1s,1s,1s,1s,1s,1s,1s,1s"}
	for _, v := range bad {
		if v != "" {
			if _, err := load(nil, env(map[string]string{"WT_WEBHOOK_RETRY_SCHEDULE": v})); err == nil {
				t.Errorf("env %q accepted", v)
			}
		}
		if _, err := load([]string{"--webhook-retry-schedule=" + v}, env(nil)); err == nil {
			t.Errorf("flag %q accepted", v)
		}
	}

	var b strings.Builder
	Usage(&b)
	if !strings.Contains(b.String(), "(testing only)") || !strings.Contains(b.String(), "WT_WEBHOOK_RETRY_SCHEDULE") {
		t.Fatalf("usage lacks the testing-only retry schedule flag:\n%s", b.String())
	}
}
