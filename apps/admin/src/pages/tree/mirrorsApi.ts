/**
 * Git mirrors endpoints (docs/ENGINEERING_PLAN.md §15.6) used by MirrorsTab.
 *
 * Field names mirror services/server-go/internal/domain/v07.go (Mirror) and
 * internal/mirror (GitStatus, SyncResult). Go encodes empty slices as
 * `null`; the helpers below normalise lists to `[]`.
 */
import type { ApiClient } from "@/api/client";
import type { ISODateString } from "@/api/types";

// ---- wire types ----

export type MirrorStatus = "ok" | "error" | "running" | "disabled";

/** A Git push mirror (domain.Mirror). The credential is never returned. */
export interface Mirror {
  id: string;
  tenant_id: string;
  tree_id: string;
  url: string;
  has_credential: boolean;
  /** Branch globs; empty = every branch. */
  branches: string[];
  tags: boolean;
  active: boolean;
  created_by: string;
  created_at: ISODateString;
  last_sync_at?: ISODateString;
  last_status?: MirrorStatus | "";
  last_error?: string;
}

/** Whether the server can run mirrors (mirror.GitStatus). */
export interface GitStatus {
  available: boolean;
  version?: string;
  error?: string;
}

export interface MirrorList {
  mirrors: Mirror[];
  git: GitStatus;
}

export interface RefUpdate {
  ref: string;
  new?: string;
  /** created | updated | forced | deleted | up_to_date | rejected | error */
  status: string;
  reason?: string;
}

export interface SyncResult {
  commits: number;
  refs: RefUpdate[];
  pushed: boolean;
  /** Paths left out because their license does not permit redistribution. */
  excluded: string[];
  skipped: string[];
  error?: string;
  duration_ms: number;
}

export interface MirrorSync {
  mirror: Mirror;
  /** Absent when the sync was only started (wait=false). */
  result?: SyncResult;
}

export interface CreateMirrorRequest {
  url: string;
  branches?: string[];
  tags?: boolean;
  /** For http(s) remotes: a token or user:password. */
  credential?: string;
  active?: boolean;
}

/** Absent fields are unchanged; `credential: ""` removes it, `branches: []` mirrors every branch. */
export type UpdateMirrorRequest = Partial<CreateMirrorRequest>;

// ---- endpoints ----

const seg = encodeURIComponent;

function arr<T>(v: T[] | null | undefined): T[] {
  return Array.isArray(v) ? v : [];
}

export function normMirror(m: Mirror): Mirror {
  return { ...m, branches: arr(m.branches), has_credential: Boolean(m.has_credential), tags: Boolean(m.tags), active: Boolean(m.active) };
}

function normResult(r: SyncResult | undefined): SyncResult | undefined {
  if (!r) return undefined;
  return { ...r, commits: r.commits ?? 0, refs: arr(r.refs), excluded: arr(r.excluded), skipped: arr(r.skipped), pushed: Boolean(r.pushed) };
}

export function createMirrorsApi(client: ApiClient, tenant: string, tree: string) {
  const base = `/api/v1/trees/${seg(tenant)}/${seg(tree)}/mirrors`;
  const item = (id: string) => `${base}/${seg(id)}`;
  return {
    list: async (): Promise<MirrorList> => {
      const v = await client.request<Partial<MirrorList> | null>(base);
      return { mirrors: arr(v?.mirrors).map(normMirror), git: v?.git ?? { available: true } };
    },
    create: async (body: CreateMirrorRequest) => normMirror(await client.request<Mirror>(base, { method: "POST", body })),
    update: async (id: string, body: UpdateMirrorRequest) => normMirror(await client.request<Mirror>(item(id), { method: "PATCH", body })),
    remove: (id: string) => client.request<void>(item(id), { method: "DELETE" }),
    sync: async (id: string): Promise<MirrorSync> => {
      const v = await client.request<MirrorSync>(`${item(id)}/sync`, { method: "POST" });
      return { mirror: normMirror(v.mirror), result: normResult(v.result) };
    },
  };
}

export type MirrorsApi = ReturnType<typeof createMirrorsApi>;

export const mirrorsKey = (tenant: string, tree: string) => ["tree", tenant, tree, "mirrors"] as const;

// ---- form validation ----

const BRANCH_GLOB_RE = /^[^\s~^:?\\]+$/;
const SCP_RE = /^[A-Za-z0-9._~-]+@[A-Za-z0-9][A-Za-z0-9.-]+:.+$/;

export interface MirrorFormErrors {
  url: string;
  branches: string;
  credential: string;
}

/** Validation messages for the mirror form ("" = valid). */
export function mirrorFormErrors(url: string, branches: readonly string[], credential: string): MirrorFormErrors {
  const u = url.trim();
  let urlError = "";
  let scheme = "";
  if (!u) urlError = "Required.";
  else if (SCP_RE.test(u)) scheme = "ssh:";
  else {
    try {
      scheme = new URL(u).protocol;
      if (!["https:", "http:", "ssh:", "file:"].includes(scheme)) urlError = "Use an https://, ssh:// or user@host:path Git remote.";
      else if (/^[a-z]+:\/\/[^/@]*:[^/@]*@/i.test(u)) urlError = "Do not put a password in the URL: use the credential field.";
    } catch {
      urlError = "Enter a Git remote, e.g. https://github.com/acme/web.git or git@github.com:acme/web.git.";
    }
  }
  const bad = branches.find((b) => !BRANCH_GLOB_RE.test(b) || b.split("/").some((s) => s === ""));
  const credentialError =
    credential.trim() && scheme !== "" && scheme !== "https:" && scheme !== "http:" ? "Credentials apply to http(s) remotes; SSH remotes use the server's keys." : "";
  return { url: urlError, branches: bad ? `“${bad}” is not a branch pattern.` : "", credential: credentialError };
}

/** One-line summary of a sync for the toast. */
export function syncSummary(r: SyncResult | undefined): string {
  if (!r) return "Sync started.";
  if (r.error) return r.error;
  const commits = `${r.commits} new commit${r.commits === 1 ? "" : "s"}`;
  const changed = r.refs.filter((u) => u.status !== "up_to_date").length;
  const refs = changed === 0 ? "everything up to date" : `${changed} ref${changed === 1 ? "" : "s"} updated`;
  const excluded = r.excluded.length ? `, ${r.excluded.length} path${r.excluded.length === 1 ? "" : "s"} not redistributable` : "";
  return `${commits}, ${refs}${excluded}`;
}
