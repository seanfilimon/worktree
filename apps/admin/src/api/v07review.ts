/**
 * v0.7 deeper review (docs/ENGINEERING_PLAN.md §15.7, workstream `review-v07`):
 * path owners, review requests, draft merge requests, suggested changes and
 * the linked-group merge method.
 *
 * Field names mirror services/server-go/internal/domain/types.go (`v0.7
 * review` block). Go encodes empty slices as `null` and omits `omitempty`
 * fields; every function here normalises lists to `[]`.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { isApiError, type ApiClient } from "./client";
import { useApi } from "./context";
import type { CreateMergeRequestRequest, ISODateString, Snapshot } from "./types";
import type { BranchProtectionV05, CreateCommentRequest, MergeRequestV05, ReviewComment } from "./v05tree";
import type { MergeMethod } from "./v06";

const V1 = "/api/v1";
const seg = encodeURIComponent;
const arr = <T>(v: T[] | null | undefined): T[] => (Array.isArray(v) ? v : []);

// ---- wire types ----

/** domain.MergeRequest with the v0.7 fields. */
export type MergeRequestV07 = MergeRequestV05 & {
  /** Omitted when false. */
  draft?: boolean;
  /** Requested reviewers, canonical `user:<name>` / `team:<slug>`. */
  reviewers?: string[] | null;
};

/** POST {tree}/merge-requests with the v0.7 options. */
export interface CreateMergeRequestV07 extends CreateMergeRequestRequest {
  draft?: boolean;
  reviewers?: string[];
}

/** domain.BranchProtection with the v0.7 field. */
export interface BranchProtectionV07 extends BranchProtectionV05 {
  /** Every changed path needs an approval of the current source tip from one of its owners. */
  require_owner_approval?: boolean;
}

/** A suggested replacement of lines start..end (1-based, inclusive) of `path` as of `snapshot_id`. */
export interface Suggestion {
  path: string;
  start_line: number;
  end_line: number;
  /** Replacement text; "" deletes the lines. */
  replacement: string;
  snapshot_id: string;
  /** Set once applied. */
  applied_snapshot?: string;
  applied_by?: string;
  applied_at?: ISODateString;
}

export type ReviewCommentV07 = ReviewComment & { suggestion?: Suggestion };

export interface SuggestionRequest {
  path: string;
  start_line: number;
  end_line: number;
  replacement: string;
  /** Defaults to the source tip on the server. */
  snapshot_id?: string;
}

export interface CreateCommentRequestV07 extends CreateCommentRequest {
  suggestion?: SuggestionRequest;
}

export interface ApplySuggestionResult {
  comment: ReviewCommentV07;
  snapshot: Snapshot;
}

export interface OwnersRule {
  /** "**" glob anchored at the tree root. */
  pattern: string;
  /** Canonical owners; empty un-owns matching paths. */
  owners: string[];
}

export type OwnersSource = "config" | "file" | "none";

/** GET {tree}/owners. */
export interface OwnersView {
  tree_id: string;
  rules: OwnersRule[];
  updated_by?: string;
  updated_at?: ISODateString;
  source: OwnersSource;
  /** OWNERS file lines that could not be parsed. */
  errors: string[];
}

export interface MROwnersPath {
  path: string;
  owners: string[];
  approved: boolean;
  approved_by: string[];
}

/** GET {tree}/merge-requests/{n}/owners. */
export interface MROwners {
  source: OwnersSource;
  /** The target's protection has require_owner_approval. */
  required: boolean;
  satisfied: boolean;
  paths: MROwnersPath[];
  missing: string[];
}

// ---- helpers ----

export function asV07(mr: { draft?: boolean } | MergeRequestV05): MergeRequestV07 {
  return mr as MergeRequestV07;
}

export function isDraft(mr: unknown): boolean {
  return !!(mr && typeof mr === "object" && (mr as { draft?: unknown }).draft === true);
}

export function reviewersOf(mr: unknown): string[] {
  return arr((mr as MergeRequestV07 | null)?.reviewers);
}

/** `409 suggestion_outdated`: the source tip changed the suggested lines. */
export function isSuggestionOutdated(err: unknown): boolean {
  return isApiError(err) && err.code === "suggestion_outdated";
}

export interface Principal {
  kind: "user" | "team";
  name: string;
}

/** Parses a canonical (or shorthand) owner / reviewer identifier. */
export function parsePrincipal(raw: string): Principal {
  const s = raw.trim();
  if (s.startsWith("team:")) return { kind: "team", name: s.slice(5) };
  if (s.startsWith("user:")) return { kind: "user", name: s.slice(5) };
  if (s.startsWith("@") && s.includes("/")) return { kind: "team", name: s.slice(s.indexOf("/") + 1) };
  return { kind: "user", name: s.replace(/^@/, "") };
}

/** Splits free text ("@alice, team:core bob") into identifier tokens. */
export function splitPrincipals(text: string): string[] {
  return [...new Set(text.split(/[\s,]+/).map((t) => t.trim()).filter(Boolean))];
}

