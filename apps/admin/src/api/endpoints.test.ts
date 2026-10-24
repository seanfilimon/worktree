import { describe, expect, it } from "vitest";
import { json, mockFetch, noContent } from "@/test/mockFetch";
import { ApiClient } from "./client";
import { createApi } from "./endpoints";
import { createPersistentValue } from "./storage";

function setup(routes: Parameters<typeof mockFetch>[0]) {
  const fm = mockFetch(routes);
  const api = createApi(new ApiClient({ tokenStore: createPersistentValue("e.token"), fetch: fm.fetch }));
  return { fm, api };
}

describe("endpoints", () => {
  it("normalises null lists to empty arrays", async () => {
    const { api } = setup({ "GET /api/v1/trees/acme/web/branches": json(null) });
    await expect(api.trees.branches("acme", "web")).resolves.toEqual([]);
  });

  it("encodes tenant and tree names in paths", async () => {
    const { fm, api } = setup({ "GET /api/v1/trees/my org/a b": {} });
    await api.trees.get("my org", "a b");
    expect(fm.calls[0]?.url.pathname).toBe("/api/v1/trees/my%20org/a%20b");
  });

  it("passes the snapshot query parameters", async () => {
    const { fm, api } = setup({ "GET /api/v1/trees/acme/web/snapshots": [] });
    await api.trees.snapshots("acme", "web", { branch: "feature/x" });
    expect(fm.calls[0]?.url.searchParams.get("branch")).toBe("feature/x");
    expect(fm.calls[0]?.url.searchParams.get("limit")).toBe("50");
  });

  it("deletes protections and licenses by query parameter", async () => {
    const { fm, api } = setup({
      "DELETE /api/v1/trees/acme/web/protections": noContent(),
      "DELETE /api/v1/trees/acme/web/licenses": noContent(),
    });
    await api.trees.deleteProtection("acme", "web", "release/*");
    await api.trees.deleteLicense("acme", "web", "vendor/**");
    expect(fm.calls[0]?.url.searchParams.get("pattern")).toBe("release/*");
    expect(fm.calls[1]?.url.searchParams.get("path_glob")).toBe("vendor/**");
  });

  it("posts merge-request actions to their sub-resources", async () => {
    const { fm, api } = setup({
      "POST /api/v1/trees/acme/web/merge-requests/3/approve": {},
      "POST /api/v1/trees/acme/web/merge-requests/3/merge": {},
      "POST /api/v1/trees/acme/web/merge-requests/3/close": {},
    });
    await api.mergeRequests.approve("acme", "web", 3);
    await api.mergeRequests.merge("acme", "web", 3);
    await api.mergeRequests.close("acme", "web", 3);
    expect(fm.calls.map((c) => c.url.pathname.split("/").at(-1))).toEqual(["approve", "merge", "close"]);
  });

  it("fetches blobs as raw bytes with per-segment path encoding", async () => {
    const { fm, api } = setup({ "GET /api/v1/trees/acme/web/blob/s1/src/a b.txt": new Response(new Uint8Array([104, 105])) });
    const bytes = await api.trees.blob("acme", "web", "s1", "src/a b.txt");
    expect(new TextDecoder().decode(bytes)).toBe("hi");
    expect(fm.calls[0]?.url.pathname).toBe("/api/v1/trees/acme/web/blob/s1/src/a%20b.txt");
  });

  it("patches accounts with only the provided fields", async () => {
    const { fm, api } = setup({ "PATCH /api/v1/admin/accounts/bob": {} });
    await api.admin.updateAccount("bob", { disabled: true });
    expect(fm.calls[0]?.body).toEqual({ disabled: true });
  });

  it("filters the audit log", async () => {
    const { fm, api } = setup({ "GET /api/v1/audit": [] });
    await api.audit.list({ tenant: "acme", action: "branch.push", limit: 10 });
    expect(Object.fromEntries(fm.calls[0]!.url.searchParams)).toEqual({ tenant: "acme", action: "branch.push", limit: "10" });
  });
});
