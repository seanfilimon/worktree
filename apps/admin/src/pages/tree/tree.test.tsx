import { act, screen, within, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FakeEventSource } from "@/test/fakeEventSource";
import * as fx from "@/test/fixtures";
import { json } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";

describe("tree detail", () => {
  it("redirects the tree root to the overview tab", async () => {
    const { router } = renderApp({ route: "/trees/acme/web", api: treeRoutes() });
    await waitFor(() => expect(router.state.location.pathname).toBe("/trees/acme/web/overview"));
  });

  it("shows protected / default badges on the branches tab", async () => {
    renderApp({ route: "/trees/acme/web/branches", api: treeRoutes() });
    const table = await screen.findByRole("table", { name: "Branches" });

    const rows = within(table).getAllByRole("row");
    expect(rows[1]).toHaveTextContent("main");
    expect(within(rows[1]!).getByText("default")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("protected")).toBeInTheDocument();
    expect(within(rows[2]!).queryByText("protected")).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Branches/ })).toHaveAttribute("aria-current", "page");
  });

  it("lists snapshot history per branch with auto-generated badges", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/history",
      api: treeRoutes({ [`GET ${T}/snapshots`]: fx.snapshots }),
    });
    const list = await screen.findByRole("region", { name: "Snapshots on main" });
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(within(items[0]!).getByText("Add login page")).toBeInTheDocument();
    expect(within(items[0]!).getAllByText(/minutes ago/).length).toBeGreaterThan(0);
    expect(within(items[1]!).getByText("auto")).toBeInTheDocument();
    expect(within(items[0]!).queryByText("auto")).not.toBeInTheDocument();
    expect(fetchMock.find(`GET ${T}/snapshots`)[0]?.url.searchParams.get("branch")).toBe("main");

    await user.click(screen.getByRole("combobox", { name: "Branch" }));
    await user.click(await screen.findByRole("option", { name: "feature/login" }));
    expect(await screen.findByRole("region", { name: "Snapshots on feature/login" })).toBeInTheDocument();
    expect(fetchMock.find(`GET ${T}/snapshots`).at(-1)?.url.searchParams.get("branch")).toBe("feature/login");
  });

  // Files and Protections tabs are covered by files.test.tsx.

  it("shows staged WIP and refreshes it live from SSE", async () => {
    let current = fx.staged;
    const { fetchMock } = renderApp({
      route: "/trees/acme/web/staged",
      api: treeRoutes({ [`GET ${T}/staged`]: () => json(current) }),
    });
    const list = await screen.findByRole("list", { name: "Staged snapshots" });
    expect(within(list).getByText("bob")).toBeInTheDocument();
    expect(within(list).getByText("src/login.tsx")).toBeInTheDocument();
    expect(within(list).getByText("+1 added")).toBeInTheDocument();

    const es = FakeEventSource.latest();
    const url = new URL(es.url, "http://x");
    expect([url.searchParams.get("tenant"), url.searchParams.get("tree")]).toEqual(["acme", "web"]);

    current = [...fx.staged, { ...fx.staged[0]!, account_id: "acc-3", author: "carol", branch_name: "main", files_changed: ["docs/intro.md"] }];
    const before = fetchMock.find(`GET ${T}/staged`).length;
    act(() => {
      es.open();
      es.emit("snapshot.staged", { type: "snapshot.staged", tenant_id: fx.tenant.id, tree_id: "tree-web", actor: "carol", at: new Date().toISOString() });
    });
    expect(await within(list).findByText("carol")).toBeInTheDocument();
    expect(fetchMock.find(`GET ${T}/staged`).length).toBeGreaterThan(before);
  });

  it("ignores live events for other trees", async () => {
    const { fetchMock } = renderApp({ route: "/trees/acme/web/staged", api: treeRoutes({ [`GET ${T}/staged`]: fx.staged }) });
    await screen.findByRole("list", { name: "Staged snapshots" });
    const before = fetchMock.find(`GET ${T}/staged`).length;
    act(() => {
      FakeEventSource.latest().emit("snapshot.staged", { type: "snapshot.staged", tenant_id: "x", tree_id: "other-tree", at: new Date().toISOString() });
    });
    expect(fetchMock.find(`GET ${T}/staged`).length).toBe(before);
  });

  it("shows tags with links to their target snapshot detail", async () => {
    renderApp({
      route: "/trees/acme/web/tags",
      api: treeRoutes({
        [`GET ${T}/tags`]: [{ tree_id: "tree-web", name: "v1.0.0", target: "snap-3", message: "First release", tagger: "alice", created_at: new Date().toISOString() }],
      }),
    });
    const table = await screen.findByRole("table", { name: "Tags" });
    expect(within(table).getByText("v1.0.0")).toBeInTheDocument();
    expect(within(table).getByRole("link", { name: "snap3" })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-3");
  });
});
