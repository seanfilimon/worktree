import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as ix from "@/test/fixtures.iam";
import * as px from "@/test/fixtures.people";
import { apiError, noContent } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const G = "/api/v1/trees/acme/web/grants";
const T = "/api/v1/tenants/acme";

function accessRoutes(extra: Record<string, unknown> = {}) {
  return treeRoutes({
    [`GET ${G}`]: [ix.grant, { ...ix.grant, subject_type: "service_account", subject_id: "sa-1", subject: "ci-bot", role: "viewer" }],
    [`GET ${T}/roles`]: ix.roles,
    [`GET ${T}/members`]: px.acmeMembers,
    [`GET ${T}/teams`]: [ix.team],
    [`GET ${T}/service-accounts`]: [ix.serviceAccount],
    ...(extra as object),
  });
}

describe("tree access tab", () => {
  it("lists grants and adds one for a user", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/access",
      api: accessRoutes({ [`PUT ${G}`]: (req: { body: unknown }) => ({ ...ix.grant, ...(req.body as object) }) }),
    });
    expect(await screen.findByRole("link", { name: "Access" })).toHaveAttribute("aria-current", "page");
    const table = await screen.findByRole("table", { name: "Grants" });
    expect(within(table).getByText("platform").closest("tr")).toHaveTextContent("Team");
    expect(within(table).getByText("ci-bot").closest("tr")).toHaveTextContent("Service account");

    await user.click(screen.getByRole("button", { name: "Add grant" }));
    const dialog = await screen.findByRole("dialog", { name: "Add grant" });
    await user.click(within(dialog).getByRole("button", { name: "Save grant" }));
    expect(within(dialog).getByText("Required.")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Username"), "carol");
    await user.click(within(dialog).getByRole("combobox", { name: "Role" }));
    await user.click(await screen.findByRole("option", { name: "release-manager" }));
    await user.click(within(dialog).getByRole("button", { name: "Save grant" }));
    await waitFor(() =>
      expect(fetchMock.find(`PUT ${G}`)[0]?.body).toEqual({ subject_type: "user", subject: "carol", role: "release-manager" }),
    );
    expect(await screen.findByText("Grant saved")).toBeInTheDocument();
  });

  it("grants a team picked from the list and removes a grant", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/access",
      api: accessRoutes({
        [`PUT ${G}`]: (req: { body: unknown }) => ({ ...ix.grant, ...(req.body as object) }),
        [`DELETE ${G}`]: noContent(),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Add grant" }));
    const dialog = await screen.findByRole("dialog", { name: "Add grant" });
    await user.click(within(dialog).getByRole("radio", { name: "Team" }));
    await user.click(within(dialog).getByRole("combobox", { name: "Team" }));
    await user.click(await screen.findByRole("option", { name: "Platform (platform)" }));
    await user.click(within(dialog).getByRole("button", { name: "Save grant" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${G}`)[0]?.body).toEqual({ subject_type: "team", subject: "platform", role: "developer" }));

    await user.click(await screen.findByRole("button", { name: "Remove grant for ci-bot" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Remove grant for ci-bot?" });
    await user.click(within(confirm).getByRole("button", { name: "Remove grant" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${G}`)).toHaveLength(1));
    const q = fetchMock.find(`DELETE ${G}`)[0]!.url.searchParams;
    expect(q.get("subject_type")).toBe("service_account");
    expect(q.get("subject")).toBe("ci-bot");
  });

  it("is read-only for members without IAM rights", async () => {
    renderApp({ route: "/trees/acme/web/access", api: { ...accessRoutes(), "GET /api/v1/auth/me": fx.memberMe } });
    expect(await screen.findByRole("table", { name: "Grants" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add grant" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Remove grant/ })).not.toBeInTheDocument();
  });
});

const simRoutes = {
  "GET /api/v1/iam/actions": ix.actions,
  [`GET ${T}/trees`]: px.acmeTrees,
  "GET /api/v1/iam/effective": ix.effective,
};

describe("access simulator", () => {
  it("is reachable from the sidebar", async () => {
    const { user, router } = renderApp({ route: "/audit", api: { ...simRoutes, "GET /api/v1/audit": [] } });
    const nav = await screen.findByRole("navigation", { name: "Main" });
    await user.click(within(nav).getByRole("link", { name: "Access simulator" }));
    expect(router.state.location.pathname).toBe("/iam/simulator");
    expect(await screen.findByRole("heading", { name: "Access simulator", level: 1 })).toBeInTheDocument();
  });

  it("simulates a request and renders the decision with reasons and effective permissions", async () => {
    const { user, fetchMock } = renderApp({
      route: "/iam/simulator",
      api: { ...simRoutes, "POST /api/v1/iam/simulate": ix.decision },
    });
    const form = await screen.findByRole("form", { name: "Simulation request" });
    const principal = within(form).getByLabelText("Username");
    expect(principal).toHaveValue("alice");
    await user.clear(principal);
    await user.type(principal, "bob");
    await user.click(within(form).getByRole("combobox", { name: "Action" }));
    await user.click(await screen.findByRole("option", { name: "branch:push" }));
    await waitFor(() => expect(within(form).getByRole("combobox", { name: "Tenant" })).toHaveTextContent("acme"));
    await user.click(within(form).getByRole("combobox", { name: "Tree" }));
    await user.click(await screen.findByRole("option", { name: "web" }));
    await user.type(within(form).getByLabelText("Branch"), "main");
    await user.type(within(form).getByLabelText("Client IP"), "203.0.113.7");
    await user.click(within(form).getByRole("switch", { name: "Signed in with MFA" }));
    await user.click(within(form).getByRole("button", { name: "Simulate" }));

    const card = await screen.findByLabelText("Decision");
    expect(within(card).getByText("DENIED")).toBeInTheDocument();
    expect(card).toHaveTextContent("user:bob → branch:push on wt:branch/acme/web/main");
    expect(card).toHaveTextContent("Explicit deny");
    const reasons = within(within(card).getByRole("list", { name: "Reasons" })).getAllByRole("listitem");
    expect(reasons).toHaveLength(2);
    expect(reasons[0]).toHaveTextContent("Tenant role");
    expect(reasons[0]).toHaveTextContent("bob is developer in acme");
    expect(reasons[1]).toHaveTextContent("Policy");
    expect(reasons[1]).toHaveTextContent("deny");

    expect(fetchMock.find("POST /api/v1/iam/simulate")[0]?.body).toEqual({
      principal: { type: "user", name: "bob" },
      action: "branch:push",
      tenant: "acme",
      tree: "web",
      branch: "main",
      context: { ip: "203.0.113.7", mfa: true },
    });

    const eff = await screen.findByLabelText("Effective permissions");
    expect(await within(eff).findByRole("list", { name: "tree permissions" })).toHaveTextContent("tree:readtree:write");
    expect(within(eff).getByRole("list", { name: "mr permissions" })).toHaveTextContent("mr:merge");
    expect(within(eff).getByRole("list", { name: "Team roles" })).toHaveTextContent("platform");
    const q = fetchMock.find("GET /api/v1/iam/effective")[0]!.url.searchParams;
    expect(Object.fromEntries(q)).toEqual({ tenant: "acme", tree: "web", principal: "user:bob" });
  });

  it("simulates a service account and shows ALLOWED", async () => {
    const { user, fetchMock } = renderApp({
      route: "/iam/simulator?tenant=acme",
      api: {
        ...simRoutes,
        "POST /api/v1/iam/simulate": { allowed: true, effect: "allow", reasons: [{ source: "service_account", detail: "ci-bot is developer", effect: "allow" }] },
      },
    });
    const form = await screen.findByRole("form", { name: "Simulation request" });
    await user.click(within(form).getByRole("radio", { name: "Service account" }));
    const principal = within(form).getByLabelText("Service account");
    await user.clear(principal);
    await user.type(principal, "acme/ci-bot");
    await user.click(within(form).getByRole("button", { name: "Simulate" }));
    expect(await screen.findByText("ALLOWED")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/iam/simulate")[0]?.body).toMatchObject({
      principal: { type: "service_account", name: "ci-bot", tenant: "acme" },
      action: "tree:read",
      tenant: "acme",
      context: {},
    });
    await waitFor(() => expect(fetchMock.find("GET /api/v1/iam/effective")[0]?.url.searchParams.get("principal")).toBe("sa:acme/ci-bot"));
  });

  it("reports simulation errors inline", async () => {
    const { user } = renderApp({
      route: "/iam/simulator",
      api: {
        ...simRoutes,
        "POST /api/v1/iam/simulate": apiError(403, "forbidden", "iam:read required"),
        "GET /api/v1/iam/effective": apiError(403, "forbidden", "iam:read required"),
      },
    });
    const form = await screen.findByRole("form", { name: "Simulation request" });
    await waitFor(() => expect(within(form).getByRole("combobox", { name: "Tenant" })).toHaveTextContent("acme"));
    await user.click(within(form).getByRole("button", { name: "Simulate" }));
    expect(await screen.findByText("Request failed")).toBeInTheDocument();
    expect(await screen.findByText("Could not load effective permissions")).toBeInTheDocument();
    expect(screen.getAllByText("iam:read required").length).toBeGreaterThanOrEqual(2);
  });
});
