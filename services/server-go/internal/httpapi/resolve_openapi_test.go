package httpapi

import (
	"github.com/seanfilimon/worktree/services/server-go/internal/merge3"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// conflict-ui (§16.9): register the component schemas of the conflict
// resolution routes with the OpenAPI schema checks (kept in this file so
// that the shared openAPITypes table needs no edit).
func init() {
	for name, v := range map[string]any{
		"ConflictReport": review.ConflictReport{},
		"ConflictFile":   review.ConflictFile{},
		"ConflictSide":   review.ConflictSide{},
		"ConflictRegion": merge3.Region{},
		"ResolveRequest": review.ResolveInput{},
		"ResolveFile":    review.ResolveFile{},
		"ResolveResult":  review.Resolved{},
	} {
		openAPITypes[name] = v
	}
	openAPICommentDirs = append(openAPICommentDirs, "../merge3")
}
