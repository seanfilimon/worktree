package iam

import (
	"context"

	"github.com/ramizik/worktree/server-go/internal/auth"
)

type Decision int

const (
	Allow Decision = iota
	Deny
)

func (d Decision) String() string {
	if d == Allow {
		return "allow"
	}
	return "deny"
}

// Authorizer evaluates whether principal may perform action on resource.
// action uses the canonical permission name (e.g. "staged:create").
// resource is a slash-delimited path: tenant/worktree/branch/snapshot_id.
type Authorizer interface {
	Authorize(ctx context.Context, principal auth.Principal, action string, resource string) (Decision, error)
}

// AllowAllAuthorizer permits every request. Used in development.
type AllowAllAuthorizer struct{}

func (AllowAllAuthorizer) Authorize(_ context.Context, _ auth.Principal, _, _ string) (Decision, error) {
	return Allow, nil
}

// DenyAllAuthorizer rejects every request. Used in tests.
type DenyAllAuthorizer struct{}

func (DenyAllAuthorizer) Authorize(_ context.Context, _ auth.Principal, _, _ string) (Decision, error) {
	return Deny, nil
}
