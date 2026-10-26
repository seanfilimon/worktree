/**
 * Tenant lifecycle endpoints (docs/ENGINEERING_PLAN.md §16.4): contact
 * email and verification, slug renames (old slugs answer
 * `301 tenant_moved`), deletion with a recovery window and restore.
 *
 * Field names mirror services/server-go/internal/iam/tenant_lifecycle*.go.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { isApiError, type ApiClient } from "./client";
import { useApi } from "./context";
import type { ISODateString } from "./types";
import type { TenantV05 } from "./v05ops";

const V1 = "/api/v1";
const seg = encodeURIComponent;

/** Tenant with the v0.8 contact email. */
export interface TenantV08 extends TenantV05 {
  email?: string;
  email_verified_at?: ISODateString | null;
}

/** Where a verification code went (`log`: SMTP is off, the code is in the server log). */
export interface VerificationSent {
  email: string;
  delivery: "email" | "log";
  expires_at: ISODateString;
}

export interface SlugReservation {
  slug: string;
  tenant_id: string;
  renamed_to?: string;
  reserved_at: ISODateString;
  expires_at: ISODateString;
}

export interface RenameTenantResponse {
  tenant: TenantV08;
  reservation: SlugReservation;
}

/** A scheduled deletion: the tenant is suspended until `purge_at`. */
export interface TenantDeletion {
  tenant_id: string;
  slug: string;
  requested_by: string;
  requested_at: ISODateString;
  purge_at: ISODateString;
  previous_status?: string;
  previous_status_reason?: string;
  revoked_keys: number;
}

/** Organization slug rules (the server checks them too). */
export const SLUG_RE = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;

/** Simple address check before the server's. */
export const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** The new slug of a `tenant_moved` error, if `err` is one. */
export function movedTo(err: unknown): string | undefined {
  if (!isApiError(err) || err.code !== "tenant_moved") return undefined;
  const details = err.details.details;
  if (details && typeof details === "object" && "new_slug" in details) {
    const s = (details as Record<string, unknown>).new_slug;
    if (typeof s === "string") return s;
  }
  return undefined;
}

export function createLifecycleApi(client: ApiClient) {
  const tenant = (t: string) => `${V1}/tenants/${seg(t)}`;
  return {
    setEmail: (t: string, email: string) => client.request<TenantV08>(tenant(t), { method: "PATCH", body: { email } }),
    verifyEmail: (t: string, token: string) =>
      client.request<TenantV08>(`${tenant(t)}/email/verify`, { method: "POST", body: { token } }),
    resendEmail: (t: string) => client.request<VerificationSent>(`${tenant(t)}/email/resend`, { method: "POST" }),
    rename: (t: string, slug: string) =>
      client.request<RenameTenantResponse>(`${tenant(t)}/rename`, { method: "POST", body: { slug } }),
    remove: (t: string) => client.request<TenantDeletion>(tenant(t), { method: "DELETE" }),
    restore: (t: string) => client.request<TenantV08>(`${tenant(t)}/restore`, { method: "POST" }),
    /** The scheduled deletion, or null when none is scheduled (404). */
    deletion: async (t: string): Promise<TenantDeletion | null> => {
      try {
        return await client.request<TenantDeletion>(`${tenant(t)}/deletion`);
      } catch (err) {
        if (isApiError(err) && err.status === 404) return null;
        throw err;
      }
    },
  };
}

export type LifecycleApi = ReturnType<typeof createLifecycleApi>;

export const lifecycleKeys = {
  deletion: (t: string) => ["tenant", t, "deletion"] as const,
};

export function useLifecycleApi(): LifecycleApi {
  const { client } = useApi();
  return useMemo(() => createLifecycleApi(client), [client]);
}

export function useTenantDeletion(tenant: string) {
  const api = useLifecycleApi();
  return useQuery({ queryKey: lifecycleKeys.deletion(tenant), queryFn: () => api.deletion(tenant), retry: false });
}

/** "29d 23h", "4h 05m", "3m 20s", "now" until `targetMs`. */
export function countdown(targetMs: number, nowMs: number): string {
  let s = Math.max(0, Math.floor((targetMs - nowMs) / 1000));
  if (s === 0) return "now";
  const d = Math.floor(s / 86400);
  s -= d * 86400;
  const h = Math.floor(s / 3600);
  s -= h * 3600;
  const m = Math.floor(s / 60);
  s -= m * 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${pad(m)}m`;
  return `${m}m ${pad(s)}s`;
}
