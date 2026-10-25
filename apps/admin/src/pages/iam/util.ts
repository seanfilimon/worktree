import { ROLES, type Role } from "@/api/types";
import type { PolicyStatement, PolicyValidationError, SubjectType } from "@/api/iam";

export const SUBJECT_LABEL: Record<SubjectType, string> = { user: "User", team: "Team", service_account: "Service account" };

export const BUILTIN_ROLES: readonly string[] = ROLES;

export function isBuiltinRole(role: string): role is Role {
  return BUILTIN_ROLES.includes(role);
}

/** Custom role names: 2-40 of a-z, 0-9 and '-'. */
export const ROLE_NAME_RE = /^[a-z0-9][a-z0-9-]{1,39}$/;
export const ROLE_NAME_HINT = "2–40 lowercase letters, digits or dashes, starting with a letter or digit.";

/** Team slugs and service-account names: [a-z0-9-], no leading or trailing dash. */
export const IAM_SLUG_RE = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;
export const IAM_SLUG_HINT = "Lowercase letters, digits or dashes; no leading or trailing dash.";

export const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export const WEEKDAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] as const;

export const RESOURCE_EXAMPLES = (tenant: string) => [
  `wt:tenant/${tenant}`,
  `wt:tree/${tenant}/*`,
  `wt:branch/${tenant}/*/main`,
  `wt:path/${tenant}/*/secrets/**`,
  "*",
];

export const PRINCIPAL_EXAMPLES = ["user:alice", "team:platform", "role:developer", "sa:ci-bot", "authenticated", "anonymous", "*"];

export const SCOPE_EXAMPLES = ["tree:read", "tree:write", "tree:*", "mr:*", "*"];

export function emptyStatement(): PolicyStatement {
  return { effect: "allow", actions: [], resources: [], principals: [], conditions: {} };
}

/** Keeps statements in the canonical shape the server expects (drops empty conditions). */
export function cleanStatement(s: PolicyStatement): PolicyStatement {
  const c = s.conditions;
  const conditions: PolicyStatement["conditions"] = {};
  if (c.ip_ranges?.length) conditions.ip_ranges = c.ip_ranges;
  if (c.require_mfa) conditions.require_mfa = true;
  if (c.not_before) conditions.not_before = c.not_before;
  if (c.not_after) conditions.not_after = c.not_after;
  if (c.time_window) conditions.time_window = c.time_window;
  const out: PolicyStatement = {
    effect: s.effect,
    actions: s.actions,
    resources: s.resources,
    principals: s.principals,
    conditions,
  };
  return s.sid ? { sid: s.sid, ...out } : out;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

export type ParsedStatements = { ok: true; statements: PolicyStatement[] } | { ok: false; error: string };

/**
 * Parses the JSON editor's text: an array of statements or an object with a
 * `statements` array. Structural problems are left to the server validator;
 * only the shape needed by the visual builder is checked here.
 */
export function parseStatementsJson(text: string): ParsedStatements {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (err) {
    return { ok: false, error: `Invalid JSON: ${err instanceof Error ? err.message : String(err)}` };
  }
  const list = isRecord(raw) && Array.isArray(raw.statements) ? raw.statements : raw;
  if (!Array.isArray(list)) return { ok: false, error: "Expected an array of statements." };
  const statements: PolicyStatement[] = [];
  for (const [i, s] of list.entries()) {
    if (!isRecord(s)) return { ok: false, error: `Statement ${i + 1} must be an object.` };
    const cond = isRecord(s.conditions) ? s.conditions : {};
    const tw = isRecord(cond.time_window) ? cond.time_window : undefined;
    statements.push({
      ...(typeof s.sid === "string" && s.sid ? { sid: s.sid } : {}),
      effect: s.effect === "deny" ? "deny" : "allow",
      actions: strings(s.actions),
      resources: strings(s.resources),
      principals: strings(s.principals),
      conditions: {
        ...(Array.isArray(cond.ip_ranges) ? { ip_ranges: strings(cond.ip_ranges) } : {}),
        ...(cond.require_mfa === true ? { require_mfa: true } : {}),
        ...(typeof cond.not_before === "string" ? { not_before: cond.not_before } : {}),
        ...(typeof cond.not_after === "string" ? { not_after: cond.not_after } : {}),
        ...(tw
          ? {
              time_window: {
                days: strings(tw.days),
                start: typeof tw.start === "string" ? tw.start : "",
                end: typeof tw.end === "string" ? tw.end : "",
                timezone: typeof tw.timezone === "string" ? tw.timezone : "UTC",
              },
            }
          : {}),
      },
    });
  }
  return { ok: true, statements };
}

/** Validation errors of one statement field ("conditions.ip_ranges" matches "conditions" and "conditions.ip_ranges"). */
export function fieldErrors(errors: readonly PolicyValidationError[], statement: number, field: string): string[] {
  return errors
    .filter((e) => e.statement === statement && (e.field === field || e.field.startsWith(`${field}.`) || e.field.startsWith(`${field}[`)))
    .map((e) => e.message);
}

const KNOWN_FIELDS = ["sid", "effect", "actions", "resources", "principals", "conditions"];

/** Errors not attached to a known field of an existing statement. */
export function otherErrors(errors: readonly PolicyValidationError[], count: number): PolicyValidationError[] {
  return errors.filter(
    (e) => !(e.statement >= 0 && e.statement < count && KNOWN_FIELDS.some((f) => e.field === f || e.field.startsWith(`${f}.`) || e.field.startsWith(`${f}[`))),
  );
}

const pad = (n: number) => String(n).padStart(2, "0");

/** RFC 3339 → value for <input type="datetime-local"> in local time. */
export function toLocalInput(iso: string | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** <input type="datetime-local"> value → RFC 3339 (UTC); "" → undefined. */
export function fromLocalInput(v: string): string | undefined {
  if (!v) return undefined;
  const d = new Date(v);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString().replace(/\.\d{3}Z$/, "Z");
}

/** Accept-invitation link served by the admin panel. */
export function inviteLink(code: string, origin: string = window.location.origin): string {
  return `${origin}/admin/invite/${encodeURIComponent(code)}`;
}

/** Humanises a snake_case source / effect ("tree_grant" → "Tree grant"). */
export function humanize(s: string): string {
  const t = s.replace(/_/g, " ");
  return t.charAt(0).toUpperCase() + t.slice(1);
}
