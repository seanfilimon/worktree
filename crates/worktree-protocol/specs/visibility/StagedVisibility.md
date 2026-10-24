# Staged Snapshot Visibility Specification

## Overview

Staged snapshot visibility is W0rkTree's key innovation for team collaboration. In Git, all work is invisible until someone pushes. In W0rkTree, the team can see who's working on what in real-time — without those changes polluting branch history.

**Key principle: Staged ≠ Pushed. Visible ≠ Merged.**

Staged snapshots are a collaboration primitive: they let teams coordinate without committing to branch history. They are ephemeral, low-stakes, and informational. They exist to answer the question: "What is everyone working on right now?"

---

## The Git Problem

Git's collaboration model has a fundamental visibility gap:

- **All work is invisible until push.** A developer can work for days on a feature branch, and no one knows until they push and open a PR.
- **No one knows what anyone else is working on.** Two developers can unknowingly work on the same file, the same function, the same bug — and only discover the collision at merge time.
- **Teams resort to external tools.** Standups, Slack messages, ticket system updates, "hey are you working on X?" — all because Git provides zero in-flight visibility.
- **Surprise merge conflicts.** The worst kind of conflict is the one you didn't know was coming. Git makes ALL merge conflicts surprises.
- **Stale branches accumulate.** Without visibility into activity, branches rot. No one knows if a branch is actively being worked on or abandoned.

W0rkTree solves this by making in-progress work visible at the snapshot level — not the keystroke level, not the push level.

---

## How It Works

The staged visibility pipeline:

1. **Developer edits files locally.** Normal workflow — edit, save, test, iterate.
2. **BGProcess captures snapshots.** Either automatically (on interval/file-change) or manually (`wt snapshot`). See BGProcess spec.
3. **BGProcess syncs snapshots to server as "staged snapshots."** Attributed to user, branch, and tree. This happens automatically if auto-sync is enabled.
4. **Server stores staged snapshots.** Visible to authorized team members but NOT part of branch history. Staged snapshots are indexed by user, branch, tree, and timestamp.
5. **Other developers see activity.** "Alice has staged snapshots touching `auth-service/src/oauth.rs` on `feature/oauth`" — visible via CLI, admin panel, SDK, WebSocket.
6. **When ready, developer pushes.** `wt push` moves staged snapshots into branch history. Only THEN do they become permanent.

The critical distinction:

```
Staged snapshot:  Visible to team, NOT in branch history, ephemeral
Pushed snapshot:  In branch history, permanent, part of the DAG
```

---

## What's Visible

When a developer has staged snapshots, authorized team members can see:

- **Which files have been modified** — full file paths (e.g., `auth-service/src/oauth.rs`, `shared/types/user.rs`)
- **Who made the changes** — user identity (email / display name)
- **Which branch they're working on** — branch name (e.g., `feature/oauth`)
- **Which tree the changes are in** — tree path (e.g., `auth-service`)
- **When the staged snapshot was created** — timestamp (UTC)
- **How many staged snapshots are pending** — count of unpushed snapshots per user/branch
- **Optional message** — if the developer used `wt snapshot --message "WIP: fixing token refresh"`

This is enough information to:
- Know who's working on what
- Detect potential conflicts early ("Alice and Bob are both touching `pricing.rs`")
- Gauge progress on features
- Identify active vs. stale branches

---

## What's NOT Visible

Staged visibility has deliberate limits:

- **File contents for proprietary-licensed paths.** License compliance applies to staged snapshots. If a file is under a proprietary license, viewers can see the path but NOT the content (unless they have a license grant). See License Compliance spec.
- **This is NOT real-time keystroke/line-level presence** (like Google Docs). W0rkTree does not stream individual keystrokes or cursor positions. Granularity is snapshot-level.
- **This is NOT broadcasting raw file edits.** The content of staged snapshots is available to authorized users, but it's pulled on demand — not pushed to everyone's screen.
- **Granularity is snapshot-level, not character-level.** A snapshot captures a point-in-time state. Between snapshots, work is invisible (just like between Git commits).
- **Diffs are not pre-computed for staged visibility.** Viewers see file paths changed, not line-by-line diffs. Diffs can be computed on demand.

---

## Visibility Surfaces

Staged snapshot visibility is exposed through multiple surfaces:

