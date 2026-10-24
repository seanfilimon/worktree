// Package audit implements domain.AuditLog. STUB: replaced by the
// go-audit-license workstream (delete this file when implementing).
package audit

import (
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Log records audit events into the MetaStore.
type Log struct{ domain.AuditLog }

// New returns an audit log.
func New(store domain.MetaStore, log *slog.Logger) *Log { return &Log{} }
