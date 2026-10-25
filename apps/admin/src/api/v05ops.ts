/**
 * v0.5 operations endpoints (docs/ENGINEERING_PLAN.md §13.3, §13.6, §13.7):
 * garbage collection, tenant usage / limits / status, signing keys,
 * declarative access config and the NDJSON audit export.
 *
 * Field names mirror services/server-go/internal/domain/{v05.go,types.go}.
 * Go encodes empty slices as `null`; every function here normalises them to
 * `[]`. Features whose server-side service is missing answer
 * `501 not_implemented` — see {@link isNotImplemented}.
 */
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { isApiError, type ApiClient } from "./client";
import { useApi } from "./context";
import type { AuditEvent, AuditQuery, ISODateString, Tenant } from "./types";

const V1 = "/api/v1";
const seg = encodeURIComponent;

// ---- wire types ----

export const TENANT_STATUSES = ["active", "suspended", "deactivated"] as const;
export type TenantStatus = (typeof TENANT_STATUSES)[number];

/** Per-tenant quotas; a missing / zero field means "server default". */
export interface TenantLimits {
  max_trees?: number;
  max_storage_bytes?: number;
  max_branches_per_tree?: number;
  max_members?: number;
  requests_per_minute?: number;
}

export type LimitKey = keyof TenantLimits;

export const LIMIT_KEYS: LimitKey[] = [
  "max_trees",
  "max_storage_bytes",
  "max_branches_per_tree",
  "max_members",
  "requests_per_minute",
];

/** Tenant with the v0.5 fields (Status "" = active). */
export interface TenantV05 extends Tenant {
  status?: TenantStatus | "";
  status_reason?: string;
  limits?: TenantLimits | null;
}

export interface UpdateTenantStatusRequest {
  status: TenantStatus;
  status_reason: string;
}

/** GET /tenants/{t}/usage. */
export interface TenantUsage {
  trees: number;
  branches: number;
  members: number;
  storage_bytes: number;
  /** The tenant's own overrides. */
  limits: TenantLimits;
  /** Tenant overrides merged over the server defaults (0 / missing = unlimited). */
  effective: TenantLimits;
}

/** One garbage-collection run. */
export interface GCReport {
  dry_run: boolean;
  scanned: number;
  reachable: number;
  unreachable: number;
  /** Newly marked for deletion (the grace period starts). */
  marked: number;
  /** Past the grace period and removed. */
  deleted: number;
  freed_bytes: number;
  started_at: ISODateString;
  finished_at: ISODateString;
}

/** Server GC / retention configuration (keys as reported by the server). */
export type GCConfig = Record<string, unknown>;

/** GET /admin/gc. */
export interface GCStatus {
  config: GCConfig;
  /** Newest first (the server keeps the last 20). */
  runs: GCReport[];
}

/** An account's registered Ed25519 public key. */
export interface SigningKey {
  id: string;
  account_id: string;
  name: string;
  algorithm: string;
  /** Base64 (32 bytes). */
  public_key: string;
  fingerprint: string;
  created_at: ISODateString;
  last_used_at?: ISODateString;
  revoked_at?: ISODateString;
}

export interface AddSigningKeyRequest {
  name: string;
  /** Standard base64 of the 32-byte public key. */
  public_key: string;
}

/** The last applied declarative access document of a tenant (or tree). */
export interface AccessConfigDoc {
  tenant_id: string;
  tree_id?: string;
  kind: string;
  /** Base64 of the canonical JSON document (Go []byte). */
  content: string;
  /** Hex BLAKE3 of the content. */
  hash: string;
  updated_by: string;
  updated_at: ISODateString;
}

/** A plan / apply entry: the server sends strings; objects are tolerated. */
export type AccessPlanItem = string | Record<string, unknown>;

/** POST /tenants/{t}/access-config/plan|apply. */
export interface AccessPlanResult {
  accepted: boolean;
  errors: AccessPlanItem[];
  changes: AccessPlanItem[];
}

/** GET /audit with the v0.5 filters. */
export interface AuditFilterQuery extends AuditQuery {
  /** RFC 3339, inclusive. */
  since?: string;
  /** RFC 3339, exclusive. */
  until?: string;
  resource_type?: string;
  resource_id?: string;
}

// ---- helpers ----

function arr<T>(v: T[] | null | undefined): T[] {
  return v ?? [];
}

/** True for `501 not_implemented` (the feature's server-side service is missing). */
export function isNotImplemented(err: unknown): boolean {
  return isApiError(err) && (err.status === 501 || err.code === "not_implemented");
}

