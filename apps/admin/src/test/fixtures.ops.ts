import type { AccessConfigDoc, GCReport, GCStatus, SigningKey, TenantUsage } from "@/api/v05ops";
import { tenant } from "./fixtures";

const T0 = "2026-10-20T12:00:00Z";

export const gcRun: GCReport = {
  dry_run: false,
  scanned: 1200,
  reachable: 1150,
  unreachable: 50,
  marked: 20,
  deleted: 30,
  freed_bytes: 3 * 1024 * 1024,
  started_at: "2026-10-24T03:00:00Z",
  finished_at: "2026-10-24T03:00:02.500Z",
};

export const gcDryRun: GCReport = { ...gcRun, dry_run: true, marked: 0, deleted: 0, freed_bytes: 0, started_at: "2026-10-23T03:00:00Z", finished_at: "2026-10-23T03:00:01Z" };

export const gcStatus: GCStatus = {
  config: {
    // The shape GET /admin/gc returns: readable durations plus seconds.
    interval: "24h0m0s",
    interval_seconds: 86400,
    grace: "720h0m0s",
    grace_seconds: 2592000,
    staged_retention: 168 * 3600 * 1e9,
    audit_retention: "0s",
    audit_retention_seconds: 0,
    rate_limit: 1200,
  },
  runs: [gcRun, gcDryRun],
};

export const usage: TenantUsage = {
  trees: 4,
  branches: 17,
  members: 9,
  storage_bytes: 512 * 1024 * 1024,
  limits: { max_trees: 10 },
  effective: { max_trees: 10, max_storage_bytes: 1024 * 1024 * 1024, max_members: 50 },
};

export const signingKey: SigningKey = {
  id: "key-1",
  account_id: "acc-1",
  name: "laptop",
  algorithm: "ed25519",
  public_key: btoa(String.fromCharCode(...new Uint8Array(32).fill(7))),
  fingerprint: "SHA256:abcDEF123",
  created_at: T0,
};

export const revokedKey: SigningKey = { ...signingKey, id: "key-0", name: "old-desktop", fingerprint: "SHA256:zzz999", revoked_at: T0 };

export const accessContent = { roles: [{ name: "release-manager", description: "Cut releases", permissions: ["release:*"] }], policies: [] };

export const accessDoc: AccessConfigDoc = {
  tenant_id: tenant.id,
  kind: "combined",
  content: btoa(JSON.stringify(accessContent)),
  hash: "b3a1c0ffee",
  updated_by: "alice",
  updated_at: T0,
};
