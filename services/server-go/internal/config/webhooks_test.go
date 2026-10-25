package config

import "testing"

// webhooks (§14.2): --webhook-allow-private / WT_WEBHOOK_ALLOW_PRIVATE.
func TestWebhookAllowPrivate(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil || cfg.WebhookAllowPrivate {
		t.Fatalf("default = %v, %v", cfg.WebhookAllowPrivate, err)
	}
	cfg, err = load(nil, env(map[string]string{"WT_WEBHOOK_ALLOW_PRIVATE": "true"}))
	if err != nil || !cfg.WebhookAllowPrivate {
		t.Fatalf("env = %v, %v", cfg.WebhookAllowPrivate, err)
	}
	cfg, err = load([]string{"--webhook-allow-private=false"}, env(map[string]string{"WT_WEBHOOK_ALLOW_PRIVATE": "1"}))
	if err != nil || cfg.WebhookAllowPrivate {
		t.Fatalf("flag over env = %v, %v", cfg.WebhookAllowPrivate, err)
	}
	cfg, err = load([]string{"--webhook-allow-private"}, env(nil))
	if err != nil || !cfg.WebhookAllowPrivate {
		t.Fatalf("flag = %v, %v", cfg.WebhookAllowPrivate, err)
	}
	if _, err := load(nil, env(map[string]string{"WT_WEBHOOK_ALLOW_PRIVATE": "maybe"})); err == nil {
		t.Fatal("invalid boolean accepted")
	}
}
