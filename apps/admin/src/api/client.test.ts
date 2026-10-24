import { describe, expect, it, vi } from "vitest";
import { apiError, json, mockFetch, noContent } from "@/test/mockFetch";
import { ApiClient, ApiError, buildQuery, encodePath, parseErrorResponse } from "./client";
import { createPersistentValue } from "./storage";

function setup(routes: Parameters<typeof mockFetch>[0], token: string | null = "tok-123") {
  const fm = mockFetch(routes);
  const tokenStore = createPersistentValue("test.token");
  tokenStore.set(token);
  const client = new ApiClient({ tokenStore, fetch: fm.fetch });
  return { fm, client, tokenStore };
}

describe("ApiClient", () => {
  it("sends the bearer token from the store", async () => {
    const { fm, client } = setup({ "GET /api/v1/auth/me": { ok: true } });
    await client.request("/api/v1/auth/me");
    expect(fm.calls[0]?.headers.get("Authorization")).toBe("Bearer tok-123");
    expect(fm.calls[0]?.headers.get("Accept")).toBe("application/json");
  });

  it("omits the Authorization header when signed out", async () => {
    const { fm, client } = setup({ "GET /healthz": { status: "ok" } }, null);
    await client.request("/healthz");
    expect(fm.calls[0]?.headers.has("Authorization")).toBe(false);
  });

  it("serialises JSON bodies with a content type", async () => {
    const { fm, client } = setup({ "POST /api/v1/tenants": (req) => json(req.body, 201) });
    const res = await client.request<{ slug: string }>("/api/v1/tenants", { method: "POST", body: { slug: "acme" } });
    expect(res).toEqual({ slug: "acme" });
    expect(fm.calls[0]?.headers.get("Content-Type")).toBe("application/json");
    expect(fm.calls[0]?.body).toEqual({ slug: "acme" });
  });

  it("parses the error envelope into an ApiError", async () => {
    const { client } = setup({ "GET /api/v1/tenants/x": apiError(404, "not_found", "tenant x not found") });
    const err = await client.request("/api/v1/tenants/x").catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toMatchObject({ status: 404, code: "not_found", message: "tenant x not found" });
  });

  it("keeps extra envelope fields as details", async () => {
    const { client } = setup({
      "POST /m": apiError(403, "forbidden", "push denied", { kind: "review_required" }),
    });
    const err = (await client.request("/m", { method: "POST" }).catch((e: unknown) => e)) as ApiError;
    expect(err.details).toEqual({ kind: "review_required" });
  });

  it("falls back to the status code for non-JSON errors", async () => {
    const { client } = setup({ "GET /x": new Response("upstream exploded", { status: 502, statusText: "Bad Gateway" }) });
    const err = (await client.request("/x").catch((e: unknown) => e)) as ApiError;
    expect(err.code).toBe("internal");
    expect(err.status).toBe(502);
    expect(err.message).toBe("upstream exploded");
  });

  it("clears the token and notifies listeners on 401", async () => {
    const { client, tokenStore } = setup({ "GET /api/v1/auth/me": apiError(401, "unauthenticated", "token expired") });
    const listener = vi.fn();
    client.onUnauthorized(listener);
    await expect(client.request("/api/v1/auth/me")).rejects.toMatchObject({ code: "unauthenticated" });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(tokenStore.get()).toBeNull();
  });

  it("does not treat a failed login as an expired session", async () => {
    const { client, tokenStore } = setup({ "POST /api/v1/auth/login": apiError(401, "unauthenticated", "bad credentials") }, "old");
    const listener = vi.fn();
    client.onUnauthorized(listener);
    await expect(client.request("/api/v1/auth/login", { method: "POST", body: {}, skipAuthRedirect: true })).rejects.toBeInstanceOf(ApiError);
    expect(listener).not.toHaveBeenCalled();
    expect(tokenStore.get()).toBe("old");
  });

  it("returns undefined for 204 responses", async () => {
    const { client } = setup({ "DELETE /api/v1/auth/tokens/t1": noContent() });
    await expect(client.request("/api/v1/auth/tokens/t1", { method: "DELETE" })).resolves.toBeUndefined();
  });

  it("maps network failures to a 'network' error", async () => {
    const tokenStore = createPersistentValue("t");
    const client = new ApiClient({ tokenStore, fetch: () => Promise.reject(new TypeError("Failed to fetch")) });
    await expect(client.request("/healthz")).rejects.toMatchObject({ code: "network", status: 0 });
  });

  it("prefixes the configured base URL", async () => {
    const fetchFn = vi.fn((_input: RequestInfo | URL) => Promise.resolve(json({})));
    const client = new ApiClient({ baseUrl: "http://wt.local:7420/", tokenStore: createPersistentValue("t"), fetch: fetchFn });
    await client.request("/healthz");
    expect(fetchFn.mock.calls[0]?.[0]).toBe("http://wt.local:7420/healthz");
  });
});

describe("helpers", () => {
  it("buildQuery skips empty values", () => {
    expect(buildQuery({ a: "1", b: "", c: undefined, d: null, e: 0 })).toBe("?a=1&e=0");
    expect(buildQuery({})).toBe("");
  });

  it("encodePath encodes each segment but keeps slashes", () => {
    expect(encodePath("src/my file#1.ts")).toBe("src/my%20file%231.ts");
    expect(encodePath("/a//b/")).toBe("a/b");
  });

  it("parseErrorResponse tolerates an envelope without a message", async () => {
    const err = await parseErrorResponse(new Response(JSON.stringify({ error: { code: "conflict" } }), { status: 409, statusText: "Conflict" }));
    expect(err).toMatchObject({ code: "conflict", message: "Conflict" });
  });
});

describe("createPersistentValue", () => {
  it("falls back to memory when storage throws", () => {
    const broken = {
      getItem: () => {
        throw new Error("SecurityError");
      },
      setItem: () => {
        throw new Error("QuotaExceeded");
      },
      removeItem: () => {
        throw new Error("SecurityError");
      },
    } as unknown as Storage;
    const v = createPersistentValue("k", broken);
    expect(v.get()).toBeNull();
    v.set("abc");
    expect(v.get()).toBe("abc");
    v.set(null);
    expect(v.get()).toBeNull();
  });

  it("persists to localStorage", () => {
    createPersistentValue("persist.k").set("v1");
    expect(localStorage.getItem("persist.k")).toBe("v1");
    expect(createPersistentValue("persist.k").get()).toBe("v1");
  });
});
