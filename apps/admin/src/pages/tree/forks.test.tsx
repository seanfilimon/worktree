import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as mrx from "@/test/fixtures.mr";
import { apiError, json } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";
import { allowedVisibilities, forkNameError, syncSummary, type Fork, type Upstream } from "./forksApi";

const T = "/api/v1/trees/acme/web";
const UP = "/api/v1/trees/core/web";

const fork: Fork = {
  tree_id: "tree-bob-web",
  tenant_id: "ten-bob",
  tenant: "bob",
  name: "web",
  description: "",
  visibility: "private",
  branches: ["main"],
  created_by: "bob",
  created_at: "2026-10-01T10:00:00Z",
  last_sync_at: "2026-10-02T10:00:00Z",
};

/** acme/web as a fork of core/web. */
const forkSummary = { ...fx.summary, tree: { ...fx.summary.tree, fork_of: "tree-core-web", visibility: "internal" as const } };

const upstream: Upstream = {
  fork: true,
  upstream_tree_id: "tree-core-web",
  upstream_missing: false,
  upstream_hidden: false,
  tenant: "core",
  name: "web",
  visibility: "internal",
  default_branch: "main",
  forked_by: "alice",
  forked_at: "2026-10-01T10:00:00Z",
  branches: [
    { name: "main", tip: "aaaa1111", upstream_tip: "bbbb2222", status: "behind" },
    { name: "feature/login", tip: "cccc3333", upstream_tip: "", status: "fork_only" },
    { name: "next", tip: "", upstream_tip: "dddd4444", status: "upstream_only" },
  ],
};

describe("fork helpers", () => {
  it("limits visibility, validates names and summarises syncs", () => {
    expect(allowedVisibilities("private")).toEqual(["private"]);
    expect(allowedVisibilities("public")).toEqual(["private", "internal", "public"]);
    expect(forkNameError("")).toBe("");
    expect(forkNameError("web-2")).toBe("");
    expect(forkNameError("bad name")).toMatch(/letters/);
    expect(forkNameError("web.")).toMatch(/letters/);
    expect(syncSummary({ branches: [], updated: 0, diverged: [] })).toBe("Everything up to date");
    expect(syncSummary({ branches: [], updated: 2, diverged: ["dev"] })).toBe("2 branches updated; diverged: dev (merge the upstream locally)");
  });
});

