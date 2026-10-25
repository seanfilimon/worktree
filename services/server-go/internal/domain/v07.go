package domain

import "time"

// v0.7 build-out (docs/ENGINEERING_PLAN.md §15).

// Notification is one item of an account's inbox.
type Notification struct {
	ID        string `json:"id"`
	AccountID string `json:"account_id"`
	// Reason: "review_requested" | "mentioned" | "ci_failed" | "todo_assigned" |
	// "mr_merged" | "mr_commented" | "release" | "watching".
	Reason   string `json:"reason"`
	Event    string `json:"event"` // bus event type that caused it
	TenantID string `json:"tenant_id"`
	TreeID   string `json:"tree_id,omitempty"`
	Title    string `json:"title"`
	Body     string `json:"body,omitempty"`
	// URL is an admin-panel path, e.g. /trees/acme/web/merge-requests/3.
	URL       string     `json:"url,omitempty"`
	Actor     string     `json:"actor,omitempty"`
	CreatedAt time.Time  `json:"created_at"`
	ReadAt    *time.Time `json:"read_at,omitempty"`
}

// WatchSetting is an account's subscription level for a tree (or a whole
// tenant when TreeID is "").
type WatchSetting struct {
	AccountID string `json:"account_id"`
	TenantID  string `json:"tenant_id"`
	TreeID    string `json:"tree_id,omitempty"`
	// Level: "participating" (default: only what involves you) | "watching"
	// (every merge request, release and CI failure) | "ignoring".
	Level string    `json:"level"`
	Email bool      `json:"email"` // also send emails (when SMTP is configured)
	At    time.Time `json:"updated_at"`
}

// Notification watch levels.
const (
	WatchParticipating = "participating"
	WatchWatching      = "watching"
	WatchIgnoring      = "ignoring"
)

// PathOwnerRule makes accounts or teams owners of paths of a tree
// (CODEOWNERS-style; the last matching rule wins).
type PathOwnerRule struct {
	Pattern string   `json:"pattern"` // "**" glob, '/'-separated
	Owners  []string `json:"owners"`  // "user:<name>" | "team:<slug>"
}

// PathOwners is the owner configuration of a tree.
type PathOwners struct {
	TreeID    string          `json:"tree_id"`
	Rules     []PathOwnerRule `json:"rules"`
	UpdatedBy string          `json:"updated_by"`
	UpdatedAt time.Time       `json:"updated_at"`
}

// Mirror pushes a tree's branches and tags to a Git remote.
type Mirror struct {
	ID       string `json:"id"`
	TenantID string `json:"tenant_id"`
	TreeID   string `json:"tree_id"`
	URL      string `json:"url"` // https://... or ssh://... Git remote
	// Credential for the remote (token or "user:password"); never returned.
	Credential    string     `json:"-"`
	HasCredential bool       `json:"has_credential"`
	Branches      []string   `json:"branches,omitempty"` // glob patterns; empty = all
	Tags          bool       `json:"tags"`
	Active        bool       `json:"active"`
	CreatedBy     string     `json:"created_by"`
	CreatedAt     time.Time  `json:"created_at"`
	LastSyncAt    *time.Time `json:"last_sync_at,omitempty"`
	LastStatus    string     `json:"last_status,omitempty"` // "ok" | "error" | "running"
	LastError     string     `json:"last_error,omitempty"`
}

// BackupInfo describes one server backup.
type BackupInfo struct {
	ID        string    `json:"id"`
	Path      string    `json:"path"` // file path on the server
	SizeBytes int64     `json:"size_bytes"`
	Objects   int64     `json:"objects"`
	CreatedAt time.Time `json:"created_at"`
	CreatedBy string    `json:"created_by"`
	Note      string    `json:"note,omitempty"`
}

// v0.7 events.
const (
	EventMRReviewRequested = "mr.review_requested"
	EventMRReady           = "mr.ready" // a draft was marked ready
	EventSuggestionApplied = "mr.suggestion_applied"
	EventMirrorSynced      = "mirror.synced"
	EventMirrorFailed      = "mirror.failed"
	EventNotification      = "notification.created"
)
