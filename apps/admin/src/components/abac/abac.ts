/**
 * Pure helpers for ABAC attributes (§16.5): client-side validation mirroring
 * the server rules, editor row conversion, condition normalisation and text.
 */
import {
  ATTRIBUTE_OPS,
  ATTRIBUTE_SCOPES,
  type AttributeCondition,
  type AttributeOp,
  type AttributeOverrides,
  type Attributes,
  type AttributeScope,
} from "@/api/abac";

export const ATTRIBUTE_KEY_RE = /^[a-z][a-z0-9_.-]{0,63}$/;
export const ATTRIBUTE_KEY_HINT = "Lowercase letter first, then up to 63 of a-z, 0-9, _ . -";
export const MAX_ATTRIBUTES = 64;
export const MAX_VALUE_BYTES = 256;
export const MAX_CONDITIONS = 32;
export const MAX_CONDITION_VALUES = 64;

// C0, DEL and C1 control characters (Go's unicode.IsControl).
// eslint-disable-next-line no-control-regex
const CONTROL_RE = /[\u0000-\u001f\u007f-\u009f]/;

const encoder = new TextEncoder();

export function byteLength(s: string): number {
  return encoder.encode(s).length;
}

/** Error for an attribute key, or undefined when valid. */
export function validateAttributeKey(key: string): string | undefined {
  if (key === "") return "Enter a key.";
  if (!ATTRIBUTE_KEY_RE.test(key)) return `Invalid key. ${ATTRIBUTE_KEY_HINT}.`;
  return undefined;
}

/** Error for an attribute value, or undefined when valid. */
export function validateAttributeValue(value: string): string | undefined {
  if (value === "") return "Enter a value.";
  if (byteLength(value) > MAX_VALUE_BYTES) return `At most ${MAX_VALUE_BYTES} bytes.`;
  if (CONTROL_RE.test(value)) return "No control characters.";
  return undefined;
}

// ---- attribute editor rows ----

export interface AttributeRow {
  key: string;
  value: string;
}

export interface RowErrors {
  key?: string;
  value?: string;
}

export interface RowsValidation {
  rows: RowErrors[];
  /** Errors about the whole set (too many attributes). */
  general: string[];
  valid: boolean;
}

/** Map → rows sorted by key. */
export function rowsFromAttributes(attrs: Attributes): AttributeRow[] {
  return Object.keys(attrs)
    .sort()
    .map((key) => ({ key, value: attrs[key] ?? "" }));
}

/** Rows → map (keys trimmed). */
export function rowsToAttributes(rows: readonly AttributeRow[]): Attributes {
  const out: Attributes = {};
  for (const r of rows) out[r.key.trim()] = r.value;
  return out;
}

export function validateRows(rows: readonly AttributeRow[]): RowsValidation {
  const seen = new Map<string, number>();
  const errors = rows.map((r, i): RowErrors => {
    const key = r.key.trim();
    let keyError = validateAttributeKey(key);
    if (!keyError) {
      if (seen.has(key)) keyError = "Duplicate key.";
      else seen.set(key, i);
    }
    const valueError = validateAttributeValue(r.value);
    return { ...(keyError ? { key: keyError } : {}), ...(valueError ? { value: valueError } : {}) };
  });
  const general = rows.length > MAX_ATTRIBUTES ? [`At most ${MAX_ATTRIBUTES} attributes.`] : [];
  return { rows: errors, general, valid: general.length === 0 && errors.every((e) => !e.key && !e.value) };
}

/** Stable comparison of two attribute maps. */
export function sameAttributes(a: Attributes, b: Attributes): boolean {
  const ka = Object.keys(a).sort();
  const kb = Object.keys(b).sort();
  return ka.length === kb.length && ka.every((k, i) => k === kb[i] && a[k] === b[k]);
}

// ---- policy conditions ----

export type OpArity = "one" | "many" | "none";

export function opArity(op: string): OpArity {
  if (op === "exists" || op === "not_exists") return "none";
  if (op === "equals" || op === "not_equals") return "one";
  return "many";
}

export const OP_LABEL: Record<AttributeOp, string> = {
  equals: "equals",
  not_equals: "not equals",
  in: "in",
  not_in: "not in",
  starts_with: "starts with",
  exists: "exists",
  not_exists: "not exists",
};

export function isAttributeOp(op: string): op is AttributeOp {
  return (ATTRIBUTE_OPS as readonly string[]).includes(op);
}

export function isAttributeScope(s: string): s is AttributeScope {
  return (ATTRIBUTE_SCOPES as readonly string[]).includes(s);
}

