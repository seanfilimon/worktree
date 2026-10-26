/**
 * v0.7 notifications (docs/ENGINEERING_PLAN.md §15.3, workstream
 * `notifications`): the caller's inbox, notification settings and watch
 * levels. Field names mirror services/server-go/internal/domain/v07.go and
 * internal/notify. Go may encode empty lists as `null`; every function here
 * normalises them to `[]`.
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { encodePath, type ApiClient } from "@/api/client";
import { useApi } from "@/api/context";
import type { ISODateString } from "@/api/types";

const BASE = "/api/v1/notifications";
const seg = encodeURIComponent;

export const REASONS = [
  "review_requested",
  "mentioned",
  "ci_failed",
  "todo_assigned",
  "mr_merged",
  "mr_commented",
  "release",
  "watching",
] as const;
export type Reason = (typeof REASONS)[number];

export const REASON_LABEL: Record<Reason, string> = {
  review_requested: "Review requested",
  mentioned: "Mentioned",
  ci_failed: "CI failed",
  todo_assigned: "TODO assigned",
  mr_merged: "Merged",
  mr_commented: "Activity on your MR",
  release: "Release",
  watching: "Watching",
};

export function reasonLabel(reason: string): string {
  return (REASON_LABEL as Record<string, string>)[reason] ?? reason.replace(/_/g, " ");
}

/** One inbox item (domain.Notification). */
export interface Notification {
  id: string;
  account_id: string;
  reason: string;
  /** Bus event type that caused it. */
  event: string;
  tenant_id: string;
  tree_id?: string;
  title: string;
  body?: string;
  /** Admin-panel path, e.g. /trees/acme/web/merge-requests/3. */
  url?: string;
  actor?: string;
  created_at: ISODateString;
  read_at?: ISODateString;
}

export interface NotificationList {
  notifications: Notification[];
  unread_count: number;
}

export const WATCH_LEVELS = ["participating", "watching", "ignoring"] as const;
export type WatchLevel = (typeof WATCH_LEVELS)[number];

export const WATCH_LABEL: Record<WatchLevel, string> = {
  participating: "Participating",
  watching: "Watching",
  ignoring: "Ignoring",
};

export const WATCH_DESCRIPTION: Record<WatchLevel, string> = {
  participating: "Only what involves you: review requests, mentions, your merge requests, CI failures on them, TODOs assigned to you.",
  watching: "Also every merge request event, CI failure and published release.",
  ignoring: "Never notify me.",
};

export function isWatchLevel(v: unknown): v is WatchLevel {
  return typeof v === "string" && (WATCH_LEVELS as readonly string[]).includes(v);
}

/** domain.WatchSetting; `tree_id` empty = the whole tenant. */
export interface WatchSetting {
  account_id: string;
  tenant_id: string;
  tree_id?: string;
  level: WatchLevel;
  email: boolean;
  updated_at: ISODateString;
}

/** A stored watch setting with its tenant slug and tree name. */
export interface WatchView extends WatchSetting {
  tenant: string;
  tree?: string;
}

export interface NotificationSettings {
  /** Email participating notifications. */
  email: boolean;
  /** The server has SMTP configured (read-only). */
  email_configured: boolean;
  updated_at?: ISODateString;
}

/** A tenant-wide (`tree` unset) or tree watch scope. */
export interface WatchScope {
  tenant: string;
  tree?: string;
}

function watchPath(s: WatchScope): string {
  return s.tree ? `/api/v1/trees/${encodePath(s.tenant)}/${encodePath(s.tree)}/watch` : `/api/v1/tenants/${encodePath(s.tenant)}/watch`;
}

function normList(v: Partial<NotificationList> | null | undefined): NotificationList {
  return { notifications: v?.notifications ?? [], unread_count: v?.unread_count ?? 0 };
}

