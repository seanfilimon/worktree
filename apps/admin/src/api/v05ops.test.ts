import { describe, expect, it } from "vitest";
import { ApiClient, ApiError } from "./client";
import { createPersistentValue, TOKEN_STORAGE_KEY } from "./storage";
import {
  compactLimits,
  createOpsApi,
  decodeBase64Text,
  isNotImplemented,
  normalizePublicKey,
  parseAccessDocument,
  planItemText,
  retryUnlessFinal,
  tenantStatus,
} from "./v05ops";
import * as fx from "@/test/fixtures";
import { apiError, mockFetch } from "@/test/mockFetch";

function api(routes: Parameters<typeof mockFetch>[0]) {
  const fetchMock = mockFetch(routes);
  const client = new ApiClient({ tokenStore: createPersistentValue(TOKEN_STORAGE_KEY), fetch: fetchMock.fetch });
  return { ops: createOpsApi(client), fetchMock };
}

describe("v05ops helpers", () => {
  it("normalises Ed25519 public keys", () => {
    const bytes = new Uint8Array(32).map((_, i) => i * 7);
    const std = btoa(String.fromCharCode(...bytes));
    expect(normalizePublicKey(std)).toEqual({ key: std });
    const url = std.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    expect(normalizePublicKey(url)).toEqual({ key: std });
    const hex = [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
    expect(normalizePublicKey(hex)).toEqual({ key: std });
    expect(normalizePublicKey("")).toEqual({ error: "Paste the public key." });
    expect(normalizePublicKey("not base64!").error).toBe("Not valid base64 (or hex).");
    expect(normalizePublicKey(btoa("short")).error).toBe("An Ed25519 public key is 32 bytes; this is 5.");
  });

  it("parses canonical access documents", () => {
    expect(parseAccessDocument('{"roles":[],"policies":[]}')).toEqual({ content: { roles: [], policies: [] } });
    expect(parseAccessDocument("").error).toBe("Paste a canonical access document.");
    expect(parseAccessDocument("{").error).toMatch(/^Invalid JSON/);
    expect(parseAccessDocument("[]").error).toBe("The document must be a JSON object.");
    expect(parseAccessDocument('{"roles":{}}').error).toBe('"roles" must be an array.');
    expect(parseAccessDocument('{"x":1}').error).toBe('Expected "roles" and/or "policies".');
  });

  it("compacts limits, decodes base64 and formats plan items", () => {
    expect(compactLimits({ max_trees: 0, max_members: 5, max_storage_bytes: -1 })).toEqual({ max_members: 5 });
    expect(compactLimits(null)).toEqual({});
    expect(decodeBase64Text(btoa('{"a":1}'))).toBe('{"a":1}');
    expect(decodeBase64Text("%%%")).toBe("%%%");
    expect(planItemText("policy p: update")).toBe("policy p: update");
    expect(planItemText({ code: "E2001", message: "unknown action" })).toBe("E2001: unknown action");
    expect(planItemText({ op: "create" })).toBe('{"op":"create"}');
    expect(tenantStatus(undefined)).toBe("active");
    expect(tenantStatus({ ...fx.tenant, status: "" })).toBe("active");
    expect(tenantStatus({ ...fx.tenant, status: "suspended" })).toBe("suspended");
  });

  it("recognises 501 and does not retry final errors", () => {
    const nie = new ApiError(501, "not_implemented", "nope");
    expect(isNotImplemented(nie)).toBe(true);
    expect(isNotImplemented(new ApiError(500, "internal", "boom"))).toBe(false);
    expect(retryUnlessFinal(0, nie)).toBe(false);
    expect(retryUnlessFinal(0, new ApiError(404, "not_found", "x"))).toBe(false);
    expect(retryUnlessFinal(0, new ApiError(500, "internal", "x"))).toBe(true);
    expect(retryUnlessFinal(2, new ApiError(500, "internal", "x"))).toBe(false);
  });
});

describe("createOpsApi", () => {
  it("normalises null slices and parses the GC status", async () => {
    const { ops, fetchMock } = api({
      "GET /api/v1/admin/gc": { config: null, runs: null },
      "POST /api/v1/admin/gc": { dry_run: true },
    });
    expect(await ops.gc.status()).toEqual({ config: {}, runs: [] });
    await ops.gc.run(true);
    expect(fetchMock.find("POST /api/v1/admin/gc")[0]!.url.searchParams.get("dry_run")).toBe("true");
  });

  it("maps a 404 access document to null but rethrows other errors", async () => {
    const { ops } = api({ "GET /api/v1/tenants/b/access-config": apiError(501, "not_implemented", "nope") });
    expect(await ops.accessConfig.get("a")).toBeNull();
    await expect(ops.accessConfig.get("b")).rejects.toMatchObject({ status: 501 });
  });

  it("normalises plan results and usage", async () => {
    const { ops } = api({
      "POST /api/v1/tenants/a/access-config/plan": { accepted: true, errors: null, changes: null },
      "GET /api/v1/tenants/a/usage": { trees: 1, branches: 2, members: 3, storage_bytes: 4, limits: null, effective: { max_trees: 0 } },
    });
    expect(await ops.accessConfig.plan("a", {})).toEqual({ accepted: true, errors: [], changes: [] });
    expect(await ops.tenants.usage("a")).toEqual({ trees: 1, branches: 2, members: 3, storage_bytes: 4, limits: {}, effective: {} });
  });
});
