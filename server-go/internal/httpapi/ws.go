package httpapi

import (
	"context"
	"net/http"
	"sync"
	"time"

	"github.com/coder/websocket"
	"github.com/coder/websocket/wsjson"
	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/staged"
)

// StagedBroadcaster defines the interface for publishing and subscribing to staged snapshots.
type StagedBroadcaster interface {
	Publish(snapshot staged.Snapshot)
	Subscribe(tenant, worktree string) (<-chan staged.Snapshot, func())
}

// InMemoryHub is a simple in-memory pub/sub broker for staged snapshots.
type InMemoryHub struct {
	mu          sync.RWMutex
	subscribers map[int]*subscriber
	nextID      int
}

type subscriber struct {
	ch       chan staged.Snapshot
	tenant   string
	worktree string
}

func NewInMemoryHub() *InMemoryHub {
	return &InMemoryHub{
		subscribers: make(map[int]*subscriber),
	}
}

func (h *InMemoryHub) Publish(snapshot staged.Snapshot) {
	h.mu.RLock()
	defer h.mu.RUnlock()

	for _, sub := range h.subscribers {
		// Filter by tenant if specified
		if sub.tenant != "" && sub.tenant != snapshot.Tenant {
			continue
		}
		// Filter by worktree if specified
		if sub.worktree != "" && sub.worktree != snapshot.Worktree {
			continue
		}

		// Non-blocking send
		select {
		case sub.ch <- snapshot:
		default:
			// If the channel is full, we drop the event for this subscriber
			// to avoid blocking the publisher.
		}
	}
}

func (h *InMemoryHub) Subscribe(tenant, worktree string) (<-chan staged.Snapshot, func()) {
	h.mu.Lock()
	defer h.mu.Unlock()

	id := h.nextID
	h.nextID++

	ch := make(chan staged.Snapshot, 64)
	h.subscribers[id] = &subscriber{
		ch:       ch,
		tenant:   tenant,
		worktree: worktree,
	}

	unsubscribe := func() {
		h.mu.Lock()
		defer h.mu.Unlock()
		if sub, ok := h.subscribers[id]; ok {
			close(sub.ch)
			delete(h.subscribers, id)
		}
	}

	return ch, unsubscribe
}

// HandleStagedWS is the WebSocket handler for real-time staged snapshot streaming.
func HandleStagedWS(broadcaster StagedBroadcaster) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// Enforce authentication
		principal, ok := auth.PrincipalFromContext(r.Context())
		if !ok || !principal.Authenticated {
			writeError(w, http.StatusUnauthorized, "Unauthorized", "authentication required")
			return
		}

		// Extract filters
		tenant := r.URL.Query().Get("tenant")
		worktree := r.URL.Query().Get("worktree")

		// If principal has a tenant constraint, enforce it
		if principal.Tenant != "" {
			if tenant != "" && tenant != principal.Tenant {
				writeError(w, http.StatusForbidden, "Forbidden", "tenant mismatch")
				return
			}
			tenant = principal.Tenant
		}

		// Upgrade to WebSocket
		c, err := websocket.Accept(w, r, &websocket.AcceptOptions{
			// Be restrictive in production, but allow origins for local dev/demo
			InsecureSkipVerify: true,
		})
		if err != nil {
			// websocket.Accept writes the error to the response
			return
		}
		defer c.Close(websocket.StatusInternalError, "the sky is falling")

		ctx, cancel := context.WithCancel(r.Context())
		defer cancel()

		// Subscribe to the broadcaster
		ch, unsubscribe := broadcaster.Subscribe(tenant, worktree)
		defer unsubscribe()

		// Send a connection success message (optional, but good for client confirmation)
		err = wsjson.Write(ctx, c, map[string]string{"status": "connected", "tenant": tenant, "worktree": worktree})
		if err != nil {
			return
		}

		// Keep alive and read loop to handle client disconnects cleanly
		go func() {
			for {
				_, _, err := c.Read(ctx)
				if err != nil {
					cancel()
					return
				}
			}
		}()

		// Write loop
		for {
			select {
			case <-ctx.Done():
				c.Close(websocket.StatusNormalClosure, "client disconnected")
				return
			case snap, ok := <-ch:
				if !ok {
					c.Close(websocket.StatusNormalClosure, "hub closed")
					return
				}
				ctxWrite, cancelWrite := context.WithTimeout(ctx, 5*time.Second)
				err := wsjson.Write(ctxWrite, c, snap)
				cancelWrite()
				if err != nil {
					c.Close(websocket.StatusAbnormalClosure, "failed to write message")
					return
				}
			}
		}
	}
}
