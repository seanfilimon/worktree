/**
 * v0.8 ABAC attributes (docs/ENGINEERING_PLAN.md §16.5, workstream `abac`):
 * key/value attributes on tenants, accounts and trees, attribute conditions
 * in policy statements and the simulator's attribute explanation.
 *
 * Field names mirror services/server-go/internal/httpapi/attributes.go and
 * authz/attributes.go. Go encodes empty maps / slices as `null`; every
 * function here normalises them.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ApiClient } from "./client";
import { useApi } from "./context";

// ---- wire types ----

/** Attributes of one object: keys `[a-z][a-z0-9_.-]{0,63}`, values 1..256 bytes. */
export type Attributes = Record<string, string>;

export interface AttributesResponse {
  attributes: Attributes | null;
}

export const ATTRIBUTE_SCOPES = ["principal", "tree", "tenant"] as const;
export type AttributeScope = (typeof ATTRIBUTE_SCOPES)[number];

export const ATTRIBUTE_OPS = ["equals", "not_equals", "in", "not_in", "starts_with", "exists", "not_exists"] as const;
export type AttributeOp = (typeof ATTRIBUTE_OPS)[number];

/** One attribute condition of a policy statement (`conditions.attributes[]`). */
export interface AttributeCondition {
  /** "principal.<k>" | "tree.<k>" | "tenant.<k>". */
  key: string;
  op: AttributeOp | (string & {});
  /** Omitted for exists / not_exists. */
  values?: string[];
}

/** Simulator `context.attributes`: scoped key → override value (null = unset). */
export type AttributeOverrides = Record<string, string | null>;

/** What the simulated evaluation saw, after overrides. */
export interface SimulatedAttributes {
  principal: Attributes;
  tree: Attributes;
  tenant: Attributes;
}

/** One attribute condition of a statement whose principal / action / resource matched. */
export interface AttributeCheck {
  /** Policy name. */
  policy: string;
  policy_id: string;
  /** Sid, or "#<n>" (1-based position). */
  statement: string;
  effect: "allow" | "deny" | (string & {});
  key: string;
  op: string;
  values: string[] | null;
  /** The attribute's value; null when unset. */
  value: string | null;
  matched: boolean;
}

/** Whose attributes an editor reads and writes. */
export type AttributeTarget =
  | { kind: "tenant"; tenant: string }
  | { kind: "account"; username: string }
  | { kind: "tree"; tenant: string; tree: string }
  | { kind: "me" };

// ---- endpoints ----

const V1 = "/api/v1";
const seg = encodeURIComponent;

function attrPath(target: AttributeTarget): string {
  switch (target.kind) {
    case "tenant":
      return `${V1}/tenants/${seg(target.tenant)}/attributes`;
    case "account":
      return `${V1}/admin/accounts/${seg(target.username)}/attributes`;
    case "tree":
      return `${V1}/trees/${seg(target.tenant)}/${seg(target.tree)}/attributes`;
    case "me":
      return `${V1}/me/attributes`;
  }
}

const norm = (r: AttributesResponse | null | undefined): Attributes => ({ ...(r?.attributes ?? {}) });

/** Attribute endpoints bound to a client. */
export function createAbacApi(client: ApiClient) {
  return {
    get: async (target: AttributeTarget) => norm(await client.request<AttributesResponse>(attrPath(target))),
    /** Full replace; `{}` clears. `me` is read-only. */
    put: async (target: AttributeTarget, attributes: Attributes) =>
      norm(await client.request<AttributesResponse>(attrPath(target), { method: "PUT", body: { attributes } })),
  };
}

export type AbacApi = ReturnType<typeof createAbacApi>;

// ---- query keys + hooks ----

export const abacKeys = {
  attributes: (target: AttributeTarget): readonly unknown[] => {
    switch (target.kind) {
      case "tenant":
        return ["tenant", target.tenant, "attributes"];
      case "account":
        return ["accounts", target.username, "attributes"];
      case "tree":
        return ["tree", target.tenant, target.tree, "attributes"];
      case "me":
        return ["me", "attributes"];
    }
  },
};

export function useAbacApi(): AbacApi {
  const { client } = useApi();
  return useMemo(() => createAbacApi(client), [client]);
}

export function useAttributes(target: AttributeTarget, enabled = true) {
  const abac = useAbacApi();
  return useQuery({ queryKey: abacKeys.attributes(target), queryFn: () => abac.get(target), enabled });
}
