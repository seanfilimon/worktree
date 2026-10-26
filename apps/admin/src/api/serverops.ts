/**
 * v0.8 server operations (docs/ENGINEERING_PLAN.md §16.11): the durable
 * email outbox and data retention. Server administrators only; older
 * servers answer 501 not_implemented (or 404 for the unknown route).
 *
 * Field names mirror the Go server's notify outbox (OutboxList,
 * OutboxEmail) and retention (Status, Config, Report).
 */
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { isApiError, type ApiClient } from "./client";
import { useApi } from "./context";
import type { ISODateString } from "./types";
import { isNotImplemented, retryUnlessFinal } from "./v05ops";

const V1 = "/api/v1";
const seg = encodeURIComponent;

// ---- Email outbox ----

export type OutboxStatus = "pending" | "sent" | "dead";

/** One queued email. */
export interface OutboxEmail {
  /** "em-…". */
  id: string;
  to: string;
  subject: string;
  notification_id?: string;
  status: OutboxStatus;
  attempts: number;
  last_error?: string;
  created_at: ISODateString;
  updated_at: ISODateString;
  next_attempt_at?: ISODateString;
  /** When a pending email is dead-lettered if it still fails. */
  deadline_at: ISODateString;
  sent_at?: ISODateString;
  dead_at?: ISODateString;
}

export interface OutboxCounts {
  pending: number;
  sent: number;
  dead: number;
}

/** GET /admin/outbox. */
export interface OutboxList {
  /** SMTP is configured (`--smtp-url`); otherwise nothing is sent. */
  enabled: boolean;
  /** Go duration after which failing emails are dead-lettered ("24h0m0s"). */
  dead_after: string;
  dead_after_seconds: number;
  counts: OutboxCounts;
  /** Newest first. */
  emails: OutboxEmail[];
}

export interface OutboxQuery {
  status?: OutboxStatus;
  limit?: number;
}

/** POST /admin/outbox/retry. */
export interface OutboxRetryAllResult {
  retried: number;
}

function normOutbox(l: Partial<Omit<OutboxList, "emails">> & { emails?: OutboxEmail[] | null } | null | undefined): OutboxList {
  return {
    enabled: l?.enabled ?? false,
    dead_after: l?.dead_after ?? "24h0m0s",
    dead_after_seconds: l?.dead_after_seconds ?? 86400,
    counts: { pending: 0, sent: 0, dead: 0, ...(l?.counts ?? {}) },
    emails: l?.emails ?? [],
  };
}

/** The email can be retried (pending now, or dead-lettered). */
export function canRetryEmail(e: OutboxEmail): boolean {
  return e.status === "pending" || e.status === "dead";
}

/** The timestamp that matters for an email's status: next attempt / sent / dead-lettered. */
export function outboxEmailTime(e: OutboxEmail): { label: string; at?: ISODateString } {
  if (e.status === "sent") return { label: "Sent", at: e.sent_at ?? e.updated_at };
  if (e.status === "dead") return { label: "Dead-lettered", at: e.dead_at ?? e.updated_at };
  return { label: "Next attempt", at: e.next_attempt_at };
}

// ---- Retention ----

export interface RetentionAgeRule {
  /** Go duration ("0s" = no age limit). */
  max_age: string;
  max_age_seconds: number;
}

export interface RetentionAgeKeepRule extends RetentionAgeRule {
  /** Newest entries kept regardless of age limit (0 = no count limit). */
  keep: number;
}

export interface RetentionConfig {
  policy_versions: RetentionAgeKeepRule;
  audit: RetentionAgeRule;
  webhook_deliveries: RetentionAgeKeepRule;
  backups: RetentionAgeKeepRule;
}

/** One retention pass (POST /admin/retention/run, `last_report`). */
export interface RetentionReport {
  at: ISODateString;
  policy_versions_pruned: number;
  audit_pruned: number;
  webhook_deliveries_pruned: number;
  backups_pruned: number;
  took_ms: number;
}

/** GET /admin/retention. */
export interface RetentionStatus {
  /** Go duration between passes. */
  interval: string;
  running: boolean;
  config: RetentionConfig;
  last_run_at?: ISODateString;
  last_error?: string;
  last_report?: RetentionReport;
}

const noAge: RetentionAgeRule = { max_age: "0s", max_age_seconds: 0 };
const noAgeKeep: RetentionAgeKeepRule = { ...noAge, keep: 0 };

