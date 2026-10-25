import type { Account, MeResponse, Member, Tenant, TreeSummary } from "@/api/types";
import * as fx from "./fixtures";

const T0 = "2026-10-20T12:00:00Z";

export const carol: Account = {
  ...fx.bob,
  id: "acc-3",
  username: "carol",
  email: "carol@example.com",
  display_name: "Carol Viewer",
  tenant_id: "ten-carol",
};

export const globex: Tenant = { id: "ten-globex", slug: "globex", name: "Globex", kind: "org", plan: "enterprise", created_at: T0 };
export const aliceHome: Tenant = { id: "ten-alice", slug: "alice", name: "alice", kind: "user", plan: "free", created_at: T0 };

export const acmeMembers: Member[] = [
  { account: fx.account, role: "owner" },
  { account: fx.bob, role: "developer" },
  { account: carol, role: "viewer" },
];

/** Alice owns acme, is a viewer in globex and has a personal tenant. */
export const meMulti: MeResponse = {
  account: fx.account,
  memberships: [
    { tenant: fx.tenant, role: "owner" },
    { tenant: globex, role: "viewer" },
    { tenant: aliceHome, role: "owner" },
  ],
};

/** Same, but without server-admin rights. */
export const meMember: MeResponse = { ...meMulti, account: { ...fx.account, is_admin: false } };

const summary = (tenant: Tenant, name: string, over: Partial<TreeSummary["tree"]> & { snapshots?: number } = {}): TreeSummary => {
  const { snapshots = 0, ...tree } = over;
  return {
    tree: {
      id: `tree-${tenant.slug}-${name}`,
      tenant_id: tenant.id,
      name,
      description: "",
      default_branch: "main",
      visibility: "private",
      created_at: T0,
      updated_at: T0,
      ...tree,
    },
    branch_count: 1,
    snapshot_count: snapshots,
    staged_count: 0,
    open_mr_count: 0,
  };
};

export const acmeTrees: TreeSummary[] = [
  fx.summary,
  summary(fx.tenant, "api", { visibility: "internal", snapshots: 40, updated_at: "2026-10-22T09:00:00Z" }),
];

export const globexTrees: TreeSummary[] = [
  summary(globex, "docs", { visibility: "public", snapshots: 12, updated_at: "2026-10-21T09:00:00Z", description: "Public handbook" }),
];
