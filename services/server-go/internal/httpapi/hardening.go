package httpapi

import (
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
)

// Performance helpers (hardening, docs/ENGINEERING_PLAN.md §15.10).

// withAuthzRequestCache returns r with an authorization request cache
// (authz.WithRequestCache): every Authorize/Evaluate made while serving r
// reads the caller's account, memberships, teams, grants and each
// tenant's policy list from the store once. Use it only in read-only
// handlers that authorize many times (one check per tree, per path
// batch...), where nothing the cache holds can change mid-request.
//
// Measured on code search over 9 trees of a tenant with 300 policies: the
// policy list was decoded from bbolt twice per tree per search.
func withAuthzRequestCache(r *http.Request) *http.Request {
	return r.WithContext(authz.WithRequestCache(r.Context()))
}
