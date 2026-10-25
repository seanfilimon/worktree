package httpapi

import (
	"errors"
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/blame"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// maxBlameLine bounds the start/end query parameters of a blame.
const maxBlameLine = 1 << 30

// registerBlameRoutes registers the v0.6 routes for file blame (§14.4).
func (a *api) registerBlameRoutes(tree string) {
	a.handle("GET "+tree+"/blame/{rev}/{path...}", a.handleBlame)
}

// blameResponse is the body of GET {tree}/blame/{rev}/{path...}.
type blameResponse struct {
	Lines    []domain.BlameLine `json:"lines"`
	Snapshot domain.Snapshot    `json:"snapshot"` // the snapshot REV resolved to
}

// handleBlame answers GET /api/v1/trees/{tenant}/{tree}/blame/{rev}/{path...}
// with every line of the file at REV attributed to the snapshot that last
// changed it (see package blame). REV is a snapshot id, branch, tag or
// unique id prefix (snapshotForRev). Optional ?start=N&end=M restrict the
// result to lines N..M (1-based, inclusive; end is clamped).
//
// Read checks are those of the blob endpoint (§13.1): tree:read, then a
// path-denied path answers 403 path_denied and a license-restricted one
// 451 license_restricted, before the file is looked up so that its
// existence is not revealed. A missing file is 404, a binary file 422
// binary_file, a file over blame.MaxFileSize 413 payload_too_large and a
// bad range 400.
func (a *api) handleBlame(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	filePath := r.PathValue("path")
	if !validTreePath(filePath) {
		return badRequestf("invalid file path %q", filePath)
	}
	start, err := queryInt(r, "start", 0, maxBlameLine)
	if err != nil {
		return err
	}
	end, err := queryInt(r, "end", 0, maxBlameLine)
	if err != nil {
		return err
	}
	ctx := r.Context()
	s, err := a.snapshotForRev(ctx, tree, r.PathValue("rev"))
	if err != nil {
		return err
	}
	rs, err := a.readRestrictions(ctx, principal(r), tree, []string{filePath})
	if err != nil {
		return err
	}
	if err := rs.check(filePath); err != nil {
		return err
	}
	lines, err := blame.New(a.d.Store, a.d.Objects).File(ctx, s, filePath, blame.Options{Start: start, End: end})
	switch {
	case errors.Is(err, blame.ErrNoFile):
		return notFoundf("file %q not found in snapshot %s", filePath, s.ID)
	case errors.Is(err, blame.ErrBinary):
		return &apiError{status: http.StatusUnprocessableEntity, code: "binary_file", msg: filePath + " is a binary file"}
	case errors.Is(err, blame.ErrTooLarge):
		return &apiError{status: http.StatusRequestEntityTooLarge, code: "payload_too_large", msg: err.Error()}
	case errors.Is(err, blame.ErrRange):
		return badRequestf("%s", err.Error())
	case err != nil:
		return err
	}
	normalizeSnapshot(s)
	writeJSON(w, http.StatusOK, blameResponse{Lines: nonNil(lines), Snapshot: *s})
	return nil
}
