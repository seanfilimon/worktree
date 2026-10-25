import type { CompareResult, FileDiff, ServerEvent, Tag, TreeStats } from "@/api/types";
import { snapshots, tenant } from "./fixtures";

const recent = (minutesAgo: number) => new Date(Date.now() - minutesAgo * 60_000).toISOString();

export const modifiedPatch = [
  "--- a/src/app.ts",
  "+++ b/src/app.ts",
  "@@ -1,4 +1,5 @@ export function main()",
  " const a = 1;",
  "-const b = 2;",
  "+const b = 3;",
  "+const c = 4;",
  " export default a;",
  " // end",
  "@@ -10,2 +11,2 @@",
  "-old tail",
  "+new tail",
  " last",
  "\\ No newline at end of file",
  "",
].join("\n");

export const addedPatch = ["--- /dev/null", "+++ b/src/login.tsx", "@@ -0,0 +1,2 @@", "+export function Login() {}", "+export default Login;", ""].join("\n");

export const deletedPatch = ["--- a/old.txt", "+++ /dev/null", "@@ -1 +0,0 @@", "-goodbye", ""].join("\n");

export const fileDiffs: FileDiff[] = [
  { path: "assets/logo.png", status: "modified", old_size: 1024, new_size: 2048, binary: true, too_large: false, additions: 0, deletions: 0 },
  { path: "data/big.json", status: "modified", old_size: 3 << 20, new_size: 3 << 20, binary: false, too_large: true, additions: 0, deletions: 0 },
  { path: "old.txt", status: "deleted", old_size: 8, new_size: 0, binary: false, too_large: false, additions: 0, deletions: 1, patch: deletedPatch },
  { path: "src/app.ts", status: "modified", old_size: 60, new_size: 72, binary: false, too_large: false, additions: 3, deletions: 2, patch: modifiedPatch },
  { path: "src/login.tsx", status: "added", old_size: 0, new_size: 48, binary: false, too_large: false, additions: 2, deletions: 0, patch: addedPatch },
];

export const compareResult: CompareResult = {
  from: snapshots[1]!,
  to: snapshots[0]!,
  snapshots: [snapshots[0]!],
  files: fileDiffs,
  stats: { files: fileDiffs.length, additions: 5, deletions: 3 },
};

export const treeStats: TreeStats = {
  branch: "main",
  snapshot_count: 42,
  contributors: [
    { author: "bob", snapshots: 12, last_at: recent(90) },
    { author: "alice", snapshots: 30, last_at: recent(3) },
  ],
  activity: Array.from({ length: 30 }, (_, i) => ({
    date: new Date(Date.UTC(2026, 8, 25 + i)).toISOString().slice(0, 10),
    snapshots: i % 4,
  })),
  file_count: 128,
  total_bytes: 3 * 1024 * 1024,
  languages: [
    { language: "TypeScript", files: 80, bytes: 3000 },
    { language: "CSS", files: 10, bytes: 750 },
    { language: "Other", files: 38, bytes: 250 },
  ],
};

export const recentEvents: ServerEvent[] = [
  { type: "branch.created", tenant_id: tenant.id, tree_id: "tree-web", actor: "bob", payload: { branch: "feature/login" }, at: recent(30) },
  { type: "branch.pushed", tenant_id: tenant.id, tree_id: "tree-web", actor: "alice", payload: { branch: "main" }, at: recent(3) },
];

export const tags: Tag[] = [
  { tree_id: "tree-web", name: "v1.0.0", target: "snap-3", message: "First release", tagger: "alice", created_at: recent(10) },
];

export const readme = "# Web\n\nThe **marketing** site. See [docs](https://example.com/docs).\n\n- fast\n- small\n\n```\nnpm run dev\n```\n";