| Surface | Command / Endpoint | What it shows |
|---|---|---|
| CLI | `wt status --team` | Staged snapshots from all team members on current tree |
| CLI | `wt staged` | All staged (unpushed) snapshots, per user and branch |
| CLI | `wt staged --user alice` | Staged snapshots from a specific user |
| CLI | `wt staged --branch feature/oauth` | Staged snapshots on a specific branch |
| Admin panel | Dashboard | Real-time staged activity per tenant/tree |
| Admin panel | User activity view | Per-user staged snapshot history |
| SDK | Event subscription | Subscribe to staged snapshot events for tooling |
| SDK | Query API | Programmatic access to staged snapshot metadata |
| WebSocket | `/api/repositories/:id/staged/live` | Real-time stream of staged snapshot updates |
| REST API | `GET /api/repositories/:id/staged` | List current staged snapshots |

### CLI Examples

```
$ wt status --team

  Tree: auth-service
  Branch: feature/oauth

  Staged Activity:
    Alice (alice@company.io) — 3 staged snapshots (latest: 2 min ago)
      modified: src/oauth.rs, src/token.rs
      branch: feature/oauth

    Bob (bob@company.io) — 1 staged snapshot (latest: 15 min ago)
      modified: src/middleware/auth.rs
      branch: fix/auth-middleware-panic

    You — 2 staged snapshots (latest: just now)
      modified: src/handlers/login.rs
      branch: feature/oauth
```

```
$ wt staged

  Unpushed staged snapshots:

  feature/oauth (yours):
    [snap-a1b2c3] 2 min ago — src/handlers/login.rs
    [snap-d4e5f6] 5 min ago — src/handlers/login.rs, src/types.rs

  feature/oauth (alice@company.io):
    [snap-g7h8i9] 2 min ago — src/oauth.rs, src/token.rs
    [snap-j0k1l2] 8 min ago — src/oauth.rs
    [snap-m3n4o5] 12 min ago — src/oauth.rs, tests/oauth_test.rs
```

### WebSocket Event Format

```json
{
  "event": "staged_snapshot",
  "data": {
    "snapshot_id": "snap-a1b2c3d4",
    "user": "alice@company.io",
    "tree": "auth-service",
    "branch": "feature/oauth",
    "files_changed": ["src/oauth.rs", "src/token.rs"],
    "timestamp": "2025-01-15T14:32:00Z",
    "message": null,
    "snapshot_count": 3
  }
}
```

---

## Staged Snapshot Data Model

```
StagedSnapshot {
    id: SnapshotId,
    user: AccountEmail,
    tree: TreePath,
    branch: BranchName,
    files_changed: Vec<FilePath>,
    timestamp: DateTime<Utc>,
    message: Option<String>,   // If manual snapshot with --message
    status: StagedStatus,      // Always "staged" until pushed
}

enum StagedStatus {
    Staged,      // Synced to server, not yet pushed
    Pushed,      // Has been pushed to branch history
    Cleared,     // Manually cleared by user
    Expired,     // Past retention window, pending GC
}
```

### Server-Side Indexing

Staged snapshots are indexed for fast lookup:

- **By user**: All staged snapshots for a given user across all trees/branches
- **By branch**: All staged snapshots on a given branch across all users
- **By tree**: All staged snapshots in a given tree across all users/branches
- **By timestamp**: For retention policy enforcement and activity timelines
- **By file path**: For conflict detection ("who else is touching this file?")

### Conflict Detection via Staged Visibility

One of the most valuable uses of staged visibility is early conflict detection:

```
$ wt push

  ⚠ Potential conflict detected:
    alice@company.io has staged changes to src/oauth.rs on feature/oauth
    Your push also modifies src/oauth.rs

  This is not a merge conflict (Alice hasn't pushed yet), but you may
  want to coordinate before she pushes.

  Proceed with push? [y/N]
```

This is advisory, not blocking. The actual merge conflict (if any) happens when Alice pushes. But the early warning prevents surprises.

---

## Privacy Controls

Staged visibility is powerful, but developers need control over their visibility:

### Opt-Out Per Tree

In `.wt-tree/config.toml`:
```toml
[sync]
auto = false        # Disables auto-sync, which disables auto-staging
```

This stops the bgprocess from syncing staged snapshots for this tree. The developer's work is invisible until they explicitly push.

### Temporary Pause

```
$ wt sync pause
Staged snapshot sync paused. Your work is invisible to the team.
Resume with: wt sync resume

$ wt sync resume
Staged snapshot sync resumed.
```

