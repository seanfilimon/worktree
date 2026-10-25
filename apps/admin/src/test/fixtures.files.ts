import type { FileHistoryEntry, LicenseRule, SnapshotDetail, StagedBranchActivity, StagedSnapshot } from "@/api/types";
import { snapshots, tenant } from "./fixtures";

const recent = (minutesAgo: number) => new Date(Date.now() - minutesAgo * 60_000).toISOString();

export const appSource = "// entry point\nconst greeting = 'hello';\nexport default greeting;\n";

export const readmeSource = [
  "# Hello",
  "",
  "Some **bold** text and a [guide](docs/guide.md).",
  "",
  "[click me](javascript:alert(1)) <script>alert(1)</script>",
  "",
  "- [x] done",
  "- [ ] todo",
].join("\n");

/** Manifest of snapshot snap-3 (tip of main). */
export const filesDetail: SnapshotDetail = {
  snapshot: snapshots[0]!,
  files: [
    { path: "README.md", hash: "11".repeat(32), size: readmeSource.length, executable: false },
    { path: "src/app.ts", hash: "22".repeat(32), size: appSource.length, executable: false },
    { path: "src/lib/util.ts", hash: "33".repeat(32), size: 10, executable: false },
    { path: "docs/guide.md", hash: "44".repeat(32), size: 8, executable: false },
    { path: "assets/logo.png", hash: "55".repeat(32), size: 8, executable: false },
    { path: "scripts/build.sh", hash: "66".repeat(32), size: 12, executable: true },
  ],
};

export const appHistory: FileHistoryEntry[] = [
  { snapshot: snapshots[0]!, change: "modified" },
  { snapshot: snapshots[2]!, change: "added" },
];

export const stagedWithOverlap: StagedSnapshot[] = [
  {
    snapshot_id: "staged-bob",
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
  {
    snapshot_id: "staged-carol",
    tree_id: "tree-web",
    tenant_id: tenant.id,
    account_id: "acc-3",
    author: "carol",
    branch_name: "feature/login",
    files_changed: ["src/auth.ts"],
    files_added: 0,
    files_modified: 1,
    files_deleted: 0,
    message: "Token refresh",
    staged_at: recent(8),
  },
];

export const stagedSummary: StagedBranchActivity[] = [
  {
    branch: "feature/login",
    authors: [
      { author: "bob", account_id: "acc-2", files: 2, staged_at: recent(2) },
      { author: "carol", account_id: "acc-3", files: 1, staged_at: recent(8) },
    ],
    files_changed: 2,
    latest_at: recent(2),
    overlaps: [{ path: "src/auth.ts", authors: ["bob", "carol"], branches: ["feature/login"] }],
  },
];

export const licenseRule: LicenseRule = { tree_id: "tree-web", path_glob: "vendor/**", spdx: "MIT", grant: "redistribute" };
