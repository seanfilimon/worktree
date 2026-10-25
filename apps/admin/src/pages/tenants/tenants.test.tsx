import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as px from "@/test/fixtures.people";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const listRoutes = {
  "GET /api/v1/auth/me": px.meMulti,
  "GET /api/v1/tenants": [fx.tenant, px.globex, px.aliceHome],
  "GET /api/v1/tenants/acme/trees": px.acmeTrees,
  "GET /api/v1/tenants/acme/members": px.acmeMembers,
  "GET /api/v1/tenants/globex/trees": px.globexTrees,
  "GET /api/v1/tenants/globex/members": apiError(403, "forbidden", "not a tenant admin"),
  "GET /api/v1/tenants/alice/trees": [],
  "GET /api/v1/tenants/alice/members": [{ account: fx.account, role: "owner" }],
};

describe("tenants list", () => {
  it("shows badges and counts, and filters by search", async () => {
    const { user } = renderApp({ route: "/tenants", api: listRoutes });
    const table = await screen.findByRole("table", { name: "Tenants" });
    const acme = within(table).getByRole("link", { name: "Acme Corp" }).closest("tr")!;
    expect(acme).toHaveTextContent("Organisation");
    expect(acme).toHaveTextContent("pro");
    expect(acme).toHaveTextContent("owner");
    await waitFor(() => expect(within(acme).getAllByRole("cell")[4]).toHaveTextContent("2"));
    expect(within(acme).getAllByRole("cell")[5]).toHaveTextContent("3");
    const globex = within(table).getByRole("link", { name: "Globex" }).closest("tr")!;
    // Members of globex are not readable by a viewer.
    await waitFor(() => expect(within(globex).getAllByRole("cell")[5]).toHaveTextContent("—"));

    await user.type(screen.getByRole("searchbox", { name: "Search tenants" }), "glob");
    expect(within(table).queryByRole("link", { name: "Acme Corp" })).not.toBeInTheDocument();
    expect(within(table).getByRole("link", { name: "Globex" })).toBeInTheDocument();
    expect(screen.getByText("1 of 3 tenants")).toBeInTheDocument();
  });

  it("opens the detail page when a row is clicked", async () => {
    const { user, router } = renderApp({ route: "/tenants", api: { ...listRoutes, "GET /api/v1/tenants/globex": px.globex } });
    const table = await screen.findByRole("table", { name: "Tenants" });
    await user.click(within(table).getByText("globex"));
    expect(router.state.location.pathname).toBe("/tenants/globex");
  });

  it("validates the new organisation form and derives the slug from the name", async () => {
    const { user, fetchMock, router } = renderApp({
      route: "/tenants",
      api: {
        ...listRoutes,
        "POST /api/v1/tenants": (req) => json({ ...px.globex, id: "ten-new", ...(req.body as object), plan: "free" }, 201),
        "GET /api/v1/tenants/new-co": { ...px.globex, id: "ten-new", slug: "new-co", name: "New Co" },
      },
    });
    await user.click(await screen.findByRole("button", { name: "New organisation" }));
    const dialog = await screen.findByRole("dialog", { name: "New organisation" });

    await user.click(within(dialog).getByRole("button", { name: "Create organisation" }));
    expect(await within(dialog).findByText("Required.")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/tenants")).toHaveLength(0);

    await user.type(within(dialog).getByLabelText("Display name"), "New Co!");
    expect(within(dialog).getByLabelText("Slug")).toHaveValue("new-co");
    expect(within(dialog).getByText("new-co/<tree>")).toBeInTheDocument();
    expect(within(dialog).getByText("Slug is valid")).toBeInTheDocument();

    const slug = within(dialog).getByLabelText("Slug");
    await user.clear(slug);
    await user.type(slug, "-bad");
    expect(within(dialog).getByText("Slug is not valid")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Create organisation" }));
    expect(slug).toHaveAttribute("aria-invalid", "true");
    expect(fetchMock.find("POST /api/v1/tenants")).toHaveLength(0);

    await user.clear(slug);
    await user.type(slug, "new-co");
    // Once edited by hand the slug no longer follows the name.
    await user.type(within(dialog).getByLabelText("Display name"), " Ltd");
    expect(slug).toHaveValue("new-co");
    await user.click(within(dialog).getByRole("button", { name: "Create organisation" }));

    await waitFor(() => expect(router.state.location.pathname).toBe("/tenants/new-co"));
    expect(fetchMock.find("POST /api/v1/tenants")[0]?.body).toEqual({ slug: "new-co", name: "New Co! Ltd" });
  });

  it("shows server errors inside the dialog", async () => {
    const { user } = renderApp({
      route: "/tenants",
      api: { ...listRoutes, "POST /api/v1/tenants": apiError(409, "conflict", "tenant acme already exists") },
    });
    await user.click(await screen.findByRole("button", { name: "New organisation" }));
    const dialog = await screen.findByRole("dialog", { name: "New organisation" });
    await user.type(within(dialog).getByLabelText("Display name"), "Acme");
    await user.click(within(dialog).getByRole("button", { name: "Create organisation" }));
    expect(await within(dialog).findByText("tenant acme already exists")).toBeInTheDocument();
  });
});

const detailRoutes = {
  "GET /api/v1/tenants/acme": fx.tenant,
  "GET /api/v1/tenants/acme/trees": px.acmeTrees,
  "GET /api/v1/tenants/acme/members": px.acmeMembers,
};

describe("tenant detail", () => {
  it("adds a member by username with a role", async () => {
    const { user, fetchMock } = renderApp({
      route: "/tenants/acme?tab=members",
      api: {
        ...detailRoutes,
        "PUT /api/v1/tenants/acme/members/dave": (req) =>
          json({ account: { ...px.carol, id: "acc-4", username: "dave" }, role: (req.body as { role: string }).role }),
      },
    });
    await user.click(await screen.findByRole("button", { name: "Add member" }));
    const dialog = await screen.findByRole("dialog", { name: "Add member" });
    await user.type(within(dialog).getByLabelText("Username"), "Dave");
    await user.click(within(dialog).getByRole("combobox", { name: "Role" }));
    await user.click(await screen.findByRole("option", { name: "maintainer" }));
    expect(within(dialog).getByText(/approve and merge/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Add member" }));

    expect(await screen.findByText("Member added")).toBeInTheDocument();
    expect(fetchMock.find("PUT /api/v1/tenants/acme/members/dave")[0]?.body).toEqual({ role: "maintainer" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("reports an unknown username inline", async () => {
    const { user } = renderApp({
      route: "/tenants/acme?tab=members",
      api: { ...detailRoutes, "PUT /api/v1/tenants/acme/members/ghost": apiError(404, "not_found", "account ghost not found") },
    });
    await user.click(await screen.findByRole("button", { name: "Add member" }));
    const dialog = await screen.findByRole("dialog", { name: "Add member" });
    await user.type(within(dialog).getByLabelText("Username"), "ghost");
    await user.click(within(dialog).getByRole("button", { name: "Add member" }));
    expect(await within(dialog).findByText("account ghost not found")).toBeInTheDocument();
  });

  it("removes a member after confirmation", async () => {
    let members = px.acmeMembers;
    const { user, fetchMock } = renderApp({
      route: "/tenants/acme?tab=members",
      api: {
        ...detailRoutes,
        "GET /api/v1/tenants/acme/members": () => json(members),
        "DELETE /api/v1/tenants/acme/members/carol": () => {
          members = members.filter((m) => m.account.username !== "carol");
          return noContent();
        },
      },
    });
    const table = await screen.findByRole("table", { name: "Members" });
    // You cannot remove yourself here.
    expect(within(table).queryByRole("button", { name: "Actions for alice" })).not.toBeInTheDocument();
    await user.click(within(table).getByRole("button", { name: "Actions for carol" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove from tenant" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Remove carol?" });
    await user.click(within(confirm).getByRole("button", { name: "Remove member" }));

    expect(await screen.findByText("Member removed")).toBeInTheDocument();
    expect(fetchMock.find("DELETE /api/v1/tenants/acme/members/carol")).toHaveLength(1);
    await waitFor(() => expect(screen.getByRole("table", { name: "Members" })).not.toHaveTextContent("carol"));
  });

  it("is read-only for members without admin rights", async () => {
    renderApp({
      route: "/tenants/globex?tab=members",
      api: {
        "GET /api/v1/auth/me": px.meMember,
        "GET /api/v1/tenants/globex": px.globex,
        "GET /api/v1/tenants/globex/trees": px.globexTrees,
        "GET /api/v1/tenants/globex/members": [{ account: fx.account, role: "viewer" }, { account: fx.bob, role: "owner" }],
      },
    });
    const table = await screen.findByRole("table", { name: "Members" });
    expect(within(table).queryByRole("combobox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add member" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New tree" })).not.toBeInTheDocument();
    expect(screen.getByRole("table", { name: "Role permissions" })).toHaveTextContent("Grant or revoke owner");
  });

  it("creates a tree from the Trees tab", async () => {
    const { user, fetchMock, router } = renderApp({
      route: "/tenants/acme",
      api: {
        ...detailRoutes,
        "POST /api/v1/tenants/acme/trees": (req) => json({ ...fx.summary.tree, ...(req.body as object), id: "tree-new" }, 201),
      },
    });
    expect(await screen.findByRole("table", { name: "Trees" })).toHaveTextContent("api");
    await user.click(screen.getByRole("button", { name: "New tree" }));
    const dialog = await screen.findByRole("dialog", { name: "New tree" });
    await user.type(within(dialog).getByLabelText("Name"), "bad name");
    await user.click(within(dialog).getByRole("button", { name: "Create tree" }));
    expect(await within(dialog).findByText(/Letters, digits/)).toBeInTheDocument();

    await user.clear(within(dialog).getByLabelText("Name"));
    await user.type(within(dialog).getByLabelText("Name"), "backend");
    expect(within(dialog).getByText("acme/backend")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Description"), "Services");
    await user.click(within(dialog).getByRole("radio", { name: /public/i }));
    await user.clear(within(dialog).getByLabelText("Default branch"));
    await user.type(within(dialog).getByLabelText("Default branch"), "trunk");
    await user.click(within(dialog).getByRole("button", { name: "Create tree" }));

    await waitFor(() => expect(router.state.location.pathname).toMatch(/^\/trees\/acme\/backend/));
    expect(fetchMock.find("POST /api/v1/tenants/acme/trees")[0]?.body).toEqual({
      name: "backend",
      description: "Services",
      visibility: "public",
      default_branch: "trunk",
    });
  });

  it("renames the tenant and lets server admins change the plan", async () => {
    const { user, fetchMock } = renderApp({
      route: "/tenants/acme?tab=settings",
      api: {
        ...detailRoutes,
        "PATCH /api/v1/tenants/acme": (req) => json({ ...fx.tenant, ...(req.body as object) }),
      },
    });
    const name = await screen.findByLabelText("Display name");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await user.clear(name);
    expect(screen.getByText("Required.")).toBeInTheDocument();
    await user.type(name, "Acme Inc");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Tenant updated")).toBeInTheDocument();
    expect(fetchMock.find("PATCH /api/v1/tenants/acme")[0]?.body).toEqual({ name: "Acme Inc" });

    await user.click(screen.getByRole("combobox", { name: "Plan" }));
    await user.click(await screen.findByRole("option", { name: "enterprise" }));
    await user.click(screen.getByRole("button", { name: "Change plan" }));
    await waitFor(() => expect(fetchMock.find("PATCH /api/v1/tenants/acme")).toHaveLength(2));
    expect(fetchMock.find("PATCH /api/v1/tenants/acme")[1]?.body).toEqual({ plan: "enterprise" });
  });

  it("hides the plan picker from non server admins", async () => {
    renderApp({ route: "/tenants/acme?tab=settings", api: { ...detailRoutes, "GET /api/v1/auth/me": px.meMember } });
    expect(await screen.findByLabelText("Display name")).toBeEnabled();
    expect(screen.queryByRole("combobox", { name: "Plan" })).not.toBeInTheDocument();
    expect(screen.getByText("Contact a server admin to change the plan.")).toBeInTheDocument();
  });
});