`wt sync pause` pauses staged snapshot visibility temporarily. Useful for:
- Experimental/throwaway work you don't want visible
- Sensitive refactoring you want to reveal all at once
- Personal preference during deep focus time

### Branch-Level Privacy

Staged snapshots from private branches are only visible to users with `branch:read` permission for that branch. This means:

- Private feature branches: only the developer (and admins) see staged activity
- Team branches: all team members see staged activity
- Public branches: all authorized users see staged activity

### Public Worktree Behavior

In public worktrees:
- Staged snapshot metadata (file paths, user, branch) is visible to all authenticated users
- File contents in staged snapshots respect license restrictions
- Unauthenticated users cannot see staged snapshots

---

## Staged Snapshot Retention

Staged snapshots are ephemeral by design. Retention policy:

### Pushed Snapshots
- Staged snapshots that are pushed to a branch via `wt push`: marked as `Pushed`
- Metadata retained indefinitely (part of branch history audit trail)
- The snapshot itself becomes a branch snapshot — the staged copy is just a status marker

### Unpushed Snapshots
- Staged snapshots never pushed: server retains per configurable policy
- **Default retention: 30 days** from creation timestamp
- After retention window: marked as `Expired`, then garbage collected
- Configurable in server config:

```toml
[staged]
retention_days = 30        # Default: 30
gc_interval_hours = 24     # How often GC runs
```

### Manual Clearing

```
$ wt staged clear
Cleared 5 staged snapshots.

$ wt staged clear --branch feature/old-experiment
Cleared 12 staged snapshots on feature/old-experiment.

$ wt staged clear --all
Cleared all 23 staged snapshots across all branches.
```

Cleared snapshots are marked as `Cleared` and garbage collected on next GC cycle.

---

## License Compliance Interaction

Staged visibility respects license compliance at all times. This interaction is critical:

### Metadata Is Always Visible
Staged snapshot metadata (file paths, user, branch, timestamp) is always visible to authorized users, regardless of license. You can always see:
- "Alice is working on `billing-engine/src/pricing.rs`"
- "Bob modified `vendor/sdk/internal.rs` on `feature/sdk-update`"

### Content Respects Licenses
Actual file CONTENTS in staged snapshots respect license restrictions:

| File License | Viewer Has Grant? | Can See Path? | Can Read Content? |
|---|---|---|---|
| MIT / Apache-2.0 | N/A | ✓ | ✓ |
| Proprietary | No | ✓ | ✗ |
| Proprietary | Yes (read-only) | ✓ | ✓ |
| Proprietary | Yes (modify) | ✓ | ✓ |
| Proprietary | Yes (redistribute) | ✓ | ✓ |

### Example Scenario

Alice works at TenantA. Bob works at PartnerCorp. The path `billing-engine/` is proprietary with a `read-only` grant to PartnerCorp.

- Bob runs `wt staged` and sees: "Alice has staged changes to `billing-engine/src/pricing.rs`"
- Bob can read the staged file content (he has a read-only grant)
- Bob CANNOT export or copy Alice's staged content (read-only, not redistribute)
- Carol at UnrelatedCorp runs `wt staged` and sees: "Alice has staged changes to `billing-engine/src/pricing.rs`"
- Carol CANNOT read the staged file content (no grant)

See [License Compliance Specification](../licensing/LicenseCompliance.md) for full details.

---

## Relationship to Other Specs

- **BGProcess**: The bgprocess is responsible for creating snapshots and syncing them to the server. Staged visibility is a consequence of the sync pipeline. See [BGProcess spec](../bgprocess/).
- **Sync Protocol**: Staged snapshot upload is the first phase of the sync protocol. See [Sync spec](../sync/Sync.md).
- **IAM**: Branch-level and tree-level permissions govern who can see staged snapshots. See [IAM spec](../iam/).
- **License Compliance**: License restrictions apply to staged snapshot content access. See [License Compliance spec](../licensing/LicenseCompliance.md).
- **Snapshots**: Staged snapshots use the same snapshot data model as branch snapshots. The difference is status (staged vs. committed).

---

## Implementation Status

- **IMPLEMENTED**: None (new concept)
- **TODO**: Staged snapshot data model, `StagedStatus` enum, server storage/indexing, API endpoints, CLI commands (`wt staged`, `wt status --team`)
- **PLANNED**: WebSocket streaming for real-time updates, admin panel dashboard, SDK event subscriptions, conflict detection advisory system
- **DEFERRED**: Line-level granularity (intentionally deferred — snapshot-level is the right abstraction for now)