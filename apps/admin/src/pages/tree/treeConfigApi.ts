/**
 * Config-as-code endpoints (docs/ENGINEERING_PLAN.md §16.3) used by
 * TreeConfigTab and BranchesTab.
 *
 * Field names mirror services/server-go/internal/treeconfig (State,
 * Applied, Attempt, Diagnostic, ValidationResult). Go encodes empty slices
 * as `null`; the helpers below normalise lists to `[]`.
 */
import type { ApiClient } from "@/api/client";
import type { Branch, BranchProtection, ISODateString } from "@/api/types";

/** A protection with the v0.5+ rule fields (domain.BranchProtection). */
export interface DeclaredProtection extends BranchProtection {
  require_ci_pass?: boolean;
  required_checks?: string[] | null;
  require_signed?: boolean;
  require_linear?: boolean;
  block_on_todos?: boolean;
  merge_methods?: string[] | null;
  no_force_push?: boolean;
  require_owner_approval?: boolean;
}

// ---- wire types ----

/** A problem in a .wt-tree file (codes E30xx/W31xx, access files E20xx/W21xx). */
export interface ConfigDiagnostic {
  code: string;
  severity: "error" | "warning";
  /** Tree-relative path, e.g. ".wt-tree/branches.toml". */
  file: string;
  /** 1-based; 0 = no position. */
  line: number;
  col: number;
  message: string;
}

export interface ConfigFileInfo {
  path: string;
  hash: string;
  size: number;
}

export interface BranchDecl {
  name: string;
  description?: string;
  owner?: string;
}

export interface TreeConfigApplied {
  snapshot_id: string;
  branch: string;
  applied_by: string;
  applied_at: ISODateString;
  files: ConfigFileInfo[];
  branches: BranchDecl[];
  protections: DeclaredProtection[];
  managed_patterns: string[];
  access_hash?: string;
  changes: string[];
  warnings: ConfigDiagnostic[];
}

export interface TreeConfigAttempt {
  snapshot_id: string;
  branch: string;
  by: string;
  at: ISODateString;
  accepted: boolean;
  errors: ConfigDiagnostic[];
  warnings: ConfigDiagnostic[];
}

export interface TreeConfigState {
  tree_id: string;
  default_branch: string;
  applied: TreeConfigApplied | null;
  last_attempt: TreeConfigAttempt | null;
}

export interface ConfigValidation {
  valid: boolean;
  diagnostics: ConfigDiagnostic[];
  branches: BranchDecl[];
  protections: DeclaredProtection[];
  access_changes: string[];
}

/** Absent fields are unchanged; "" clears. */
export interface PatchBranchRequest {
  description?: string;
  owner?: string;
}

/** The files the server validates and applies. */
export const CONFIG_FILES = [
  { path: ".wt-tree/branches.toml", label: "branches.toml" },
  { path: ".wt-tree/protections.toml", label: "protections.toml" },
  { path: ".wt-tree/config.toml", label: "config.toml" },
  { path: ".wt-tree/access/policies.toml", label: "access/policies.toml" },
] as const;

// ---- endpoints ----

const seg = encodeURIComponent;

function arr<T>(v: T[] | null | undefined): T[] {
  return Array.isArray(v) ? v : [];
}

function normApplied(a: TreeConfigApplied | null | undefined): TreeConfigApplied | null {
  if (!a) return null;
  return {
    ...a,
    files: arr(a.files),
    branches: arr(a.branches),
    protections: arr(a.protections),
    managed_patterns: arr(a.managed_patterns),
    changes: arr(a.changes),
    warnings: arr(a.warnings),
  };
}

function normAttempt(a: TreeConfigAttempt | null | undefined): TreeConfigAttempt | null {
  if (!a) return null;
  return { ...a, errors: arr(a.errors), warnings: arr(a.warnings) };
}

export function normState(s: Partial<TreeConfigState> | null | undefined): TreeConfigState {
  return {
    tree_id: s?.tree_id ?? "",
    default_branch: s?.default_branch ?? "",
    applied: normApplied(s?.applied),
    last_attempt: normAttempt(s?.last_attempt),
  };
}

export function createTreeConfigApi(client: ApiClient, tenant: string, tree: string) {
  const base = `/api/v1/trees/${seg(tenant)}/${seg(tree)}`;
  return {
    get: async (): Promise<TreeConfigState> => normState(await client.request<TreeConfigState>(`${base}/config`)),
    validate: async (files: Record<string, string>): Promise<ConfigValidation> => {
      const v = await client.request<ConfigValidation>(`${base}/config/validate`, { method: "POST", body: { files } });
      return {
        valid: Boolean(v.valid),
        diagnostics: arr(v.diagnostics),
        branches: arr(v.branches),
        protections: arr(v.protections),
        access_changes: arr(v.access_changes),
      };
    },
    patchBranch: (branch: string, body: PatchBranchRequest) =>
      client.request<Branch>(`${base}/branches/${branch.split("/").map(seg).join("/")}`, { method: "PATCH", body }),
  };
}

export type TreeConfigApi = ReturnType<typeof createTreeConfigApi>;

export const treeConfigKey = (tenant: string, tree: string) => ["tree", tenant, tree, "config"] as const;

// ---- helpers ----

/** "file:line:col" (just "file" without a position). */
export function diagnosticLocation(d: ConfigDiagnostic): string {
  return d.line > 0 ? `${d.file}:${d.line}:${d.col}` : d.file;
}

const OWNER_RE = /^(user:[a-z0-9][a-z0-9-]{0,38}|team:[a-z0-9][a-z0-9-]{0,62})$/;

/** Validation message for a branch owner ("" = valid; an empty owner clears). */
export function ownerError(owner: string): string {
  const o = owner.trim();
  if (!o) return "";
  return OWNER_RE.test(o) ? "" : "Use user:NAME or team:SLUG (lowercase letters, digits and -).";
}

/** Human summary of a protection's rules. */
export function protectionSummary(p: DeclaredProtection): string {
  const parts: string[] = [];
  if (p.no_direct_push) parts.push("no direct push");
  if (p.required_approvals > 0) parts.push(`${p.required_approvals} approval${p.required_approvals === 1 ? "" : "s"}`);
  if (p.no_delete) parts.push("no delete");
  if (p.require_ci_pass) parts.push("CI must pass");
  if (p.require_signed) parts.push("signed");
  if (p.require_linear) parts.push("linear");
  if (p.no_force_push) parts.push("no force push");
  if (p.require_owner_approval) parts.push("owner approval");
  if (p.merge_methods && p.merge_methods.length > 0) parts.push(`merge: ${p.merge_methods.join(", ")}`);
  if (p.allowed_pushers && p.allowed_pushers.length > 0) parts.push(`pushers: ${p.allowed_pushers.join(", ")}`);
  return parts.length > 0 ? parts.join(" · ") : "no restrictions";
}