function normRetention(s: Partial<RetentionStatus> | null | undefined): RetentionStatus {
  const c: Partial<RetentionConfig> = s?.config ?? {};
  return {
    interval: s?.interval ?? "0s",
    running: s?.running ?? false,
    config: {
      policy_versions: { ...noAgeKeep, ...(c.policy_versions ?? {}) },
      audit: { ...noAge, ...(c.audit ?? {}) },
      webhook_deliveries: { ...noAgeKeep, ...(c.webhook_deliveries ?? {}) },
      backups: { ...noAgeKeep, ...(c.backups ?? {}) },
    },
    last_run_at: s?.last_run_at,
    last_error: s?.last_error,
    last_report: s?.last_report,
  };
}

/**
 * "Older than 2160h0m0s or beyond 100 per policy" / "Kept forever".
 * `per` names the unit the count applies to ("per policy"); omit it for
 * rules without a count.
 */
export function retentionRuleText(rule: RetentionAgeRule & { keep?: number }, per = ""): string {
  const parts: string[] = [];
  if (rule.max_age_seconds > 0) parts.push(`older than ${rule.max_age}`);
  if (rule.keep && rule.keep > 0) parts.push(`beyond ${rule.keep.toLocaleString("en")}${per ? ` ${per}` : ""}`);
  if (parts.length === 0) return "Kept forever";
  const text = parts.join(" or ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** "3 policy versions, 10 audit events, 0 webhook deliveries, 1 backup". */
export function retentionReportText(r: RetentionReport): string {
  const n = (count: number, one: string, many: string) => `${count.toLocaleString("en")} ${count === 1 ? one : many}`;
  return [
    n(r.policy_versions_pruned, "policy version", "policy versions"),
    n(r.audit_pruned, "audit event", "audit events"),
    n(r.webhook_deliveries_pruned, "webhook delivery", "webhook deliveries"),
    n(r.backups_pruned, "backup", "backups"),
  ].join(", ");
}

/** The route is missing on this server (501, or 404 from servers older than v0.8). */
export function isUnavailable(err: unknown): boolean {
  return isNotImplemented(err) || (isApiError(err) && err.status === 404);
}

// ---- Client ----

export function createServerOpsApi(client: ApiClient) {
  const outbox = `${V1}/admin/outbox`;
  const retention = `${V1}/admin/retention`;
  return {
    outbox: {
      list: async (q: OutboxQuery = {}): Promise<OutboxList> =>
        normOutbox(await client.request<OutboxList | null>(outbox, { query: { status: q.status, limit: q.limit } })),
      retry: (id: string) => client.request<OutboxEmail>(`${outbox}/${seg(id)}/retry`, { method: "POST" }),
      /** Requeues every dead-lettered email. */
      retryDead: async (): Promise<OutboxRetryAllResult> => {
        const r = await client.request<OutboxRetryAllResult | null>(`${outbox}/retry`, { method: "POST" });
        return { retried: r?.retried ?? 0 };
      },
      remove: (id: string) => client.request<void>(`${outbox}/${seg(id)}`, { method: "DELETE" }),
    },
    retention: {
      status: async (): Promise<RetentionStatus> => normRetention(await client.request<RetentionStatus | null>(retention)),
      run: (): Promise<RetentionReport> => client.request<RetentionReport>(`${retention}/run`, { method: "POST" }),
    },
  };
}

export type ServerOpsApi = ReturnType<typeof createServerOpsApi>;

export const serverOpsKeys = {
  outbox: ["admin", "outbox"] as const,
  outboxList: (q: OutboxQuery) => ["admin", "outbox", q.status ?? "all", q.limit ?? 0] as const,
  retention: ["admin", "retention"] as const,
};

export function useServerOpsApi(): ServerOpsApi {
  const { client } = useApi();
  return useMemo(() => createServerOpsApi(client), [client]);
}

/** Outbox poll interval while emails are pending. */
export const OUTBOX_POLL_MS = 10_000;

export function useOutbox(q: OutboxQuery, enabled = true) {
  const api = useServerOpsApi();
  return useQuery({
    queryKey: serverOpsKeys.outboxList(q),
    queryFn: () => api.outbox.list(q),
    enabled,
    retry: retryUnlessFinal,
    // Keep the previous filter's rows (and counts) while the next loads.
    placeholderData: keepPreviousData,
    refetchInterval: (query) => (query.state.data?.counts.pending ? OUTBOX_POLL_MS : false),
  });
}

export function useRetention(enabled = true) {
  const api = useServerOpsApi();
  return useQuery({
    queryKey: serverOpsKeys.retention,
    queryFn: api.retention.status,
    enabled,
    retry: retryUnlessFinal,
    // Poll while a pass runs.
    refetchInterval: (query) => (query.state.data?.running ? 3000 : false),
  });
}
