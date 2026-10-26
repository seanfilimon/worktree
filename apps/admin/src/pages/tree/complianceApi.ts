/**
 * Secret scanning and license compliance endpoints (docs/ENGINEERING_PLAN.md
 * §16.7) used by ComplianceTab and CompliancePage.
 *
 * Field names mirror services/server-go/internal/scan (Settings, Finding,
 * AllowList, ScanResult, LicenseAudit, Summary). Go encodes empty slices as
 * `null`; the helpers below normalise lists to `[]`.
 */
import type { ApiClient } from "@/api/client";
import type { ISODateString } from "@/api/types";

// ---- wire types ----

export type ScanMode = "off" | "warn" | "block";
export type FindingStatus = "open" | "resolved";
export type FindingAction = "warned" | "blocked" | "bypassed";
export type Resolution =
  | "revoked"
  | "false_positive"
  | "test_data"
  | "wont_fix";

export interface ComplianceSettings {
  tree_id: string;
  mode: ScanMode;
  exclude: string[];
  /** 0 = the 1 MiB default. */
  max_file_bytes: number;
  updated_by?: string;
  updated_at?: ISODateString;
}

export type ComplianceSettingsRequest = Partial<
  Pick<ComplianceSettings, "mode" | "exclude" | "max_file_bytes">
>;

export interface SecretFinding {
  id: string;
  tree_id: string;
  path: string;
  line: number;
  rule: string;
  /** Redacted. */
  excerpt: string;
  fingerprint: string;
  branch: string;
  snapshot_id: string;
  status: FindingStatus;
  action: FindingAction;
  count: number;
  pushed_by: string;
  first_seen_at: ISODateString;
  last_seen_at: ISODateString;
  resolution?: Resolution | "";
  note?: string;
  resolved_by?: string;
  resolved_at?: ISODateString;
}

export interface SecretFindingList {
  findings: SecretFinding[];
  open: number;
  resolved: number;
}

export interface AllowEntry {
  fingerprint: string;
  reason: string;
  created_by: string;
  created_at: ISODateString;
}

export interface AllowList {
  tree_id: string;
  entries: AllowEntry[];
}

export interface LicenseUsage {
  spdx: string;
  files: number;
  proprietary: boolean;
  globs: string[];
}

export interface ExposedPath {
  path: string;
  spdx: string;
  /** "" = every tenant that does not own the tree. */
  grantee: string;
  grant: string;
  glob: string;
  expires_at?: ISODateString;
}

export interface LicenseRuleCheck {
  path_glob: string;
  spdx: string;
  grant: string;
  grantee: string;
  expires_at?: ISODateString;
  expired: boolean;
  files: number;
  valid: boolean;
  problems: string[];
}

export interface LicenseAudit {
  tree_id: string;
  ref: string;
  snapshot_id: string;
  files: number;
  licenses: LicenseUsage[];
  unlicensed: string[];
  unlicensed_count: number;
  exposed: ExposedPath[];
  exposed_count: number;
  rules: LicenseRuleCheck[];
  invalid_rules: number;
  spdx_list_version: string;
  truncated: boolean;
}

export interface TreeCompliance {
  tree_id: string;
  tenant: string;
  tree: string;
  mode: ScanMode;
  open_findings: number;
  resolved_findings: number;
  blocked_findings: number;
  allowlisted: number;
  license_rules: number;
  invalid_rules: number;
  last_finding_at?: ISODateString;
}

export interface RecentSecretFinding extends SecretFinding {
  tenant: string;
  tree_name: string;
}

export interface ComplianceSummary {
  trees: number;
  modes: Record<ScanMode, number>;
  open_findings: number;
  resolved_findings: number;
  allowlisted: number;
  license_rules: number;
  invalid_rules: number;
  spdx_list_version: string;
  rules_version: number;
  scan_rules: number;
  tree_list: TreeCompliance[];
  recent: RecentSecretFinding[];
}

// ---- normalisation ----

function arr<T>(v: T[] | null | undefined): T[] {
  return Array.isArray(v) ? v : [];
}

export function normSettings(s: ComplianceSettings): ComplianceSettings {
  return {
    ...s,
    mode: s.mode || "warn",
    exclude: arr(s.exclude),
    max_file_bytes: s.max_file_bytes ?? 0,
  };
}

export function normAudit(a: LicenseAudit): LicenseAudit {
  return {
    ...a,
    licenses: arr(a.licenses).map((u) => ({ ...u, globs: arr(u.globs) })),
    unlicensed: arr(a.unlicensed),
    exposed: arr(a.exposed),
    rules: arr(a.rules).map((r) => ({ ...r, problems: arr(r.problems) })),
  };
}