/** Query retry policy: never retry 4xx or 501, otherwise twice. */
export function retryUnlessFinal(count: number, err: unknown): boolean {
  if (isNotImplemented(err)) return false;
  if (isApiError(err) && err.status >= 400 && err.status < 500) return false;
  return count < 2;
}

/** Normalised effective status ("" → active). */
export function tenantStatus(t: TenantV05 | undefined): TenantStatus {
  const s = t?.status;
  return s === "suspended" || s === "deactivated" ? s : "active";
}

/** Drops zero / missing values so "server default" round-trips as absent. */
export function compactLimits(l: TenantLimits | null | undefined): TenantLimits {
  const out: TenantLimits = {};
  if (!l) return out;
  for (const k of LIMIT_KEYS) {
    const v = l[k];
    if (typeof v === "number" && Number.isFinite(v) && v > 0) out[k] = v;
  }
  return out;
}

function normUsage(u: TenantUsage): TenantUsage {
  return {
    trees: u.trees ?? 0,
    branches: u.branches ?? 0,
    members: u.members ?? 0,
    storage_bytes: u.storage_bytes ?? 0,
    limits: compactLimits(u.limits),
    effective: compactLimits(u.effective),
  };
}

function normPlan(r: AccessPlanResult): AccessPlanResult {
  return { accepted: Boolean(r.accepted), errors: arr(r.errors), changes: arr(r.changes) };
}

/** Decodes base64 to UTF-8 text (falls back to the raw value when it is not base64). */
export function decodeBase64Text(b64: string): string {
  try {
    const bin = atob(b64);
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return b64;
  }
}

/** Pretty-prints JSON text; returns the input unchanged when it is not JSON. */
export function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

