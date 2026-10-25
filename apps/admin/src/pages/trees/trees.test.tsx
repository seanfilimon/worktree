import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as px from "@/test/fixtures.people";
import { json } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const routes = {
  "GET /api/v1/auth/me": px.meMember,
  "GET /api/v1/tenants": [fx.tenant, px.globex],
  "GET /api/v1/tenants/acme/trees": px.acmeTrees,
  "GET /api/v1/tenants/globex/trees": px.globexTrees,
};

/** Tree names in table order. */
function treeNames(): string[] {
  const table = screen.getByRole("table", { name: "Trees" });
  return within(table)
    .getAllByRole("link")
    .map((a) => a.textContent ?? "");
}

describe("trees page", () => {
  it("lists trees across tenants with totals", async () => {
    renderApp({ route: "/trees", api: routes });
    await screen.findByRole("table", { name: "Trees" });
    expect(treeNames()).toEqual(["acme/api", "acme/web", "globex/docs"]);
    const totals = screen.getByLabelText("Totals");
    expect(totals).toHaveTextContent("Trees3");
    expect(totals).toHaveTextContent("Branches4");
    expect(totals).toHaveTextContent("Snapshots55");
  });

  it("filters by tenant, visibility and search, kept in the URL", async () => {
    const { user, router } = renderApp({ route: "/trees", api: routes });
    await screen.findByRole("table", { name: "Trees" });

    await user.click(screen.getByRole("combobox", { name: "Filter by tenant" }));
    await user.click(await screen.findByRole("option", { name: "acme" }));
    await waitFor(() => expect(treeNames()).toEqual(["acme/api", "acme/web"]));
    expect(router.state.location.search).toContain("tenant=acme");

    await user.click(screen.getByRole("combobox", { name: "Filter by visibility" }));
    await user.click(await screen.findByRole("option", { name: "internal" }));
    await waitFor(() => expect(treeNames()).toEqual(["acme/api"]));

    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    await waitFor(() => expect(treeNames()).toHaveLength(3));

    await user.type(screen.getByRole("searchbox", { name: "Search trees" }), "handbook");
    expect(treeNames()).toEqual(["globex/docs"]);
    await user.clear(screen.getByRole("searchbox", { name: "Search trees" }));
    await user.type(screen.getByRole("searchbox", { name: "Search trees" }), "nothing-here");
    expect(screen.getByText("No trees match these filters")).toBeInTheDocument();
  });

  it("applies filters from the URL", async () => {
    renderApp({ route: "/trees?visibility=public", api: routes });
    await screen.findByRole("table", { name: "Trees" });
    expect(treeNames()).toEqual(["globex/docs"]);
  });

  it("sorts by snapshots and updated", async () => {
    const { user, router } = renderApp({ route: "/trees", api: routes });
    await screen.findByRole("table", { name: "Trees" });

    await user.click(screen.getByRole("button", { name: "Snapshots" }));
    expect(treeNames()).toEqual(["acme/api", "globex/docs", "acme/web"]);
    expect(screen.getByRole("columnheader", { name: "Snapshots" })).toHaveAttribute("aria-sort", "descending");
    expect(router.state.location.search).toContain("sort=snapshots");

    await user.click(screen.getByRole("button", { name: "Snapshots" }));
    expect(treeNames()).toEqual(["acme/web", "globex/docs", "acme/api"]);

    await user.click(screen.getByRole("button", { name: "Updated" }));
    expect(treeNames()).toEqual(["acme/api", "globex/docs", "acme/web"]);

    await user.click(screen.getByRole("button", { name: "Tree" }));
    expect(treeNames()).toEqual(["acme/api", "acme/web", "globex/docs"]);
  });

  it("only offers tenants the caller can create trees in", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees",
      api: {
        ...routes,
        "POST /api/v1/tenants/acme/trees": (req) => json({ ...fx.summary.tree, ...(req.body as object), id: "tree-x" }, 201),
      },
    });
    await screen.findByRole("table", { name: "Trees" });
    await user.click(screen.getByRole("button", { name: "New tree" }));
    const dialog = await screen.findByRole("dialog", { name: "New tree" });
    // Alice is only a viewer in globex, so acme is the single choice.
    expect(within(dialog).queryByRole("combobox", { name: "Tenant" })).not.toBeInTheDocument();
    expect(within(dialog).getByText("acme")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Name"), "infra");
    await user.click(within(dialog).getByRole("button", { name: "Create tree" }));
    await waitFor(() => expect(fetchMock.find("POST /api/v1/tenants/acme/trees")).toHaveLength(1));
    expect(fetchMock.find("POST /api/v1/tenants/acme/trees")[0]?.body).toMatchObject({ name: "infra", visibility: "private", default_branch: "main" });
  });

  it("lets server admins pick any tenant", async () => {
    const { user } = renderApp({ route: "/trees?new=1", api: { ...routes, "GET /api/v1/auth/me": px.meMulti } });
    const dialog = await screen.findByRole("dialog", { name: "New tree" });
    await user.click(within(dialog).getByRole("combobox", { name: "Tenant" }));
    expect(await screen.findByRole("option", { name: "globex" })).toBeInTheDocument();
  });
});
