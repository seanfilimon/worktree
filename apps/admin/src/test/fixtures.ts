import type {
  Account,
  Branch,
  BranchProtection,
  MeResponse,
  MergeRequest,
  ServerStats,
  Snapshot,
  SnapshotDetail,
  SearchResults,
  StagedSnapshot,
  Tenant,
  TreeSummary,
} from "@/api/types";

const T0 = "2026-10-20T12:00:00Z";
const recent = (minutesAgo: number) => new Date(Date.now() - minutesAgo * 60_000).toISOString();

export const account: Account = {
  id: "acc-1",
  username: "alice",
  email: "alice@example.com",
  display_name: "Alice Admin",
  tenant_id: "ten-alice",
  is_admin: true,
  disabled: false,
  created_at: T0,
};

export const bob: Account = { ...account, id: "acc-2", username: "bob", email: "bob@example.com", display_name: "Bob", tenant_id: "ten-bob", is_admin: false };

export const tenant: Tenant = { id: "ten-acme", slug: "acme", name: "Acme Corp", kind: "org", plan: "pro", created_at: T0 };

export const me: MeResponse = { account, memberships: [{ tenant, role: "owner" }] };

/** `me` for a regular member (not a server admin). */
export const memberMe: MeResponse = { account: { ...account, is_admin: false }, memberships: [{ tenant, role: "developer" }] };

export const stats: ServerStats = {
  tenants: 3,
  accounts: 12,
  trees: 7,
  branches: 21,
  snapshots: 1234,
  staged: 4,
  objects: 98765,
  storage_bytes: 5 * 1024 * 1024 * 1024,
};

export const summary: TreeSummary = {
  tree: {
    id: "tree-web",
    tenant_id: tenant.id,
    name: "web",
    description: "Marketing site",
    default_branch: "main",
    visibility: "private",
    created_at: T0,
    updated_at: T0,
  },
  branch_count: 2,
  snapshot_count: 3,
  staged_count: 1,
  open_mr_count: 1,
};

export const branches: Branch[] = [
  { tree_id: "tree-web", name: "main", tip: "snap-3", protected: true, created_at: T0, updated_at: recent(5) },
  { tree_id: "tree-web", name: "feature/login", tip: "snap-4", protected: false, created_at: T0, updated_at: recent(60) },
];

const snap = (id: string, message: string, auto: boolean, minutesAgo: number, parents: string[] = []): Snapshot => ({
  id,
  tree_id: "tree-web",
  branch_name: "main",
  parents,
  manifest_hash: "ab".repeat(32),
  author: "alice",
  message,
  created_at: recent(minutesAgo),
  auto_generated: auto,
  pushed_by: "acc-1",
  pushed_at: recent(minutesAgo),
});

export const snapshots: Snapshot[] = [
  snap("snap-3", "Add login page", false, 3, ["snap-2"]),
  snap("snap-2", "auto: 4 files changed", true, 120, ["snap-1"]),
  snap("snap-1", "Initial import", false, 60 * 24 * 3),
];

export const snapshotDetail: SnapshotDetail = {
  snapshot: snapshots[0]!,
  files: [
    { path: "README.md", hash: "11".repeat(32), size: 24, executable: false },
    { path: "src/app.ts", hash: "22".repeat(32), size: 40, executable: false },
    { path: "src/lib/util.ts", hash: "33".repeat(32), size: 10, executable: false },
    { path: "assets/logo.png", hash: "44".repeat(32), size: 6, executable: false },
    { path: "scripts/build.sh", hash: "55".repeat(32), size: 12, executable: true },
  ],
};

export const staged: StagedSnapshot[] = [
  {
    snapshot_id: "staged-1",
    tree_id: "tree-web",
    tenant_id: tenant.id,
    account_id: "acc-2",
    author: "bob",
    branch_name: "feature/login",
    files_changed: ["src/login.tsx", "src/auth.ts"],
    files_added: 1,
    files_modified: 1,
    files_deleted: 0,
    message: "WIP login form",
    staged_at: recent(2),
  },
];

export const protection: BranchProtection = {
  tree_id: "tree-web",
  pattern: "main",
  no_direct_push: true,
  required_approvals: 2,
  allowed_pushers: [],
  no_delete: true,
};

export const mergeRequest: MergeRequest = {
  id: "mr-1",
  tree_id: "tree-web",
  number: 7,
  title: "Add login page",
  description: "Implements the sign-in flow.",
  source_branch: "feature/login",
  target_branch: "main",
  author_id: "acc-2",
  author: "bob",
  state: "open",
  approvals: [],
  created_at: recent(30),
  updated_at: recent(10),
};

/** GET /api/v1/search result covering every result kind. */
export const searchResults: SearchResults = {
  tenants: [tenant],
  trees: [{ tree: summary.tree, tenant_slug: tenant.slug }],
  branches: [{ tenant_slug: tenant.slug, tree: summary.tree.name, branch: branches[1]! }],
  accounts: [bob],
};
