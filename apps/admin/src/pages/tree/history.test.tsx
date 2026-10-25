import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { installDomPolyfills } from "@/test/dom.history";
import * as fx from "@/test/fixtures";
import * as hx from "@/test/fixtures.history";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";

installDomPolyfills();

describe("tree overview", () => {
  it("renders stats, charts, contributors, events and the README", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/overview",
      api: treeRoutes({
        [`GET ${T}/stats`]: hx.treeStats,
        [`GET ${T}/snapshots`]: fx.snapshots,
        [`GET ${T}/snapshots/snap-3`]: fx.snapshotDetail,
        [`GET ${T}/blob/snap-3/README.md`]: new Response(hx.readme),
        "GET /api/v1/events/recent": hx.recentEvents,
      }),
    });

    const stats = await screen.findByRole("region", { name: "Tree statistics" });
    expect(await within(stats).findByText("42")).toBeInTheDocument();
    expect(within(stats).getByText("128")).toBeInTheDocument();
    expect(within(stats).getByText("3.0 MiB")).toBeInTheDocument();
    expect(within(stats).getByText("2")).toBeInTheDocument();

    expect(screen.getByRole("img", { name: "Snapshots per day over the last 30 days: 43 in total" })).toBeInTheDocument();
    const languages = screen.getByRole("list", { name: "Languages" });
    expect(within(languages).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["TypeScript75.0%", "CSS18.8%", "Other6.3%"]);

    const rows = within(screen.getByRole("table", { name: "Contributors" })).getAllByRole("row");
    expect(rows[1]).toHaveTextContent("alice");
    expect(rows[1]).toHaveTextContent("30");
    expect(rows[2]).toHaveTextContent("bob");

    const events = screen.getByRole("list", { name: "Recent events" });
    expect(within(events).getAllByRole("listitem")[0]).toHaveTextContent("alice pushed main");
    expect(within(screen.getByRole("list", { name: "Latest snapshots" })).getByRole("link", { name: "Add login page" })).toHaveAttribute(
      "href",
      "/trees/acme/web/snapshots/snap-3",
    );

    expect(await screen.findByRole("heading", { name: "Web" })).toBeInTheDocument();
    expect(screen.getByText("marketing").tagName).toBe("STRONG");
    expect(screen.getByRole("link", { name: "docs" })).toHaveAttribute("href", "https://example.com/docs");
    expect(screen.getByText("npm run dev")).toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/events/recent")[0]?.url.searchParams.get("tree")).toBe("web");

    await user.click(screen.getByRole("radio", { name: "Last 7 days" }));
    await waitFor(() => expect(fetchMock.find(`GET ${T}/stats`).at(-1)?.url.searchParams.get("days")).toBe("7"));
  });
});