/** Parses editor text as the canonical access document (`{roles, policies}`). */
export function parseAccessDocument(text: string): { content?: Record<string, unknown>; error?: string } {
  if (text.trim() === "") return { error: "Paste a canonical access document." };
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch (e) {
    return { error: `Invalid JSON: ${e instanceof Error ? e.message : String(e)}` };
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return { error: "The document must be a JSON object." };
  const doc = v as Record<string, unknown>;
  for (const k of ["roles", "policies"]) {
    if (doc[k] !== undefined && doc[k] !== null && !Array.isArray(doc[k])) return { error: `"${k}" must be an array.` };
  }
  if (doc.roles === undefined && doc.policies === undefined) return { error: 'Expected "roles" and/or "policies".' };
  return { content: doc };
}

/**
 * Normalises a pasted Ed25519 public key to standard base64 of 32 bytes.
 * Accepts standard or URL-safe base64 (padding optional) and 64-char hex.
 */
export function normalizePublicKey(input: string): { key?: string; error?: string } {
  const s = input.trim().replace(/\s+/g, "");
  if (s === "") return { error: "Paste the public key." };
  let bytes: Uint8Array | undefined;
  if (/^[0-9a-fA-F]{64}$/.test(s)) {
    bytes = Uint8Array.from(s.match(/../g) ?? [], (h) => parseInt(h, 16));
  } else if (/^[A-Za-z0-9+/_-]+={0,2}$/.test(s)) {
    const std = s.replace(/-/g, "+").replace(/_/g, "/");
    try {
      const bin = atob(std.padEnd(std.length + ((4 - (std.length % 4)) % 4), "="));
      bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    } catch {
      bytes = undefined;
    }
  }
  if (!bytes) return { error: "Not valid base64 (or hex)." };
  if (bytes.length !== 32) return { error: `An Ed25519 public key is 32 bytes; this is ${bytes.length}.` };
  return { key: btoa(String.fromCharCode(...bytes)) };
}

/** Display text of a plan entry. */
export function planItemText(item: AccessPlanItem): string {
  if (typeof item === "string") return item;
  const msg = item.message;
  if (typeof msg === "string" && msg) {
    const code = typeof item.code === "string" && item.code ? `${item.code}: ` : "";
    return `${code}${msg}`;
  }
  return JSON.stringify(item);
}

/** Turns a JSON array response into NDJSON (older servers ignore `format`). */
function toNdjson(text: string): string {
  const trimmed = text.trim();
  if (!trimmed.startsWith("[")) return text;
  try {
    const events = JSON.parse(trimmed) as AuditEvent[] | null;
    return arr(events)
      .map((e) => JSON.stringify(e))
      .map((l) => `${l}\n`)
      .join("");
  } catch {
    return text;
  }
}

// ---- endpoints ----

export function createOpsApi(client: ApiClient) {
  const tenant = (t: string) => `${V1}/tenants/${seg(t)}`;
  const adminTenant = (t: string) => `${V1}/admin/tenants/${seg(t)}`;

  return {
    gc: {
      status: async (): Promise<GCStatus> => {
        const s = await client.request<GCStatus>(`${V1}/admin/gc`);
        return { config: s?.config ?? {}, runs: arr(s?.runs) };
      },
      run: (dryRun: boolean) =>
        client.request<GCReport>(`${V1}/admin/gc`, { method: "POST", query: { dry_run: dryRun ? "true" : "false" } }),
    },

    tenants: {
      usage: async (t: string) => normUsage(await client.request<TenantUsage>(`${tenant(t)}/usage`)),
      limits: async (t: string) => compactLimits(await client.request<TenantLimits | null>(`${adminTenant(t)}/limits`)),
      setLimits: async (t: string, limits: TenantLimits) =>
        compactLimits(
          await client.request<TenantLimits | null>(`${adminTenant(t)}/limits`, { method: "PUT", body: compactLimits(limits) }),
        ),
      setStatus: (t: string, body: UpdateTenantStatusRequest) =>
        client.request<TenantV05>(adminTenant(t), { method: "PATCH", body }),
    },

    signingKeys: {
      list: async () => arr(await client.request<SigningKey[] | null>(`${V1}/auth/keys`)),
      add: (body: AddSigningKeyRequest) => client.request<SigningKey>(`${V1}/auth/keys`, { method: "POST", body }),
      revoke: (id: string) => client.request<void>(`${V1}/auth/keys/${seg(id)}`, { method: "DELETE" }),
    },

    accessConfig: {
      /** The last applied document, or null when none was applied (404). */
      get: async (t: string): Promise<AccessConfigDoc | null> => {
        try {
          return await client.request<AccessConfigDoc>(`${tenant(t)}/access-config`);
        } catch (err) {
          if (isApiError(err) && err.status === 404 && err.code === "not_found") return null;
          throw err;
        }
      },
      plan: async (t: string, content: unknown) =>
        normPlan(await client.request<AccessPlanResult>(`${tenant(t)}/access-config/plan`, { method: "POST", body: { content } })),
      apply: async (t: string, content: unknown) =>
        normPlan(await client.request<AccessPlanResult>(`${tenant(t)}/access-config/apply`, { method: "POST", body: { content } })),
    },

    audit: {
      /** Every matching event as NDJSON (no limit). */
      exportNdjson: async (q: Omit<AuditFilterQuery, "limit">): Promise<string> => {
        const res = await client.raw(`${V1}/audit`, { query: { ...q, limit: undefined, format: "ndjson" } });
        return toNdjson(await res.text());
      },
    },
  };
}

export type OpsApi = ReturnType<typeof createOpsApi>;

// ---- query keys + hooks ----

export const opsKeys = {
  gc: ["admin", "gc"] as const,
  usage: (t: string) => ["tenant", t, "usage"] as const,
  limits: (t: string) => ["tenant", t, "limits"] as const,
  signingKeys: ["auth", "keys"] as const,
  accessConfig: (t: string) => ["tenant", t, "iam", "access-config"] as const,
};

/** The operations API bound to the app's client. */
export function useOpsApi(): OpsApi {
  const { client } = useApi();
  return useMemo(() => createOpsApi(client), [client]);
}

export function useGcStatus(enabled = true) {
  const ops = useOpsApi();
  return useQuery({ queryKey: opsKeys.gc, queryFn: ops.gc.status, enabled, retry: retryUnlessFinal });
}

export function useTenantUsage(tenant: string, enabled = true) {
  const ops = useOpsApi();
  return useQuery({
    queryKey: opsKeys.usage(tenant),
    queryFn: () => ops.tenants.usage(tenant),
    enabled: enabled && tenant !== "",
    retry: retryUnlessFinal,
    placeholderData: keepPreviousData,
  });
}

export function useTenantLimits(tenant: string, enabled = true) {
  const ops = useOpsApi();
  return useQuery({
    queryKey: opsKeys.limits(tenant),
    queryFn: () => ops.tenants.limits(tenant),
    enabled: enabled && tenant !== "",
    retry: retryUnlessFinal,
  });
}

export function useSigningKeys() {
  const ops = useOpsApi();
  return useQuery({ queryKey: opsKeys.signingKeys, queryFn: ops.signingKeys.list, retry: retryUnlessFinal });
}

export function useAccessConfig(tenant: string) {
  const ops = useOpsApi();
  return useQuery({
    queryKey: opsKeys.accessConfig(tenant),
    queryFn: () => ops.accessConfig.get(tenant),
    enabled: tenant !== "",
    retry: retryUnlessFinal,
  });
}