/** "principal.department" → { scope: "principal", name: "department" }; unknown scope → scope "". */
export function splitConditionKey(key: string): { scope: AttributeScope | ""; name: string } {
  const i = key.indexOf(".");
  const head = i < 0 ? key : key.slice(0, i);
  if (!isAttributeScope(head)) return { scope: "", name: key };
  return { scope: head, name: i < 0 ? "" : key.slice(i + 1) };
}

export function joinConditionKey(scope: AttributeScope, name: string): string {
  return `${scope}.${name}`;
}

/** Error for a scoped key ("principal.<k>" etc.), or undefined. */
export function validateScopedKey(key: string): string | undefined {
  const { scope, name } = splitConditionKey(key);
  if (!scope) return "Key must start with principal., tree. or tenant.";
  return validateAttributeKey(name);
}

export interface ConditionErrors {
  key?: string;
  op?: string;
  values?: string;
}

export function validateCondition(c: AttributeCondition): ConditionErrors {
  const out: ConditionErrors = {};
  const keyError = validateScopedKey(c.key);
  if (keyError) out.key = keyError;
  if (!isAttributeOp(c.op)) {
    out.op = "Choose an operator.";
    return out;
  }
  const values = c.values ?? [];
  const arity = opArity(c.op);
  if (arity === "none") {
    if (values.length) out.values = "This operator takes no values.";
  } else if (arity === "one") {
    if (values.length !== 1 || values[0] === "") out.values = "Enter exactly one value.";
  } else if (values.length === 0) {
    out.values = "Add at least one value.";
  } else if (values.length > MAX_CONDITION_VALUES) {
    out.values = `At most ${MAX_CONDITION_VALUES} values.`;
  }
  if (!out.values) {
    for (const v of values) {
      const e = validateAttributeValue(v);
      if (e) {
        out.values = e;
        break;
      }
    }
  }
  return out;
}

export function hasConditionErrors(e: ConditionErrors): boolean {
  return Boolean(e.key || e.op || e.values);
}

/** Canonical wire shape: no `values` for exists / not_exists, one value for equals / not_equals. */
export function normalizeCondition(c: AttributeCondition): AttributeCondition {
  const arity = opArity(c.op);
  const values = c.values ?? [];
  if (arity === "none") return { key: c.key, op: c.op };
  if (arity === "one") return { key: c.key, op: c.op, values: values.slice(0, 1) };
  return { key: c.key, op: c.op, values: [...values] };
}

export function cleanAttributeConditions(list: readonly AttributeCondition[]): AttributeCondition[] {
  return list.map(normalizeCondition);
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** Parses `conditions.attributes` from the JSON editor; structural problems are left to the server. */
export function parseAttributeConditions(raw: unknown[]): AttributeCondition[] {
  return raw.map((item) => {
    const r = isRecord(item) ? item : {};
    return {
      key: typeof r.key === "string" ? r.key : "",
      op: typeof r.op === "string" ? r.op : "",
      ...(Array.isArray(r.values) ? { values: r.values.filter((x): x is string => typeof x === "string") } : {}),
    };
  });
}

/** Values changed when the operator changes. */
export function valuesForOp(op: string, values: readonly string[]): string[] | undefined {
  const arity = opArity(op);
  if (arity === "none") return undefined;
  if (arity === "one") return values.slice(0, 1);
  return [...values];
}

/** "principal.department in [eng, ops]", "tree.classification equals secret", "tenant.region exists". */
export function conditionText(c: { key: string; op: string; values?: readonly string[] | null }): string {
  const values = c.values ?? [];
  const arity = opArity(c.op);
  if (arity === "none") return `${c.key} ${c.op}`;
  if (arity === "one") return `${c.key} ${c.op} ${values[0] ?? ""}`.trimEnd();
  return `${c.key} ${c.op} [${values.join(", ")}]`;
}

// ---- simulator overrides ----

export interface OverrideRow {
  key: string;
  value: string;
  /** Simulate the attribute as unset (null). */
  unset: boolean;
}

export function validateOverrides(rows: readonly OverrideRow[]): RowErrors[] {
  const seen = new Set<string>();
  return rows.map((r) => {
    const key = r.key.trim();
    let keyError = validateScopedKey(key);
    if (!keyError) {
      if (seen.has(key)) keyError = "Duplicate key.";
      else seen.add(key);
    }
    const valueError = r.unset ? undefined : validateAttributeValue(r.value);
    return { ...(keyError ? { key: keyError } : {}), ...(valueError ? { value: valueError } : {}) };
  });
}

/** Override rows → `context.attributes`, or undefined when there are none. */
export function overridesToContext(rows: readonly OverrideRow[]): AttributeOverrides | undefined {
  if (rows.length === 0) return undefined;
  const out: AttributeOverrides = {};
  for (const r of rows) out[r.key.trim()] = r.unset ? null : r.value;
  return out;
}