function normOwners(v: Partial<OwnersView> | null | undefined): OwnersView {
  const source = v?.source === "config" || v?.source === "file" ? v.source : "none";
  return {
    tree_id: v?.tree_id ?? "",
    rules: arr(v?.rules).map((r) => ({ pattern: r.pattern ?? "", owners: arr(r.owners) })),
    updated_by: v?.updated_by || undefined,
    updated_at: v?.updated_at || undefined,
    source,
    errors: arr(v?.errors),
  };
}

function normMROwners(v: Partial<MROwners> | null | undefined): MROwners {
  const source = v?.source === "config" || v?.source === "file" ? v.source : "none";
  const missing = arr(v?.missing);
  return {
    source,
    required: Boolean(v?.required),
    satisfied: v?.satisfied ?? missing.length === 0,
    paths: arr(v?.paths).map((p) => ({ path: p.path, owners: arr(p.owners), approved: Boolean(p.approved), approved_by: arr(p.approved_by) })),
    missing,
  };
}

// ---- endpoints ----

export function createV07ReviewApi(client: ApiClient) {
  const tree = (t: string, n: string) => `${V1}/trees/${seg(t)}/${seg(n)}`;
  const mr = (t: string, n: string, num: number) => `${tree(t, n)}/merge-requests/${num}`;

  return {
    mergeRequests: {
      create: (t: string, n: string, body: CreateMergeRequestV07) =>
        client.request<MergeRequestV07>(`${tree(t, n)}/merge-requests`, { method: "POST", body }),
      ready: (t: string, n: string, num: number) => client.request<MergeRequestV07>(`${mr(t, n, num)}/ready`, { method: "POST" }),
      addReviewers: (t: string, n: string, num: number, reviewers: string[]) =>
        client.request<MergeRequestV07>(`${mr(t, n, num)}/reviewers`, { method: "POST", body: { reviewers } }),
      /** DELETE .../reviewers?reviewer=a&reviewer=b (the same list is also sent as the JSON body). */
      removeReviewers: (t: string, n: string, num: number, reviewers: string[]) =>
        client.request<MergeRequestV07>(`${mr(t, n, num)}/reviewers?${reviewers.map((r) => `reviewer=${seg(r)}`).join("&")}`, {
          method: "DELETE",
          body: { reviewers },
        }),
      owners: async (t: string, n: string, num: number) => normMROwners(await client.request<MROwners>(`${mr(t, n, num)}/owners`)),
    },

    comments: {
      add: (t: string, n: string, num: number, body: CreateCommentRequestV07) =>
        client.request<ReviewCommentV07>(`${mr(t, n, num)}/comments`, { method: "POST", body }),
      apply: (t: string, n: string, num: number, id: string, message?: string) =>
        client.request<ApplySuggestionResult>(`${mr(t, n, num)}/comments/${seg(id)}/apply`, {
          method: "POST",
          body: message?.trim() ? { message: message.trim() } : undefined,
        }),
    },

    owners: {
      get: async (t: string, n: string) => normOwners(await client.request<OwnersView>(`${tree(t, n)}/owners`)),
      /** An empty list removes the server configuration (the OWNERS file applies again). */
      put: async (t: string, n: string, rules: OwnersRule[]) =>
        normOwners(await client.request<OwnersView>(`${tree(t, n)}/owners`, { method: "PUT", body: { rules } })),
    },

    linkedGroups: {
      /** POST .../merge; without a method each member uses its default. */
      merge: (tenant: string, id: string, method?: MergeMethod) =>
        client.request<unknown>(`${V1}/tenants/${seg(tenant)}/linked-groups/${seg(id)}/merge`, {
          method: "POST",
          body: method ? { method } : undefined,
        }),
    },
  };
}

export type V07ReviewApi = ReturnType<typeof createV07ReviewApi>;

// ---- query keys + hooks ----

/**
 * Keys live under ["tree", tenant, tree, ...]. The MR owners key nests under
 * "mr-checks" so every event that refreshes the merge checks (approvals,
 * pushes, reviews) refreshes the owners panel too.
 */
export const v07Keys = {
  owners: (t: string, n: string) => ["tree", t, n, "owners"] as const,
  mrOwners: (t: string, n: string, num: number) => ["tree", t, n, "mr-checks", num, "owners"] as const,
};

export function useV07ReviewApi(): V07ReviewApi {
  const { client } = useApi();
  return useMemo(() => createV07ReviewApi(client), [client]);
}

export function useTreeOwners(t: string, n: string) {
  const api = useV07ReviewApi();
  return useQuery({ queryKey: v07Keys.owners(t, n), queryFn: () => api.owners.get(t, n) });
}

export function useMrOwners(t: string, n: string, num: number, enabled = true) {
  const api = useV07ReviewApi();
  return useQuery({ queryKey: v07Keys.mrOwners(t, n, num), queryFn: () => api.mergeRequests.owners(t, n, num), enabled });
}
