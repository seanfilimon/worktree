package httpapi

// v0.8 (docs/ENGINEERING_PLAN.md §16.8, workstream staged-v2): staged
// history and push-time overlaps.

import (
	"context"
	"net/http"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// stagedHistorian and stagedOverlapper are implemented by staged.Service
// (Deps.Staged is the shared domain.StagedService interface).
type stagedHistorian interface {
	History(ctx context.Context, p *domain.Principal, tree *domain.Tree, f staged.HistoryFilter) ([]staged.HistoryEntry, error)
}

type stagedOverlapper interface {
	Overlaps(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) ([]staged.StagedOverlap, error)
}

// stagedHistoryResponse is the body of GET {tree}/staged/history.
type stagedHistoryResponse struct {
	// History holds ended staged snapshots (status pushed, cleared or
	// expired), newest first.
	History []staged.HistoryEntry `json:"history"`
}

// stagedOverlapsResponse is the body of GET {tree}/staged/overlaps.
type stagedOverlapsResponse struct {
	// Overlaps are other accounts' live staged snapshots that touch the
	// queried paths (advisory: pushes are never blocked).
	Overlaps []staged.StagedOverlap `json:"overlaps"`
}

var validHistoryStatus = map[string]bool{
	staged.StatusPushed: true, staged.StatusCleared: true, staged.StatusExpired: true,
}

// handleStagedHistory answers GET /api/v1/trees/{tenant}/{tree}/staged/history?branch=&account=&status=&limit=.
func (a *api) handleStagedHistory(w http.ResponseWriter, r *http.Request) error {
	h, ok := a.d.Staged.(stagedHistorian)
	if a.d.Staged == nil || !ok {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented", msg: "staged history is not enabled on this server"}
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	q := r.URL.Query()
	f := staged.HistoryFilter{Branch: q.Get("branch"), Status: q.Get("status")}
	if f.Branch != "" {
		if err := validateBranchName(f.Branch); err != nil {
			return err
		}
	}
	if f.Status != "" && !validHistoryStatus[f.Status] {
		return badRequestf("status must be pushed, cleared or expired")
	}
	if v := q.Get("limit"); v != "" {
		n, err := strconv.Atoi(v)
		if err != nil || n < 1 || n > staged.MaxHistoryLimit {
			return badRequestf("limit must be between 1 and %d", staged.MaxHistoryLimit)
		}
		f.Limit = n
	}
	if ref := q.Get("account"); ref != "" {
		if f.AccountID, err = a.resolveAccountRef(r, ref); err != nil {
			return err
		}
	}
	entries, err := h.History(r.Context(), principal(r), tree, f)
	if err != nil {
		return err
	}
	if err := a.filterStagedFiles(r.Context(), principal(r), tree, len(entries), func(i int) *[]string { return &entries[i].FilesChanged }); err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, stagedHistoryResponse{History: nonNil(entries)})
	return nil
}

// handleStagedOverlaps answers GET /api/v1/trees/{tenant}/{tree}/staged/overlaps?paths=a,b&path=c.
//
// paths is comma separated (repeatable); path takes one path per value
// (for paths containing commas). Paths the caller may not read are never
// reported.
func (a *api) handleStagedOverlaps(w http.ResponseWriter, r *http.Request) error {
	o, ok := a.d.Staged.(stagedOverlapper)
	if a.d.Staged == nil || !ok {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented", msg: "staged overlaps are not enabled on this server"}
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	q := r.URL.Query()
	var paths []string
	for _, v := range q["paths"] {
		for _, s := range strings.Split(v, ",") {
			if s = strings.TrimSpace(s); s != "" {
				paths = append(paths, s)
			}
		}
	}
	for _, v := range q["path"] {
		if v != "" {
			paths = append(paths, v)
		}
	}
	if len(paths) > staged.MaxOverlapPaths {
		return badRequestf("at most %d paths per request", staged.MaxOverlapPaths)
	}
	out, err := o.Overlaps(r.Context(), p, tree, paths)
	if err != nil {
		return err
	}
	if err := a.filterStagedFiles(r.Context(), p, tree, len(out), func(i int) *[]string { return &out[i].Paths }); err != nil {
		return err
	}
	kept := out[:0]
	for _, ov := range out {
		if len(ov.Paths) > 0 {
			kept = append(kept, ov)
		}
	}
	writeJSON(w, http.StatusOK, stagedOverlapsResponse{Overlaps: nonNil(kept)})
	return nil
}

// filterStagedFiles removes, in place, the paths the caller may not read
// (path policies, license rules) from n path lists (staged metadata must
// not reveal restricted file names). Lists become non-nil.
func (a *api) filterStagedFiles(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int, list func(i int) *[]string) error {
	var all []string
	seen := map[string]bool{}
	for i := 0; i < n; i++ {
		for _, f := range *list(i) {
			if !seen[f] {
				seen[f] = true
				all = append(all, f)
			}
		}
	}
	rs, err := a.readRestrictions(ctx, p, tree, all)
	if err != nil {
		return err
	}
	for i := 0; i < n; i++ {
		l := list(i)
		kept := make([]string, 0, len(*l))
		for _, f := range *l {
			if !rs.restricted(f) {
				kept = append(kept, f)
			}
		}
		*l = kept
	}
	return nil
}
