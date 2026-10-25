/** Merge request fixtures (acme/web; main tip snap-3, feature/login tip snap-4). */
import type { Approval, CompareResult, FileDiff, MergeRequest, Snapshot } from "@/api/types";
import { account, mergeRequest, snapshots } from "./fixtures";

const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

export const approval = (username: string, accountId: string, minutesAgo = 5): Approval => ({
  account_id: accountId,
  username,
  at: ago(minutesAgo),
});

/** #7 by bob, approved by both required reviewers (main requires 2). */
export const approvedMr: MergeRequest = {
  ...mergeRequest,
  approvals: [approval("carol", "acc-3", 8), approval("dave", "acc-4", 6)],
};

/** #7 opened by the signed-in user (alice). */
export const ownMr: MergeRequest = { ...mergeRequest, author_id: account.id, author: account.username };

export const mergedMr: MergeRequest = {
  ...approvedMr,
  state: "merged",
  merged_at: ago(2),
  merged_snapshot: "snap-4",
  updated_at: ago(2),
};

export const closedMr: MergeRequest = { ...mergeRequest, state: "closed", updated_at: ago(1) };

/** A mix of states, authors and targets for list filtering. */
export const mergeRequestList: MergeRequest[] = [
  { ...mergeRequest, approvals: [approval("carol", "acc-3")] },
  {
    ...mergeRequest,
    id: "mr-8",
    number: 8,
    title: "Update README",
    source_branch: "docs/readme",
    author_id: account.id,
    author: "alice",
    updated_at: ago(3),
  },
  {
    ...mergedMr,
    id: "mr-5",
    number: 5,
    title: "Fix typo in footer",
    source_branch: "fix/typo",
    author_id: "acc-3",
    author: "carol",
    updated_at: ago(60),
  },
  {
    ...mergeRequest,
    id: "mr-3",
    number: 3,
    title: "Spike: new renderer",
    source_branch: "spike/renderer",
    target_branch: "release/1.0",
    author_id: "acc-4",
    author: "dave",
    state: "closed",
    updated_at: ago(600),
  },
];

export const featureSnapshot: Snapshot = {
  ...snapshots[0]!,
  id: "snap-4",
  branch_name: "feature/login",
  parents: ["snap-3"],
  author: "bob",
  message: "Add login form\n\nWith validation.",
  pushed_by: "acc-2",
};

export const fileDiffs: FileDiff[] = [
  {
    path: "src/app.ts",
    status: "modified",
    old_hash: "22".repeat(32),
    new_hash: "66".repeat(32),
    old_size: 40,
    new_size: 52,
    binary: false,
    too_large: false,
    additions: 2,
    deletions: 1,
    patch: "--- a/src/app.ts\n+++ b/src/app.ts\n@@ -1,2 +1,3 @@\n-old\n+new\n+login()\n",
  },
  {
    path: "src/login.tsx",
    status: "added",
    new_hash: "77".repeat(32),
    old_size: 0,
    new_size: 200,
    binary: false,
    too_large: false,
    additions: 10,
    deletions: 0,
    patch: "--- /dev/null\n+++ b/src/login.tsx\n@@ -0,0 +1,10 @@\n+export function Login() {}\n",
  },
];

/** compare?from=snap-3&to=snap-4: main is an ancestor of feature/login. */
export const compareReady: CompareResult = {
  from: snapshots[0]!,
  to: featureSnapshot,
  merge_base: "snap-3",
  snapshots: [featureSnapshot],
  files: fileDiffs,
  stats: { files: 2, additions: 12, deletions: 1 },
};

/** Both branches moved since snap-2. */
export const compareDiverged: CompareResult = { ...compareReady, merge_base: "snap-2" };

/** The target already contains the source. */
export const compareUpToDate: CompareResult = {
  ...compareReady,
  snapshots: [],
  files: [],
  stats: { files: 0, additions: 0, deletions: 0 },
};

/** compare?to=snap-4 (merged snapshot against its parent). */
export const compareMerged: CompareResult = { ...compareReady, merge_base: undefined };