export function normSummary(s: ComplianceSummary): ComplianceSummary {
  const modes = s.modes ?? { off: 0, warn: 0, block: 0 };
  return {
    ...s,
    modes: {
      off: modes.off ?? 0,
      warn: modes.warn ?? 0,
      block: modes.block ?? 0,
    },
    tree_list: arr(s.tree_list),
    recent: arr(s.recent),
  };
}

// ---- endpoints ----

const seg = encodeURIComponent;

export function createComplianceApi(
  client: ApiClient,
  tenant: string,
  tree: string,
) {
  const base = `/api/v1/trees/${seg(tenant)}/${seg(tree)}/compliance`;
  return {
    settings: async () =>
      normSettings(
        await client.request<ComplianceSettings>(`${base}/settings`),
      ),
    updateSettings: async (body: ComplianceSettingsRequest) =>
      normSettings(
        await client.request<ComplianceSettings>(`${base}/settings`, {
          method: "PUT",
          body,
        }),
      ),
    findings: async (
      status: FindingStatus | "all",
    ): Promise<SecretFindingList> => {
      const v = await client.request<SecretFindingList | null>(
        `${base}/findings`,
        { query: { status, limit: 500 } },
      );
      return {
        findings: arr(v?.findings),
        open: v?.open ?? 0,
        resolved: v?.resolved ?? 0,
      };
    },
    resolve: (
      id: string,
      body: { resolution?: Resolution; note?: string; reopen?: boolean },
    ) =>
      client.request<SecretFinding>(`${base}/findings/${seg(id)}/resolve`, {
        method: "POST",
        body,
      }),
    allowlist: async (): Promise<AllowList> => {
      const v = await client.request<AllowList | null>(`${base}/allowlist`);
      return { tree_id: v?.tree_id ?? "", entries: arr(v?.entries) };
    },
    allow: (fingerprint: string, reason: string) =>
      client.request<AllowEntry>(`${base}/allowlist`, {
        method: "POST",
        body: { fingerprint, reason },
      }),
    disallow: (fingerprint: string) =>
      client.request<void>(`${base}/allowlist`, {
        method: "DELETE",
        query: { fingerprint },
      }),
    licenses: async (ref?: string) =>
      normAudit(
        await client.request<LicenseAudit>(
          `${base}/licenses`,
          ref ? { query: { ref } } : {},
        ),
      ),
  };
}

export type ComplianceApi = ReturnType<typeof createComplianceApi>;

export async function fetchComplianceSummary(
  client: ApiClient,
): Promise<ComplianceSummary> {
  return normSummary(
    await client.request<ComplianceSummary>("/api/v1/admin/compliance"),
  );
}

export const complianceKeys = {
  all: (tenant: string, tree: string) =>
    ["tree", tenant, tree, "compliance"] as const,
  settings: (tenant: string, tree: string) =>
    ["tree", tenant, tree, "compliance", "settings"] as const,
  findings: (tenant: string, tree: string, status: string) =>
    ["tree", tenant, tree, "compliance", "findings", status] as const,
  allowlist: (tenant: string, tree: string) =>
    ["tree", tenant, tree, "compliance", "allowlist"] as const,
  licenses: (tenant: string, tree: string) =>
    ["tree", tenant, tree, "compliance", "licenses"] as const,
  summary: ["admin", "compliance"] as const,
};

// ---- helpers ----

export const MODE_LABELS: Record<
  ScanMode,
  { label: string; description: string }
> = {
  off: { label: "Off", description: "Pushes are not scanned." },
  warn: {
    label: "Warn",
    description:
      "Pushes go through; possible secrets are recorded and reported.",
  },
  block: {
    label: "Block",
    description:
      "Pushes with possible secrets are rejected. Owners may bypass; allowlisted secrets pass.",
  },
};

export const RESOLUTION_LABELS: Record<Resolution, string> = {
  revoked: "Revoked / rotated",
  false_positive: "False positive",
  test_data: "Test data",
  wont_fix: "Won't fix",
};

/** Validation of an allowlist form ("" = valid). */
export function allowFormErrors(
  fingerprint: string,
  reason: string,
): { fingerprint: string; reason: string } {
  const fp = fingerprint.trim().toLowerCase();
  return {
    fingerprint: !fp
      ? "Required."
      : /^[0-9a-f]{24}$/.test(fp)
        ? ""
        : "A fingerprint is 24 hexadecimal digits.",
    reason: !reason.trim()
      ? "Required."
      : reason.trim().length > 500
        ? "At most 500 characters."
        : "",
  };
}

/** "1 MiB (default)" / "2 MiB" / "512 KiB". */
export function maxFileText(bytes: number): string {
  if (!bytes) return "1 MiB (default)";
  if (bytes % (1 << 20) === 0) return `${bytes / (1 << 20)} MiB`;
  if (bytes % 1024 === 0) return `${bytes / 1024} KiB`;
  return `${bytes} bytes`;
}
