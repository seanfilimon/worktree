package httpapi

import (
	"github.com/seanfilimon/worktree/services/server-go/internal/fork"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// forks (§16.2): register the component schemas of the fork routes with
// the OpenAPI schema checks (kept in this file so that the shared
// openAPITypes table needs no edit).
func init() {
	for name, v := range map[string]any{
		"Fork":              fork.Fork{},
		"ForkUpstream":      fork.Upstream{},
		"ForkBranchStatus":  fork.BranchStatus{},
		"ForkSyncResult":    fork.SyncResult{},
		"ForkSyncBranch":    fork.SyncBranch{},
		"CreateForkRequest": createForkRequest{},
		"SyncForkRequest":   syncForkRequest{},
		"MRSource":          review.MRSource{},
	} {
		openAPITypes[name] = v
	}
	openAPICommentDirs = append(openAPICommentDirs, "../fork")
}
