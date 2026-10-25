/** v0.5 tree fixtures (acme/web; main tip snap-3, feature/login tip snap-4). */
import type {
  CIStatus,
  LinkedGroup,
  MergeChecks,
  MergeRequestV05,
  Release,
  ReviewApproval,
  ReviewComment,
  SnapshotDetailV05,
  TagProtection,
  Todo,
} from "@/api/v05tree";
import { mergeRequest, snapshotDetail, snapshots } from "./fixtures";

const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

export const todo = (number: number, extra: Partial<Todo> = {}): Todo => ({
  id: `todo-${number}`,
  tenant_id: "ten-acme",
  tree_id: "tree-web",
  number,
  target_branch: "main",
  title: `Task ${number}`,
  priority: "medium",
  blocking: false,
  linked: false,
  status: "open",
  source_tenant_id: "ten-acme",
  source_tree_id: "tree-api",
  source_branch: "main",
  source_snapshot_id: "api-snap-1",
  created_by: "acc-2",
  author: "bob",
  created_at: ago(30),
  updated_at: ago(30),
  ...extra,
});

export const todos: Todo[] = [
  todo(1, { title: "Update API client", details: "The v2 endpoints changed.", priority: "high", blocking: true }),
  todo(2, { title: "Bump schema", status: "in_progress", assignee_id: "acc-1", assignee: "alice" }),
];

export const release: Release = {
  id: "rel-1",
  tree_id: "tree-web",
  tag: "v1.0.0",
  snapshot_id: "snap-3",
  name: "First stable",
  notes: "## Highlights\n\n- Login page",
  prerelease: true,
  assets: [
    { name: "web.tar.gz", size: 2048, hash: "aa".repeat(32), content_type: "application/gzip", uploaded_at: ago(5) },
  ],
  created_by: "acc-1",
  author: "alice",
  created_at: ago(10),
};

export const review = (username: string, accountId: string, extra: Partial<ReviewApproval> = {}): ReviewApproval => ({
  account_id: accountId,
  username,
  at: ago(5),
  ...extra,
});

/** #7: carol approved an older tip (stale), dave approved the current one. */
export const staleMr: MergeRequestV05 = {
  ...mergeRequest,
  approvals: [review("carol", "acc-3", { snapshot_id: "snap-old", decision: "approved" }), review("dave", "acc-4", { snapshot_id: "snap-4" })],
};

/** #7 with two current approvals but erin requested changes. */
export const changesRequestedMr: MergeRequestV05 = {
  ...mergeRequest,
  approvals: [
    review("carol", "acc-3", { snapshot_id: "snap-4" }),
    review("dave", "acc-4", { snapshot_id: "snap-4" }),
    review("erin", "acc-5", { snapshot_id: "snap-4", decision: "changes_requested", body: "Please add tests." }),
  ],
};

export const comments: ReviewComment[] = [
  {
    id: "c1",
    tree_id: "tree-web",
    mr_number: 7,
    author_id: "acc-3",
    author: "carol",
    body: "Looks good overall.",
    created_at: ago(20),
    updated_at: ago(20),
  },
  {
    id: "c2",
    tree_id: "tree-web",
    mr_number: 7,
    author_id: "acc-1",
    author: "alice",
    body: "Rename this variable.",
    path: "src/login.tsx",
    line: 12,
    snapshot_id: "snap-old",
    created_at: ago(15),
    updated_at: ago(15),
  },
];

export const ciChecks: CIStatus[] = [
  { tree_id: "tree-web", snapshot_id: "snap-4", context: "ci/build", state: "success", reporter: "ci-bot", updated_at: ago(3) },
  {
    tree_id: "tree-web",
    snapshot_id: "snap-4",
    context: "ci/test",
    state: "failure",
    description: "2 tests failed",
    target_url: "https://ci.example.com/run/9",
    reporter: "ci-bot",
    updated_at: ago(2),
  },
];

export const blockedChecks: MergeChecks = {
  mergeable: false,
  blockers: [{ kind: "ci_failed", message: "Check ci/test is failure." }],
  approvals: { required: 2, current: 2, stale: 0 },
  ci: { state: "failure", required: ["ci/build", "ci/test", "ci/lint"], missing: ["ci/lint"] },
  todos: { blocking_open: 1 },
};

export const linkedGroup: LinkedGroup = {
  id: "grp-1",
  tenant_id: "ten-acme",
  name: "login rollout",
  members: [
    { tree_id: "tree-web", mr_number: 7 },
    { tree_id: "tree-api", mr_number: 3 },
  ],
  state: "open",
  created_by: "acc-1",
  created_at: ago(40),
};

export const tagProtection: TagProtection = {
  tree_id: "tree-web",
  pattern: "v*",
  no_delete: true,
  no_move: true,
  allowed_taggers: ["release-bot"],
};

/** snap-3 signed and verified, reverting snap-1 and declaring a dependency on acme/api. */
export const signedDetail: SnapshotDetailV05 = {
  ...snapshotDetail,
  snapshot: {
    ...snapshots[0]!,
    verified: true,
    signature: "c2ln",
    signing_key_id: "key-7",
    reverts: "snap-1",
    dependencies: [{ tree: "api", title: "Update API client", priority: "high", blocking: true }],
  },
  restricted_count: 2,
};