export function createNotificationsApi(client: ApiClient) {
  return {
    list: async (opts: { unread?: boolean; limit?: number } = {}) =>
      normList(
        await client.request<NotificationList>(BASE, {
          query: { unread: opts.unread ? "true" : undefined, limit: opts.limit },
        }),
      ),
    read: (id: string) => client.request<Notification>(`${BASE}/${seg(id)}/read`, { method: "POST" }),
    readAll: () => client.request<{ updated: number }>(`${BASE}/read-all`, { method: "POST" }),
    remove: (id: string) => client.request<void>(`${BASE}/${seg(id)}`, { method: "DELETE" }),
    settings: () => client.request<NotificationSettings>(`${BASE}/settings`),
    putSettings: (body: { email?: boolean }) => client.request<NotificationSettings>(`${BASE}/settings`, { method: "PUT", body }),
    watches: async () => (await client.request<{ watches: WatchView[] | null }>(`${BASE}/watches`))?.watches ?? [],
    watch: (s: WatchScope) => client.request<WatchSetting>(watchPath(s)),
    putWatch: (s: WatchScope, body: { level: WatchLevel; email?: boolean }) =>
      client.request<WatchSetting>(watchPath(s), { method: "PUT", body }),
  };
}

export type NotificationsApi = ReturnType<typeof createNotificationsApi>;

/** Query keys; everything lives under ["notifications"] so one invalidation refreshes it all. */
export const notificationKeys = {
  all: ["notifications"] as const,
  list: (unread: boolean, limit: number) => ["notifications", "list", unread ? "unread" : "all", limit] as const,
  settings: ["notifications", "settings"] as const,
  watches: ["notifications", "watches"] as const,
  watch: (s: WatchScope) => ["notifications", "watch", s.tenant, s.tree ?? ""] as const,
};

export function useNotificationsApi(): NotificationsApi {
  const { client } = useApi();
  return useMemo(() => createNotificationsApi(client), [client]);
}

export function useNotifications(unread: boolean, limit: number, opts: { refetchInterval?: number } = {}) {
  const api = useNotificationsApi();
  return useQuery({
    queryKey: notificationKeys.list(unread, limit),
    queryFn: () => api.list({ unread, limit }),
    refetchInterval: opts.refetchInterval,
  });
}

export function useNotificationSettings() {
  const api = useNotificationsApi();
  return useQuery({ queryKey: notificationKeys.settings, queryFn: () => api.settings() });
}

export function useWatches() {
  const api = useNotificationsApi();
  return useQuery({ queryKey: notificationKeys.watches, queryFn: () => api.watches() });
}

export function useWatch(scope: WatchScope) {
  const api = useNotificationsApi();
  return useQuery({ queryKey: notificationKeys.watch(scope), queryFn: () => api.watch(scope) });
}

/** Mark-read / read-all / done mutations that refresh every notifications query. */
export function useInboxActions() {
  const api = useNotificationsApi();
  const queryClient = useQueryClient();
  const onSuccess = () => queryClient.invalidateQueries({ queryKey: notificationKeys.all });
  return {
    read: useMutation({ mutationFn: (id: string) => api.read(id), onSuccess }),
    readAll: useMutation({ mutationFn: () => api.readAll(), onSuccess }),
    remove: useMutation({ mutationFn: (id: string) => api.remove(id), onSuccess }),
  };
}

export function useSetWatch() {
  const api = useNotificationsApi();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ scope, level, email }: { scope: WatchScope; level: WatchLevel; email?: boolean }) => api.putWatch(scope, { level, email }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: notificationKeys.all }),
  });
}

/** "acme/web" from an admin path like /trees/acme/web/merge-requests/1. */
export function treeOfUrl(url: string | undefined): string | undefined {
  const m = /^\/trees\/([^/]+)\/([^/]+)/.exec(url ?? "");
  return m ? `${decodeURIComponent(m[1] ?? "")}/${decodeURIComponent(m[2] ?? "")}` : undefined;
}
