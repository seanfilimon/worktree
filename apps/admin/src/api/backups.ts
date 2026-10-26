/**
 * v0.7 server backups (docs/ENGINEERING_PLAN.md §15.2): create, list,
 * download and delete `.wtbak` archives. Server administrators only.
 *
 * Field names mirror services/server-go/internal/domain/v07.go
 * (BackupInfo) and internal/backup (Status, Config, StoreInfo). v0.8
 * server-ops (§16.11) adds the backup target (disk or S3) and max age.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ApiClient } from "./client";
import { useApi } from "./context";
import type { ISODateString } from "./types";
import { retryUnlessFinal } from "./v05ops";

const V1 = "/api/v1";
const seg = encodeURIComponent;

/** One backup archive on the server. */
export interface BackupInfo {
  id: string;
  /** File path on the server, or "s3://bucket/prefix/<id>.wtbak" for S3 targets. */
  path: string;
  size_bytes: number;
  /** Objects in the archive (0 for archives found without an index entry). */
  objects: number;
  created_at: ISODateString;
  /** Username, "system" for scheduled backups, "" for unindexed archives. */
  created_by: string;
  note?: string;
}

/** Where backup archives are written (`--backup-target`). */
export type BackupTarget = "disk" | "s3";

export interface BackupConfig {
  dir: string;
  /** v0.8 (§16.11); older servers omit it ("disk"). */
  target: BackupTarget;
  /** "/data/backups" or "s3://bucket/prefix/"; older servers omit it (= dir). */
  location: string;
  /** Go duration ("0s" = no age limit, `--backup-max-age`). */
  max_age: string;
  max_age_seconds: number;
  /** Go duration ("0s" = no schedule). */
  interval: string;
  interval_seconds: number;
  /** Scheduled backups kept (0 keeps all). */
  keep: number;
}

export interface BackupStoreInfo {
  /** "disk", "s3" or "other". */
  kind: string;
  root?: string;
  endpoint?: string;
  bucket?: string;
  prefix?: string;
  region?: string;
  secure?: boolean;
  compress: boolean;
}

/** GET /admin/backups. */
export interface BackupStatus {
  config: BackupConfig;
  store: BackupStoreInfo;
  supported: boolean;
  running: boolean;
  next_run_at?: ISODateString;
  last_error?: string;
  backups: BackupInfo[];
}

export interface CreateBackupRequest {
  note?: string;
}

/** Maximum note length accepted by the server. */
export const MAX_BACKUP_NOTE = 500;

/** Who created a backup, for display. */
export function backupAuthor(b: BackupInfo): string {
  if (b.created_by === "system") return "schedule";
  return b.created_by || "—";
}

/** Download file name of a backup. */
export function backupFileName(id: string): string {
  return `${id}.wtbak`;
}

/** "S3 · s3://wt/backups/" / "Disk · /data/backups". */
export function backupTargetText(c: BackupConfig): string {
  return `${c.target === "s3" ? "S3" : "Disk"} · ${c.location || c.dir || "—"}`;
}

function normConfig(c: Partial<BackupConfig> | undefined): BackupConfig {
  const dir = c?.dir ?? "";
  return {
    dir,
    target: c?.target === "s3" ? "s3" : "disk",
    location: c?.location || dir,
    interval: c?.interval ?? "0s",
    interval_seconds: c?.interval_seconds ?? 0,
    keep: c?.keep ?? 0,
    max_age: c?.max_age ?? "0s",
    max_age_seconds: c?.max_age_seconds ?? 0,
  };
}

function normStatus(s: Partial<BackupStatus> | null | undefined): BackupStatus {
  return {
    config: normConfig(s?.config),
    store: { kind: "", compress: false, ...(s?.store ?? {}) },
    supported: s?.supported ?? false,
    running: s?.running ?? false,
    next_run_at: s?.next_run_at,
    last_error: s?.last_error,
    backups: s?.backups ?? [],
  };
}

export function createBackupsApi(client: ApiClient) {
  const base = `${V1}/admin/backups`;
  return {
    status: async (): Promise<BackupStatus> => normStatus(await client.request<BackupStatus | null>(base)),
    create: (body: CreateBackupRequest) => client.request<BackupInfo>(base, { method: "POST", body }),
    /** Archive bytes (authenticated, so not a plain link). */
    download: async (id: string): Promise<ArrayBuffer> => (await client.raw(`${base}/${seg(id)}/download`)).arrayBuffer(),
    remove: (id: string) => client.request<void>(`${base}/${seg(id)}`, { method: "DELETE" }),
  };
}

export type BackupsApi = ReturnType<typeof createBackupsApi>;

export const backupKeys = {
  status: ["admin", "backups"] as const,
};

export function useBackupsApi(): BackupsApi {
  const { client } = useApi();
  return useMemo(() => createBackupsApi(client), [client]);
}

export function useBackupStatus(enabled = true) {
  const api = useBackupsApi();
  return useQuery({
    queryKey: backupKeys.status,
    queryFn: api.status,
    enabled,
    retry: retryUnlessFinal,
    // Poll while a backup runs (e.g. a scheduled one).
    refetchInterval: (q) => (q.state.data?.running ? 3000 : false),
  });
}
