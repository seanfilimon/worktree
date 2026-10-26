package httpapi

import "github.com/seanfilimon/worktree/services/server-go/internal/staged"

// staged-v2 (§16.8): register the component schemas of the staged history
// and overlaps routes with the OpenAPI schema checks (kept in this file so
// that the shared openAPITypes table needs no edit).
func init() {
	for name, v := range map[string]any{
		"StagedHistoryResponse":  stagedHistoryResponse{},
		"StagedHistoryEntry":     staged.HistoryEntry{},
		"StagedOverlapsResponse": stagedOverlapsResponse{},
		"StagedOverlap":          staged.StagedOverlap{},
	} {
		openAPITypes[name] = v
	}
	openAPICommentDirs = append(openAPICommentDirs, "../staged")
}
