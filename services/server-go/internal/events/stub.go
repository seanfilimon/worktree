// Package events implements domain.EventBus. STUB: replaced by the
// go-staged-events workstream (delete this file when implementing).
package events

import "github.com/seanfilimon/worktree/services/server-go/internal/domain"

// Bus is an in-process pub/sub bus.
type Bus struct{ domain.EventBus }

// NewBus returns an empty bus.
func NewBus() *Bus { return &Bus{} }
