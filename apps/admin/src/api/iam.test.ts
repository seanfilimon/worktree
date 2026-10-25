import { describe, expect, it } from "vitest";
import { json, mockFetch, noContent } from "@/test/mockFetch";
import { ApiClient } from "./client";
import { createIamApi, invitationStatus, permits, type Invitation } from "./iam";
import { createPersistentValue } from "./storage";

function setup(routes: Parameters<typeof mockFetch>[0]) {
  const fm = mockFetch(routes);
  const iam = createIamApi(new ApiClient({ tokenStore: createPersistentValue("iam.token"), fetch: fm.fetch }));
  return { fm, iam };
}

describe("iam endpoints", () => {
  it("normalises null lists and nested slices", async () => {
    const { iam } = setup({
      "GET /api/v1/tenants/acme/teams": json(null),
      "GET /api/v1/tenants/acme/teams/x": { team: { slug: "x" }, members: null, grants: null },
      "GET /api/v1/tenants/acme/policies": [{ id: "p", statements: [{ effect: "allow", actions: null, resources: null, principals: null }] }],
      "GET /api/v1/tenants/acme/roles": [{ name: "r", permissions: null, builtin: false }],
      "POST /api/v1/iam/simulate": { allowed: true, effect: "allow", reasons: null },
    });
    await expect(iam.teams.list("acme")).resolves.toEqual([]);
    await expect(iam.teams.get("acme", "x")).resolves.toMatchObject({ members: [], grants: [] });
    const [p] = await iam.policies.list("acme");
    expect(p?.statements[0]).toMatchObject({ actions: [], resources: [], principals: [], conditions: {} });
    expect((await iam.roles.list("acme"))[0]?.permissions).toEqual([]);
    expect((await iam.simulate({ principal: { type: "user", name: "a" }, action: "tree:read", tenant: "acme", context: {} })).reasons).toEqual([]);
  });

  it("deletes grants by subject query and passes effective parameters", async () => {
    const { fm, iam } = setup({
      "DELETE /api/v1/trees/acme/web/grants": noContent(),
      "GET /api/v1/iam/effective": { role: "viewer", team_roles: null, grants: null, permissions: null },
    });
    await iam.grants.remove("acme", "web", "service_account", "ci-bot");
    expect(fm.calls[0]?.url.searchParams.get("subject_type")).toBe("service_account");
    expect(fm.calls[0]?.url.searchParams.get("subject")).toBe("ci-bot");
    const e = await iam.effective({ tenant: "acme", principal: "sa:acme/ci-bot" });
    expect(e).toEqual({ role: "viewer", team_roles: [], grants: [], permissions: [] });
    expect(fm.calls[1]?.url.searchParams.get("principal")).toBe("sa:acme/ci-bot");
    expect(fm.calls[1]?.url.searchParams.has("tree")).toBe(false);
  });

  it("posts statements to the validator", async () => {
    const { fm, iam } = setup({ "POST /api/v1/tenants/acme/policies/validate": { valid: false, errors: null } });
    await expect(iam.policies.validate("acme", [])).resolves.toEqual({ valid: false, errors: [] });
    expect(fm.calls[0]?.body).toEqual({ statements: [] });
  });
});

describe("iam helpers", () => {
  it("matches wildcard permissions", () => {
    expect(permits(["tree:*"], "tree:write")).toBe(true);
    expect(permits(["tree:*"], "mr:merge")).toBe(false);
    expect(permits(["*"], "mr:merge")).toBe(true);
    expect(permits(["mr:merge"], "mr:merge")).toBe(true);
  });

  it("derives invitation status", () => {
    const base: Invitation = {
      id: "i",
      tenant_id: "t",
      role: "viewer",
      invited_by: "a",
      created_at: "2026-01-01T00:00:00Z",
      expires_at: "2026-02-01T00:00:00Z",
      revoked: false,
    };
    const now = Date.parse("2026-01-15T00:00:00Z");
    expect(invitationStatus(base, now)).toBe("pending");
    expect(invitationStatus({ ...base, accepted_at: "2026-01-02T00:00:00Z" }, now)).toBe("accepted");
    expect(invitationStatus({ ...base, revoked: true }, now)).toBe("revoked");
    expect(invitationStatus(base, Date.parse("2026-03-01T00:00:00Z"))).toBe("expired");
  });
});
