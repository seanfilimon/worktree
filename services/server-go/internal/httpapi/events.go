package httpapi

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// heartbeatInterval is the period of SSE keep-alive comments. A variable so
// tests can shorten it.
var heartbeatInterval = 15 * time.Second

// handleEvents answers GET /api/v1/events?tenant=&tree=&types= with a
// text/event-stream of domain events.
//
// Authorization: with tree, tree:read on it; with only tenant, tenant:read;
// with neither, server admins get every event (the firehose) and other
// signed-in accounts get the events of the tenants they are members of.
// types is an optional comma-separated list of event types.
func (a *api) handleEvents(w http.ResponseWriter, r *http.Request) error {
	ctx := r.Context()
	q := r.URL.Query()
	p := principal(r)
	var f domain.EventFilter
	// allow further restricts the subscription; nil admits every event.
	var allow func(domain.Event) bool
	switch tenantRef, treeName := q.Get("tenant"), q.Get("tree"); {
	case treeName != "" && tenantRef == "":
		return badRequestf("tree requires tenant")
	case treeName != "":
		t, err := a.tenantByRef(ctx, tenantRef)
		if err != nil {
			return err
		}
		tree, err := a.d.Store.GetTreeByName(ctx, t.ID, treeName)
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("tree %s/%s not found", t.Slug, treeName)
		}
		if err != nil {
			return err
		}
		if err := a.d.Authz.Authorize(ctx, p, domain.ActTreeRead, t.ID, tree); err != nil {
			return err
		}
		f.TenantID, f.TreeID = t.ID, tree.ID
	case tenantRef != "":
		t, err := a.tenantByRef(ctx, tenantRef)
		if err != nil {
			return err
		}
		if err := a.d.Authz.Authorize(ctx, p, domain.ActTenantRead, t.ID, nil); err != nil {
			return err
		}
		f.TenantID = t.ID
	default:
		if _, err := requireLogin(r); err != nil {
			return err
		}
		err := a.d.Authz.Authorize(ctx, p, domain.ActServerAdmin, "", nil)
		switch {
		case err == nil:
		case errors.Is(err, domain.ErrForbidden):
			members, err := a.memberTenantIDs(ctx, p)
			if err != nil {
				return err
			}
			allow = func(e domain.Event) bool { return members[e.TenantID] }
		default:
			return err
		}
	}
	if types := q.Get("types"); types != "" {
		for _, t := range strings.Split(types, ",") {
			if t = strings.TrimSpace(t); t != "" {
				f.Types = append(f.Types, t)
			}
		}
	}

	rc := http.NewResponseController(w)
	// The stream outlives any server write timeout.
	_ = rc.SetWriteDeadline(time.Time{})

	var events <-chan domain.Event
	if a.d.Bus != nil {
		ch, cancel := a.d.Bus.Subscribe(f)
		defer cancel()
		events = ch
	}

	h := w.Header()
	h.Set("Content-Type", "text/event-stream")
	h.Set("Cache-Control", "no-cache")
	h.Set("X-Accel-Buffering", "no") // disable proxy buffering (nginx)
	w.WriteHeader(http.StatusOK)
	if _, err := io.WriteString(w, ": connected\nretry: 5000\n\n"); err != nil {
		return nil
	}
	if err := rc.Flush(); err != nil {
		a.log.WarnContext(ctx, "sse: response does not support flushing", slog.Any("error", err))
		return nil
	}

	ticker := time.NewTicker(heartbeatInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return nil // client went away
		case <-a.d.Done:
			return nil // server shutting down (nil channel blocks forever)
		case <-ticker.C:
			if _, err := io.WriteString(w, ": heartbeat\n\n"); err != nil {
				return nil
			}
		case e, ok := <-events:
			if !ok {
				return nil // bus closed the subscription
			}
			if allow != nil && !allow(e) {
				continue
			}
			if err := writeSSE(w, e); err != nil {
				return nil
			}
		}
		if err := rc.Flush(); err != nil {
			return nil
		}
	}
}

// memberTenantIDs is the set of tenants p belongs to (including the
// personal tenant).
func (a *api) memberTenantIDs(ctx context.Context, p *domain.Principal) (map[string]bool, error) {
	ms, err := a.d.Store.ListAccountMemberships(ctx, p.AccountID)
	if err != nil {
		return nil, err
	}
	ids := map[string]bool{p.TenantID: true}
	for _, m := range ms {
		ids[m.TenantID] = true
	}
	return ids, nil
}

// writeSSE writes one event frame: "event: <type>\ndata: <json>\n\n".
func writeSSE(w io.Writer, e domain.Event) error {
	data, err := json.Marshal(e)
	if err != nil {
		// Unencodable payload: send the envelope without it rather than
		// dropping the event.
		e.Payload = nil
		if data, err = json.Marshal(e); err != nil {
			return nil
		}
	}
	typ := strings.Map(func(r rune) rune {
		if r == '\n' || r == '\r' {
			return -1
		}
		return r
	}, e.Type)
	_, err = fmt.Fprintf(w, "event: %s\ndata: %s\n\n", typ, data)
	return err
}
