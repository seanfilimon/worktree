// v0.8 device-auth (§16.6): access tokens are renewed with the refresh token.
import { describe, expect, it, vi } from "vitest";
import { apiError, json, mockFetch } from "@/test/mockFetch";
import { ApiClient } from "./client";
import { createPersistentValue } from "./storage";

function setup(routes: Parameters<typeof mockFetch>[0]) {
  const fm = mockFetch(routes);
  const tokenStore = createPersistentValue("refresh-test.token");
  const refreshStore = createPersistentValue("refresh-test.refresh");
  const client = new ApiClient({ tokenStore, refreshStore, fetch: fm.fetch });
  client.setToken("tok-1");
  client.setRefreshToken("wtrt_1");
  return { fm, client, tokenStore, refreshStore };
}

/** /auth/me accepts only tok-2; /auth/refresh rotates wtrt_1 -> tok-2/wtrt_2. */
const routes = {
  "GET /api/v1/auth/me": (req: { headers: Headers }) =>
    req.headers.get("Authorization") === "Bearer tok-2"
      ? json({ ok: true })
      : apiError(401, "unauthenticated", "invalid or expired token"),
  "POST /api/v1/auth/refresh": (req: { body: unknown }) =>
    (req.body as { refresh_token?: string }).refresh_token === "wtrt_1"
      ? json({ token: "tok-2", token_type: "Bearer", expires_in: 3600, refresh_token: "wtrt_2" })
      : apiError(401, "invalid_grant", "invalid or expired refresh token"),
};

describe("ApiClient refresh tokens", () => {
  it("renews an expired access token once and retries", async () => {
    const { fm, client, refreshStore } = setup(routes);
    const expired = vi.fn();
    client.onUnauthorized(expired);
    await expect(client.request("/api/v1/auth/me")).resolves.toEqual({ ok: true });
    expect(client.token).toBe("tok-2");
    expect(refreshStore.get()).toBe("wtrt_2");
    const refresh = fm.find("POST /api/v1/auth/refresh");
    expect(refresh).toHaveLength(1);
    expect(refresh[0]?.headers.has("Authorization")).toBe(false);
    expect(refresh[0]?.body).toEqual({ refresh_token: "wtrt_1" });
    expect(expired).not.toHaveBeenCalled();
  });

  it("shares one refresh between concurrent requests", async () => {
    const { fm, client } = setup(routes);
    await Promise.all([client.request("/api/v1/auth/me"), client.request("/api/v1/auth/me"), client.request("/api/v1/auth/me")]);
    expect(fm.find("POST /api/v1/auth/refresh")).toHaveLength(1);
  });

  it("signs out when the refresh token is rejected", async () => {
    const { client, refreshStore } = setup(routes);
    client.setRefreshToken("wtrt_revoked");
    const expired = vi.fn();
    client.onUnauthorized(expired);
    await expect(client.request("/api/v1/auth/me")).rejects.toMatchObject({ status: 401 });
    expect(expired).toHaveBeenCalledTimes(1);
    expect(client.token).toBeNull();
    expect(refreshStore.get()).toBeNull();
  });

  it("never refreshes login calls and drops the refresh token with the session", async () => {
    const { fm, client, refreshStore } = setup({
      ...routes,
      "POST /api/v1/auth/login": apiError(401, "unauthenticated", "invalid username or password"),
    });
    await expect(
      client.request("/api/v1/auth/login", { method: "POST", body: {}, skipAuthRedirect: true }),
    ).rejects.toMatchObject({ status: 401 });
    expect(fm.find("POST /api/v1/auth/refresh")).toHaveLength(0);
    client.setToken("tok-sso");
    expect(refreshStore.get()).toBeNull();
  });

  it("keeps working without a refresh store (memory)", async () => {
    const fm = mockFetch(routes);
    const client = new ApiClient({ tokenStore: createPersistentValue("refresh-test.mem"), fetch: fm.fetch });
    client.setToken("tok-1");
    client.setRefreshToken("wtrt_1");
    await expect(client.request("/api/v1/auth/me")).resolves.toEqual({ ok: true });
  });
});