describe("forks tab", () => {
  it("lists forks of a tree that is not a fork", async () => {
    const { fetchMock } = renderApp({
      route: "/trees/acme/web/forks",
      api: treeRoutes({ [`GET ${T}/forks`]: [fork] }),
    });
    const table = await screen.findByRole("table", { name: "Forks" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(1);
    expect(within(rows[0]!).getByRole("link", { name: "bob/web" })).toHaveAttribute("href", "/trees/bob/web/forks");
    expect(rows[0]).toHaveTextContent("by bob");
    // Not a fork: no upstream card, no badge, no upstream request.
    expect(screen.queryByText("Upstream")).not.toBeInTheDocument();
    expect(fetchMock.find(`GET ${T}/fork`)).toHaveLength(0);
  });

  it("forks the tree into a chosen tenant", async () => {
    const { user, fetchMock, router } = renderApp({
      route: "/trees/acme/web/forks",
      api: treeRoutes({
        [`GET ${T}/forks`]: [],
        [`POST ${T}/forks`]: (req) => json({ ...fx.summary.tree, id: "tree-new", tenant_id: "ten-alice", name: "site", fork_of: "tree-web", ...(req.body as object) }, 201),
      }),
    });
    expect(await screen.findByText("No forks")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Fork" }));
    const dialog = await screen.findByRole("dialog", { name: "Fork this tree" });
    // acme/web is private: so is every fork.
    expect(within(dialog).getAllByRole("radio").map((r) => r.textContent)).toEqual(["private"]);
    await user.type(within(dialog).getByLabelText("Name"), "bad name");
    await user.click(within(dialog).getByRole("button", { name: "Fork" }));
    expect(within(dialog).getByText(/letters, digits/)).toBeInTheDocument();
    await user.clear(within(dialog).getByLabelText("Name"));
    await user.type(within(dialog).getByLabelText("Name"), "site");
    await user.type(within(dialog).getByLabelText("Branches"), "main{Enter}");
    await user.click(within(dialog).getByRole("combobox", { name: "Destination tenant" }));
    await user.click(await screen.findByRole("option", { name: "acme" }));
    await user.click(within(dialog).getByRole("button", { name: "Fork" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/forks`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/forks`)[0]?.body).toEqual({ tenant: "acme", visibility: "private", name: "site", branches: ["main"] });
    await waitFor(() => expect(router.state.location.pathname).toBe("/trees/acme/site"));
  });

  it("shows a license refusal inline", async () => {
    const { user } = renderApp({
      route: "/trees/acme/web/forks",
      api: treeRoutes({
        [`GET ${T}/forks`]: [],
        [`POST ${T}/forks`]: apiError(403, "license_violation", 'cannot fork into tenant "alice": path "secret/key" is licensed "Proprietary"'),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Fork" }));
    const dialog = await screen.findByRole("dialog", { name: "Fork this tree" });
    expect(within(dialog).getByRole("combobox", { name: "Destination tenant" })).toHaveTextContent("alice (personal)");
    await user.click(within(dialog).getByRole("button", { name: "Fork" }));
    expect(await within(dialog).findByText(/cannot fork into tenant/)).toBeInTheDocument();
  });

  it("shows the upstream of a fork, syncs it and opens a merge request upstream", async () => {
    const { user, fetchMock, router } = renderApp({
      route: "/trees/acme/web/forks",
      api: treeRoutes({
        [`GET ${T}`]: forkSummary,
        [`GET ${T}/forks`]: [],
        [`GET ${T}/fork`]: upstream,
        [`POST ${T}/fork/sync`]: { branches: [{ name: "main", result: "fast_forwarded", old_tip: "a", new_tip: "b" }], updated: 1, diverged: null },
        [`GET ${UP}`]: { ...fx.summary, tree: { ...fx.summary.tree, id: "tree-core-web", tenant_id: "ten-core" } },
        [`GET ${UP}/branches`]: fx.branches,
        [`POST ${UP}/merge-requests`]: (req) => json({ ...fx.mergeRequest, number: 12, tree_id: "tree-core-web", source_tree_id: "tree-web", ...(req.body as object) }, 201),
      }),
    });
    // Header badge links to the upstream.
    expect(await screen.findByRole("link", { name: "Fork of core/web" })).toHaveAttribute("href", "/trees/core/web");
    const table = await screen.findByRole("table", { name: "Branches compared with the upstream" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining("Behind"),
      expect.stringContaining("Only in fork"),
      expect.stringContaining("Only upstream"),
    ]);

    await user.click(screen.getByRole("button", { name: "Sync with upstream" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/fork/sync`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/fork/sync`)[0]?.body).toEqual({});
    expect(await screen.findByText("1 branch updated")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open merge request upstream" }));
    const dialog = await screen.findByRole("dialog", { name: "Open a merge request upstream" });
    await user.click(within(dialog).getByRole("button", { name: "Open merge request" }));
    expect(within(dialog).getByText("Required.")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Title"), "Login page from the fork");
    await user.click(within(dialog).getByRole("combobox", { name: "Source branch" }));
    await user.click(await screen.findByRole("option", { name: "feature/login" }));
    await user.click(within(dialog).getByRole("button", { name: "Open merge request" }));
    await waitFor(() => expect(fetchMock.find(`POST ${UP}/merge-requests`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${UP}/merge-requests`)[0]?.body).toEqual({
      title: "Login page from the fork",
      description: "",
      source_branch: "feature/login",
      target_branch: "main",
      source_tenant: "acme",
      source_tree: "web",
    });
    await waitFor(() => expect(router.state.location.pathname).toBe("/trees/core/web/merge-requests/12"));
  });

  it("explains a deleted upstream", async () => {
    renderApp({
      route: "/trees/acme/web/forks",
      api: treeRoutes({
        [`GET ${T}`]: forkSummary,
        [`GET ${T}/forks`]: [],
        [`GET ${T}/fork`]: { fork: true, upstream_tree_id: "tree-core-web", upstream_missing: true, upstream_hidden: false, branches: null },
      }),
    });
    expect(await screen.findByText("The upstream was deleted")).toBeInTheDocument();
    expect(await screen.findByText("fork (upstream deleted)")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Sync with upstream" })).not.toBeInTheDocument();
  });

  it("reports servers without forks", async () => {
    renderApp({
      route: "/trees/acme/web/forks",
      api: treeRoutes({ [`GET ${T}/forks`]: apiError(501, "not_implemented", "forks are not available on this server") }),
    });
    expect(await screen.findByText(/not available/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Fork" })).not.toBeInTheDocument();
  });
});

describe("merge request from a fork", () => {
  it("compares the fork's tip copied into this tree", async () => {
    const mr = { ...fx.mergeRequest, source_branch: "topic", source_tree_id: "tree-bob-web" };
    const { fetchMock } = renderApp({
      route: "/trees/acme/web/merge-requests/7?tab=changes",
      api: treeRoutes({
        [`GET ${T}/merge-requests/7`]: mr,
        [`GET ${T}/merge-requests/7/source`]: {
          cross_tree: true,
          tree_id: "tree-bob-web",
          tenant: "bob",
          tree: "web",
          branch: "topic",
          tip: "fork-tip",
          target_tip: "copied-tip",
          missing: false,
        },
        [`GET ${T}/compare`]: mrx.compareReady,
      }),
    });
    expect(await screen.findByText("bob/web:topic")).toBeInTheDocument();
    await waitFor(() => expect(fetchMock.find(`GET ${T}/compare`).length).toBeGreaterThan(0));
    const cmp = fetchMock.find(`GET ${T}/compare`).at(-1)!;
    expect(cmp.url.searchParams.get("to")).toBe("copied-tip");
    expect(cmp.url.searchParams.get("from")).toBe("snap-3");
  });
});
