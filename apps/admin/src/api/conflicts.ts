/**
 * v0.8 conflict resolution in the browser (docs/ENGINEERING_PLAN.md §16.9,
 * workstream `conflict-ui`): the conflicts report of a merge request and
 * the resolve call that writes a merge-resolution snapshot on the source
 * branch.
 *
 * Go encodes empty slices as `null` and omits `omitempty` fields; the
 * functions here normalise lists to `[]` and sides to a full object.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ApiClient } from "./client";
import { useApi } from "./context";
import type { MergeRequest, Snapshot } from "./types";
import { retryV06 } from "./v06";

const V1 = "/api/v1";
const seg = encodeURIComponent;
const arr = <T>(v: T[] | null | undefined): T[] => (Array.isArray(v) ? v : []);

// ---- wire types ----

export type ConflictKind =
  | "content"
  | "modify_delete"
  | "add_add"
  | "binary"
  | "chunked"
  | "executable"
  | "file_directory"
  | (string & {});

/** One side (base / ours / theirs) of a conflicting path. */
export interface ConflictSide {
  exists: boolean;
  size: number;
  executable: boolean;
  hash: string;
  /** Omitted when the side does not exist, is binary, too large or restricted. */
  text?: string;
}

export interface ConflictRegion {
  start_line: number;
  ours_start: number;
  theirs_start: number;
  ours: string[];
  base: string[];
  theirs: string[];
}

export interface ConflictFile {
  path: string;
  kind: ConflictKind;
  binary: boolean;
  too_large: boolean;
  /** The caller may not read the path: texts are omitted. */
  restricted: boolean;
  base: ConflictSide;
  /** The SOURCE branch side (the resolution is written on the source branch). */
  ours: ConflictSide;
  /** The TARGET branch side. */
  theirs: ConflictSide;
  /** diff3 preview; present only when both ours and theirs are text. */
  markers?: string;
  regions: ConflictRegion[];
}

/** GET {tree}/merge-requests/{n}/conflicts. */
export interface ConflictReport {
  number: number;
  state: string;
  source_branch: string;
  target_branch: string;
  source_tip: string;
  target_tip: string;
  merge_base: string;
  resolvable: boolean;
  /** Why `resolvable` is false. */
  reason: string;
  /** Total conflicting paths. */
  total: number;
  /** `files` lists only the first conflicting paths. */
  truncated: boolean;
  files: ConflictFile[];
}

export type ResolveTake = "ours" | "theirs" | "base";

/** Exactly one of content / take / delete. */
export type FileResolution =
  | { path: string; content: string }
  | { path: string; take: ResolveTake }
  | { path: string; delete: true };

export interface ResolveRequest {
  files: FileResolution[];
  message?: string;
  source_tip?: string;
  target_tip?: string;
  allow_markers?: boolean;
}

export interface ResolveResult {
  merge_request: MergeRequest;
  snapshot: Snapshot;
  paths: string[];
}

// ---- helpers ----

function normSide(s: Partial<ConflictSide> | null | undefined): ConflictSide {
  return {
    exists: Boolean(s?.exists),
    size: s?.size ?? 0,
    executable: Boolean(s?.executable),
    hash: s?.hash ?? "",
    ...(typeof s?.text === "string" ? { text: s.text } : {}),
  };
}

function normRegion(r: ConflictRegion): ConflictRegion {
  return { ...r, ours: arr(r.ours), base: arr(r.base), theirs: arr(r.theirs) };
}

function normFile(f: ConflictFile): ConflictFile {
  return {
    ...f,
    kind: f.kind ?? "content",
    binary: Boolean(f.binary),
    too_large: Boolean(f.too_large),
    restricted: Boolean(f.restricted),
    base: normSide(f.base),
    ours: normSide(f.ours),
    theirs: normSide(f.theirs),
    regions: arr(f.regions).map(normRegion),
  };
}

export function normConflictReport(r: Partial<ConflictReport> | null | undefined): ConflictReport {
  const files = arr(r?.files).map(normFile);
  return {
    number: r?.number ?? 0,
    state: r?.state ?? "",
    source_branch: r?.source_branch ?? "",
    target_branch: r?.target_branch ?? "",
    source_tip: r?.source_tip ?? "",
    target_tip: r?.target_tip ?? "",
    merge_base: r?.merge_base ?? "",
    resolvable: Boolean(r?.resolvable),
    reason: r?.reason ?? "",
    total: r?.total ?? files.length,
    truncated: Boolean(r?.truncated),
    files,
  };
}

const MARKER = /^(<{7}|={7}|>{7}|\|{7})(?:\s|$)/;

/** True when a line is a conflict marker line (`<<<<<<<`, `=======`, `>>>>>>>`, `|||||||`). */
export function isMarkerLine(line: string): boolean {
  return MARKER.test(line);
}

/** True when the text still contains conflict marker lines. */
export function hasConflictMarkers(text: string): boolean {
  return text.split("\n").some((l) => isMarkerLine(l.replace(/\r$/, "")));
}

/** Character offsets of the lines starting a conflict block (`<<<<<<<`). */
export function markerOffsets(text: string): number[] {
  const out: number[] = [];
  let offset = 0;
  for (const line of text.split("\n")) {
    if (line.startsWith("<<<<<<<")) out.push(offset);
    offset += line.length + 1;
  }
  return out;
}

/** The file can be edited as text in the browser. */
export function isEditable(f: ConflictFile): boolean {
  if (f.binary || f.too_large || f.restricted) return false;
  return typeof f.markers === "string" || typeof f.ours.text === "string" || typeof f.theirs.text === "string";
}

// ---- endpoints ----

export function createConflictsApi(client: ApiClient) {
  const mrPath = (t: string, n: string, num: number) => `${V1}/trees/${seg(t)}/${seg(n)}/merge-requests/${num}`;
  return {
    get: async (t: string, n: string, num: number) =>
      normConflictReport(await client.request<ConflictReport>(`${mrPath(t, n, num)}/conflicts`)),
    resolve: async (t: string, n: string, num: number, body: ResolveRequest): Promise<ResolveResult> => {
      const r = await client.request<ResolveResult>(`${mrPath(t, n, num)}/resolve`, { method: "POST", body });
      return { ...r, paths: arr(r?.paths) };
    },
  };
}

export type ConflictsApi = ReturnType<typeof createConflictsApi>;

export const conflictKeys = {
  report: (t: string, n: string, num: number) => ["tree", t, n, "conflicts", num] as const,
};

export function useConflictsApi(): ConflictsApi {
  const { client } = useApi();
  return useMemo(() => createConflictsApi(client), [client]);
}

export function useConflicts(t: string, n: string, num: number, enabled = true) {
  const api = useConflictsApi();
  return useQuery({
    queryKey: conflictKeys.report(t, n, num),
    queryFn: () => api.get(t, n, num),
    enabled: enabled && num > 0,
    retry: retryV06,
    refetchOnWindowFocus: false,
  });
}
