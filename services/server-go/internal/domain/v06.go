package domain

import (
	"encoding/json"
	"time"
)

// v0.6 feature expansion (docs/ENGINEERING_PLAN.md §14).

// Merge methods for merge requests (MergeRequest.MergeMethod, protection
// BranchProtection.MergeMethods).
const (
	MergeFastForward = "fast-forward" // move the target tip to the source tip
	MergeCommit      = "merge"        // server-side three-way merge snapshot
	MergeSquash      = "squash"       // one new snapshot with the combined changes
)

// Webhook delivers bus events to an HTTP endpoint. TreeID "" = every tree
// of the tenant.
type Webhook struct {
	ID          string `json:"id"`
	TenantID    string `json:"tenant_id"`
	TreeID      string `json:"tree_id,omitempty"`
	URL         string `json:"url"`
	Description string `json:"description,omitempty"`
	// Events are event-type patterns ("branch.pushed", "mr.*", "*").
	Events []string `json:"events"`
	Active bool     `json:"active"`
	// Secret signs deliveries (X-WT-Signature: sha256=<hex HMAC>). Never
	// returned by the API after creation (HasSecret reports it).
	Secret    string    `json:"-"`
	HasSecret bool      `json:"has_secret"`
	CreatedBy string    `json:"created_by"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`

	LastDeliveryAt *time.Time `json:"last_delivery_at,omitempty"`
	LastStatus     int        `json:"last_status,omitempty"` // HTTP status of the last attempt (0 = network error)
	FailureCount   int        `json:"failure_count"`         // consecutive failed deliveries
}

// WebhookDelivery is one delivery (with its retries) of an event.
type WebhookDelivery struct {
	ID          string          `json:"id"`
	WebhookID   string          `json:"webhook_id"`
	Event       string          `json:"event"`
	Payload     json.RawMessage `json:"payload"`
	Attempts    int             `json:"attempts"`
	StatusCode  int             `json:"status_code"`
	Response    string          `json:"response,omitempty"` // first 2 KiB of the response body
	Error       string          `json:"error,omitempty"`
	DurationMS  int64           `json:"duration_ms"`
	Succeeded   bool            `json:"succeeded"`
	CreatedAt   time.Time       `json:"created_at"`
	DeliveredAt *time.Time      `json:"delivered_at,omitempty"`
	NextRetryAt *time.Time      `json:"next_retry_at,omitempty"`
	Redelivery  bool            `json:"redelivery,omitempty"`
}

// PolicyVersion is a stored revision of a policy (every create, update,
// delete and rollback appends one).
type PolicyVersion struct {
	TenantID  string    `json:"tenant_id"`
	PolicyID  string    `json:"policy_id"`
	Version   int       `json:"version"`
	Change    string    `json:"change"` // "create" | "update" | "delete" | "rollback"
	Policy    Policy    `json:"policy"` // the policy after the change (deletes: before)
	ChangedBy string    `json:"changed_by"`
	ChangedAt time.Time `json:"changed_at"`
	Comment   string    `json:"comment,omitempty"`
}

// CodeSearchHit is one matching line of a code search.
type CodeSearchHit struct {
	TenantSlug string   `json:"tenant"`
	Tree       string   `json:"tree"`
	Branch     string   `json:"branch"`
	SnapshotID string   `json:"snapshot_id"`
	Path       string   `json:"path"`
	Line       int      `json:"line"` // 1-based
	Text       string   `json:"text"`
	Before     []string `json:"before,omitempty"` // context lines
	After      []string `json:"after,omitempty"`
}

// BlameLine attributes one line of a file to the snapshot that last
// changed it.
type BlameLine struct {
	Line       int       `json:"line"` // 1-based
	Text       string    `json:"text"`
	SnapshotID string    `json:"snapshot_id"`
	Author     string    `json:"author"`
	Message    string    `json:"message"` // first line
	At         time.Time `json:"at"`
}

// v0.6 events.
const (
	EventWebhookFailing = "webhook.failing" // a webhook was disabled after repeated failures
	EventPolicyChanged  = "policy.changed"
)
