// Package meta implements domain.MetaStore. STUB: replaced by the go-store
// workstream (delete this file when implementing).
package meta

import (
	"errors"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Bolt is the bbolt-backed MetaStore.
type Bolt struct{ domain.MetaStore }

// NewBolt opens (creating if needed) the database at path.
func NewBolt(path string) (*Bolt, error) { return nil, errors.New("meta: not implemented") }

// Memory is an in-memory MetaStore for tests.
type Memory struct{ domain.MetaStore }

// NewMemory returns an empty in-memory store.
func NewMemory() *Memory { return &Memory{} }

// NewHistory returns a domain.History over store.
func NewHistory(store domain.MetaStore) domain.History { return nil }
