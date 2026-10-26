package httpapi

// v0.8 history-v2 (docs/ENGINEERING_PLAN.md §16.10): the server reflog of
// branch moves (pushes and force pushes), recorded by internal/syncsvc.

import (
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/syncsvc"
)

const (
	defaultReflogLimit = 50
	maxReflogLimit     = 500
)

// reflogResponse is the body of GET {tree}/reflog.
type reflogResponse struct {
	Entries []syncsvc.ReflogEntry `json:"entries"`
}

// registerReflogRoutes registers GET {tree}/reflog.
func (a *api) registerReflogRoutes(tree string) {
	a.handle("GET "+tree+"/reflog", a.handleReflog)
}

// handleReflog answers GET {tree}/reflog?branch=&limit=50: the branch moves
// of the tree (one branch when given), newest first. Force pushes have
// kind "force_push" and keep the rewound tip in old_tip.
func (a *api) handleReflog(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	branch := r.URL.Query().Get("branch")
	if len(branch) > maxRefNameLen {
		return badRequestf("branch must be at most %d bytes", maxRefNameLen)
	}
	limit, err := queryInt(r, "limit", defaultReflogLimit, maxReflogLimit)
	if err != nil {
		return err
	}
	entries, err := syncsvc.ListReflog(r.Context(), a.d.Store, tree.ID, branch, limit)
	if err != nil {
		return err
	}
	if entries == nil {
		entries = []syncsvc.ReflogEntry{}
	}
	writeJSON(w, http.StatusOK, reflogResponse{Entries: entries})
	return nil
}