describe("history", () => {
  it("groups snapshots by day and compares two selected snapshots", async () => {
    const { user, router } = renderApp({ route: "/trees/acme/web/history", api: treeRoutes({ [`GET ${T}/snapshots`]: fx.snapshots }) });
    const history = await screen.findByRole("region", { name: "Snapshots on main" });
    expect(within(history).getAllByRole("heading", { level: 3 }).length).toBeGreaterThanOrEqual(2);
    expect(within(history).getByRole("link", { name: "Add login page" })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-3");

    const compare = screen.getByRole("button", { name: "Compare" });
    expect(compare).toBeDisabled();
    await user.click(screen.getByRole("checkbox", { name: "Select snapshot snap3 for comparison" }));
    await user.click(screen.getByRole("checkbox", { name: "Select snapshot snap2 for comparison" }));
    // A third pick replaces the oldest selection.
    await user.click(screen.getByRole("checkbox", { name: "Select snapshot snap1 for comparison" }));
    expect(screen.getByRole("checkbox", { name: "Select snapshot snap3 for comparison" })).not.toBeChecked();
    expect(screen.getByText("2 of 2 selected")).toBeInTheDocument();

    await user.click(compare);
    expect(router.state.location.pathname).toBe("/trees/acme/web/compare");
    expect(Object.fromEntries(new URLSearchParams(router.state.location.search))).toEqual({ from: "snap-1", to: "snap-2" });
  });
});

describe("snapshot detail", () => {
  it("shows metadata and the diff against the first parent", async () => {
    const { fetchMock } = renderApp({
      route: "/trees/acme/web/snapshots/snap-3",
      api: treeRoutes({ [`GET ${T}/snapshots/snap-3`]: fx.snapshotDetail, [`GET ${T}/compare`]: hx.compareResult }),
    });
    expect(await screen.findByRole("heading", { name: "Add login page" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy snapshot id" })).toBeInTheDocument();
    // Parent in the metadata, and again as the diff base.
    for (const link of screen.getAllByRole("link", { name: "snap2" })) expect(link).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-2");
    expect(screen.getByRole("link", { name: "Browse files at this snapshot" })).toHaveAttribute("href", "/trees/acme/web/files?snapshot=snap-3");
    expect(await screen.findByRole("table", { name: "Changes to src/app.ts" })).toBeInTheDocument();

    const q = fetchMock.find(`GET ${T}/compare`)[0]!.url.searchParams;
    expect(q.get("to")).toBe("snap-3");
    expect(q.has("from")).toBe(false);
  });
});

describe("compare view", () => {
  const api = () =>
    treeRoutes({
      [`GET ${T}/compare`]: hx.compareResult,
      [`GET ${T}/tags`]: hx.tags,
      [`GET ${T}/snapshots`]: fx.snapshots,
    });

  it("reads and writes from/to through the URL", async () => {
    const { user, router, fetchMock } = renderApp({ route: "/trees/acme/web/compare?from=snap-1&to=main", api: api() });
    expect(await screen.findByText("5 files changed")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Snapshots in comparison" })).getAllByRole("listitem")).toHaveLength(1);
    const first = fetchMock.find(`GET ${T}/compare`)[0]!.url.searchParams;
    expect([first.get("from"), first.get("to")]).toEqual(["snap-1", "main"]);

    await user.click(screen.getByRole("combobox", { name: "Target (to)" }));
    await user.click(await screen.findByRole("option", { name: "v1.0.0" }));
    expect(Object.fromEntries(new URLSearchParams(router.state.location.search))).toEqual({ from: "snap-1", to: "v1.0.0" });
    await waitFor(() => expect(fetchMock.find(`GET ${T}/compare`).at(-1)?.url.searchParams.get("to")).toBe("v1.0.0"));

    await user.click(screen.getByRole("button", { name: "Swap base and target" }));
    expect(Object.fromEntries(new URLSearchParams(router.state.location.search))).toEqual({ from: "v1.0.0", to: "snap-1" });

    await user.click(screen.getByRole("combobox", { name: "Base (from)" }));
    await user.click(await screen.findByRole("option", { name: "Parent of target" }));
    expect(Object.fromEntries(new URLSearchParams(router.state.location.search))).toEqual({ to: "snap-1" });
  });

  it("defaults the target to the default branch", async () => {
    const { fetchMock } = renderApp({ route: "/trees/acme/web/compare", api: api() });
    await screen.findByText("5 files changed");
    const q = fetchMock.find(`GET ${T}/compare`)[0]!.url.searchParams;
    expect(q.get("to")).toBe("main");
    expect(q.has("from")).toBe(false);
  });
});

describe("branches", () => {
  it("creates a branch from the dialog and reports conflicts inline", async () => {
    let status = 201;
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/branches",
      api: treeRoutes({
        [`GET ${T}/tags`]: [],
        [`GET ${T}/snapshots`]: fx.snapshots,
        [`POST ${T}/branches`]: (req) =>
          status === 409 ? apiError(409, "conflict", "exists") : json({ ...fx.branches[1]!, name: (req.body as { name: string }).name }, 201),
      }),
    });
    await screen.findByRole("table", { name: "Branches" });
    await user.click(screen.getByRole("button", { name: "New branch" }));
    const form = await screen.findByRole("form", { name: "Create branch" });

    await user.type(within(form).getByLabelText("Branch name"), "bad name");
    await user.click(within(form).getByRole("button", { name: "Create branch" }));
    expect(within(form).getByText("Branch names cannot contain spaces.")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${T}/branches`)).toHaveLength(0);

    status = 409;
    await user.clear(within(form).getByLabelText("Branch name"));
    await user.type(within(form).getByLabelText("Branch name"), "feature/login");
    await user.click(within(form).getByRole("button", { name: "Create branch" }));
    expect(await within(form).findByText("A branch named “feature/login” already exists.")).toBeInTheDocument();

    status = 201;
    await user.clear(within(form).getByLabelText("Branch name"));
    await user.type(within(form).getByLabelText("Branch name"), "feature/x");
    await user.click(within(form).getByRole("button", { name: "Create branch" }));
    expect(await screen.findByText("Branch created")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${T}/branches`).at(-1)?.body).toEqual({ name: "feature/x" });
    await waitFor(() => expect(screen.queryByRole("form", { name: "Create branch" })).not.toBeInTheDocument());
  });

  it("deletes a non-default branch after confirmation and links to compare", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/branches",
      api: treeRoutes({ [`DELETE ${T}/branches/feature/login`]: noContent() }),
    });
    await screen.findByRole("table", { name: "Branches" });
    expect(screen.queryByRole("button", { name: "Delete branch main" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Compare feature/login with main" })).toHaveAttribute(
      "href",
      "/trees/acme/web/compare?from=main&to=feature%2Flogin",
    );

    await user.click(screen.getByRole("button", { name: "Delete branch feature/login" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Delete branch" }));
    expect(await screen.findByText("Branch deleted")).toBeInTheDocument();
    expect(fetchMock.find(`DELETE ${T}/branches/feature/login`)).toHaveLength(1);
  });
});

describe("tags", () => {
  it("creates and deletes tags", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/tags",
      api: treeRoutes({
        [`GET ${T}/tags`]: hx.tags,
        [`GET ${T}/snapshots`]: fx.snapshots,
        [`POST ${T}/tags`]: (req) => json({ ...hx.tags[0]!, ...(req.body as object) }, 201),
        [`DELETE ${T}/tags/v1.0.0`]: noContent(),
      }),
    });
    await screen.findByRole("table", { name: "Tags" });
    await user.click(screen.getByRole("button", { name: "New tag" }));
    const form = await screen.findByRole("form", { name: "Create tag" });
    await user.type(within(form).getByLabelText("Tag name"), "v2.0.0");
    await user.type(within(form).getByLabelText(/Message/), "Second release");
    await user.click(within(form).getByRole("button", { name: "Create tag" }));
    expect(await screen.findByText("Tag created")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${T}/tags`)[0]?.body).toEqual({ name: "v2.0.0", target: "main", message: "Second release" });

    await user.click(screen.getByRole("button", { name: "Delete tag v1.0.0" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Delete tag" }));
    expect(await screen.findByText("Tag deleted")).toBeInTheDocument();
    expect(fetchMock.find(`DELETE ${T}/tags/v1.0.0`)).toHaveLength(1);
  });
});
