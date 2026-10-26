package domain

// v0.8 (docs/ENGINEERING_PLAN.md §16).

// v0.8 actions. Role minimums: tree:fork viewer (on the source tree; the
// caller also needs tree:create in the destination tenant),
// branch:force_push admin, branch:bypass_protection owner.
const (
	ActTreeFork         Action = "tree:fork"
	ActBranchForcePush  Action = "branch:force_push"
	ActProtectionBypass Action = "branch:bypass_protection"
)

// v0.8 events.
const (
	EventTreeForked      = "tree.forked"
	EventBranchForced    = "branch.force_pushed"
	EventTenantRenamed   = "tenant.renamed"
	EventTenantDeleted   = "tenant.deleted"
	EventConfigApplied   = "config.applied" // .wt-tree config-as-code applied on push
	EventSecretDetected  = "secret.detected"
	EventConflictsSolved = "mr.conflicts_resolved"
)
