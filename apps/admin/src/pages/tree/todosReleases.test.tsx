import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Release } from "@/api/v05tree";
import { FakeEventSource } from "@/test/fakeEventSource";
import * as fx from "@/test/fixtures";
import * as v5 from "@/test/fixtures.v05";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";
const TODOS = "/trees/acme/web/todos";
const RELEASES = "/trees/acme/web/releases";

describe("TODOs tab", () => {
  it("lists open TODOs and filters by status, branch and assignee", async () => {
    const { user, fetchMock } = renderApp({ route: TODOS, api: treeRoutes({ [`GET ${T}/todos`]: { todos: v5.todos } }) });
    const table = await screen.findByRole("table", { name: "TODOs" });
    const rows = within(table).getAllByRole("row").slice(1);
    // Newest number first.
    expect(rows[0]).toHaveTextContent("#2");
    expect(rows[1]).toHaveTextContent("Update API client");
    expect(within(rows[1]!).getByText("blocking")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("high")).toHaveAttribute("data-priority", "high");
    expect(within(rows[0]!).getByText("In progress").closest("[data-status]")).toHaveAttribute("data-status", "in_progress");
    expect(fetchMock.find(`GET ${T}/todos`)[0]?.url.searchParams.get("status")).toBe("open");

    await user.click(screen.getByRole("combobox", { name: "Status" }));
    await user.click(await screen.findByRole("option", { name: "All" }));
    await waitFor(() => expect(fetchMock.find(`GET ${T}/todos`).at(-1)?.url.searchParams.has("status")).toBe(false));

    await user.click(screen.getByRole("combobox", { name: "Target branch filter" }));
    await user.click(await screen.findByRole("option", { name: "feature/login" }));
    await waitFor(() => expect(fetchMock.find(`GET ${T}/todos`).at(-1)?.url.searchParams.get("branch")).toBe("feature/login"));

    await user.click(screen.getByRole("switch", { name: "Assigned to me" }));
    await waitFor(() => expect(fetchMock.find(`GET ${T}/todos`).at(-1)?.url.searchParams.get("assignee")).toBe("alice"));
  });

  it("claims, completes and cancels a TODO", async () => {
    const { user, fetchMock } = renderApp({
      route: TODOS,
      api: treeRoutes({
        [`GET ${T}/todos`]: v5.todos,
        [`POST ${T}/todos/1/claim`]: v5.todo(1, { status: "in_progress" }),
        [`POST ${T}/todos/1/complete`]: v5.todo(1, { status: "done" }),
        [`POST ${T}/todos/1/cancel`]: v5.todo(1, { status: "cancelled" }),
      }),
    });
    await screen.findByRole("table", { name: "TODOs" });
    // #2 is already assigned to alice: no claim button.
    expect(screen.queryByRole("button", { name: "Claim TODO #2" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Claim TODO #1" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/todos/1/claim`)).toHaveLength(1));

    await user.click(screen.getByRole("button", { name: "Complete TODO #1" }));
    const dialog = await screen.findByRole("dialog", { name: /Complete #1/ });
    await user.type(within(dialog).getByLabelText(/Completing snapshot/), "snap-9");
    await user.click(within(dialog).getByRole("button", { name: "Mark done" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/todos/1/complete`)[0]?.body).toEqual({ snapshot: "snap-9" }));

    await user.click(screen.getByRole("button", { name: "Cancel TODO #1" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Cancel #1?" });
    expect(fetchMock.find(`POST ${T}/todos/1/cancel`)).toHaveLength(0);
    await user.click(within(confirm).getByRole("button", { name: "Cancel TODO" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/todos/1/cancel`)).toHaveLength(1));
  });

  it("creates a TODO by hand", async () => {
    const { user, fetchMock } = renderApp({
      route: TODOS,
      api: treeRoutes({
        [`GET ${T}/todos`]: [],
        [`POST ${T}/todos`]: (req) => json({ ...v5.todo(3), ...(req.body as object) }, 201),
      }),
    });
    expect(await screen.findByText("No open TODOs")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New TODO" }));
    const dialog = await screen.findByRole("dialog", { name: "New TODO" });
    await user.click(within(dialog).getByRole("button", { name: "Create TODO" }));
    expect(within(dialog).getByText("A title is required.")).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText("Title"), "Write docs");
    await user.type(within(dialog).getByLabelText(/Details/), "For the new flag");
    await user.click(within(dialog).getByRole("combobox", { name: "Priority" }));
    await user.click(await screen.findByRole("option", { name: "critical" }));
    await user.click(within(dialog).getByRole("switch", { name: "Blocking" }));
    await user.click(within(dialog).getByRole("button", { name: "Create TODO" }));

    await waitFor(() => expect(fetchMock.find(`POST ${T}/todos`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/todos`)[0]?.body).toEqual({
      title: "Write docs",
      details: "For the new flag",
      priority: "critical",
      blocking: true,
      target_branch: "main",
    });
  });

  it("refreshes on todo events", async () => {
    const { fetchMock } = renderApp({ route: TODOS, api: treeRoutes({ [`GET ${T}/todos`]: v5.todos }) });
    await screen.findByRole("table", { name: "TODOs" });
    const before = fetchMock.find(`GET ${T}/todos`).length;
    act(() => {
      const es = FakeEventSource.latest();
      es.open();
      es.emit("todo.created", { type: "todo.created", tenant_id: fx.tenant.id, tree_id: "tree-web", at: new Date().toISOString() });
    });
    await waitFor(() => expect(fetchMock.find(`GET ${T}/todos`).length).toBeGreaterThan(before));
  });

  it("explains when the server has no TODO service", async () => {
    renderApp({ route: TODOS, api: treeRoutes({ [`GET ${T}/todos`]: apiError(501, "not_implemented", "todos are not configured") }) });
    expect(await screen.findByText("Not available on this server")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New TODO" })).not.toBeInTheDocument();
  });
});

describe("releases tab", () => {
  it("lists releases with notes, badges and assets", async () => {
    renderApp({ route: RELEASES, api: treeRoutes({ [`GET ${T}/releases`]: [v5.release] }) });
    const card = await screen.findByRole("article", { name: "Release First stable" });
    expect(within(card).getByText("pre-release")).toBeInTheDocument();
    expect(within(card).getByRole("heading", { name: "Highlights" })).toBeInTheDocument();
    expect(within(card).getByRole("link", { name: "snap3" })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-3");
    const assets = within(card).getByRole("table", { name: "Assets of v1.0.0" });
    expect(assets).toHaveTextContent("web.tar.gz");
    expect(assets).toHaveTextContent("2.0 KiB");
  });

  it("creates a release with a new tag at a revision", async () => {
    const { user, fetchMock } = renderApp({
      route: RELEASES,
      api: treeRoutes({
        [`GET ${T}/releases`]: [],
        [`GET ${T}/tags`]: [{ tree_id: "tree-web", name: "v0.9.0", target: "snap-2", message: "", tagger: "alice", created_at: new Date().toISOString() }],
        [`GET ${T}/snapshots`]: fx.snapshots,
        [`POST ${T}/releases`]: (req) => json({ ...v5.release, ...(req.body as object), assets: null }, 201),
      }),
    });
    expect(await screen.findByText("No releases")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New release" }));
    const dialog = await screen.findByRole("dialog", { name: "New release" });
    await user.type(within(dialog).getByLabelText("Tag"), "v1.1.0");
    expect(within(dialog).getByText("New tag: created at the revision below.")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText(/Release notes/), "Notes");
    await user.click(within(dialog).getByRole("switch", { name: "Draft" }));
    await user.click(within(dialog).getByRole("button", { name: "Create release" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/releases`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/releases`)[0]?.body).toEqual({
      tag: "v1.1.0",
      name: "v1.1.0",
      notes: "Notes",
      draft: true,
      prerelease: false,
      snapshot: "main",
    });
  });

  it("uses an existing tag without a snapshot", async () => {
    const { user, fetchMock } = renderApp({
      route: RELEASES,
      api: treeRoutes({
        [`GET ${T}/releases`]: [],
        [`GET ${T}/tags`]: [{ tree_id: "tree-web", name: "v0.9.0", target: "snap-2", message: "", tagger: "alice", created_at: new Date().toISOString() }],
        [`POST ${T}/releases`]: apiError(409, "conflict", "release exists"),
      }),
    });
    await screen.findByText("No releases");
    await user.click(screen.getByRole("button", { name: "New release" }));
    const dialog = await screen.findByRole("dialog", { name: "New release" });
    await user.type(within(dialog).getByLabelText("Tag"), "v0.9.0");
    expect(await within(dialog).findByText("Existing tag → snapshot snap2.")).toBeInTheDocument();
    expect(within(dialog).queryByLabelText("Create tag at")).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Create release" }));
    expect(await within(dialog).findByText("A release for tag “v0.9.0” already exists.")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${T}/releases`)[0]?.body).not.toHaveProperty("snapshot");
  });

  it("edits, uploads, downloads and deletes", async () => {
    let current: Release = v5.release;
    const { user, fetchMock } = renderApp({
      route: RELEASES,
      api: treeRoutes({
        [`GET ${T}/releases`]: () => json([current]),
        [`PATCH ${T}/releases/v1.0.0`]: (req) => {
          current = { ...current, ...(req.body as object) };
          return json(current);
        },
        [`PUT ${T}/releases/v1.0.0/assets/notes.txt`]: noContent(),
        [`GET ${T}/releases/v1.0.0/assets/web.tar.gz`]: () => new Response("bytes"),
        [`DELETE ${T}/releases/v1.0.0`]: noContent(),
      }),
    });
    await screen.findByRole("article", { name: "Release First stable" });

    await user.click(screen.getByRole("button", { name: "Edit release v1.0.0" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit release v1.0.0" });
    const name = within(dialog).getByLabelText(/^Name/);
    await user.clear(name);
    await user.type(name, "Stable");
    await user.click(within(dialog).getByRole("switch", { name: "Pre-release" }));
    await user.click(within(dialog).getByRole("button", { name: "Save release" }));
    expect(await screen.findByRole("article", { name: "Release Stable" })).toBeInTheDocument();
    expect(fetchMock.find(`PATCH ${T}/releases/v1.0.0`)[0]?.body).toEqual({
      name: "Stable",
      notes: v5.release.notes,
      draft: false,
      prerelease: false,
    });

    const file = new File(["hello"], "notes.txt", { type: "text/plain" });
    await user.upload(screen.getByLabelText("Upload assets to v1.0.0"), file);
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/releases/v1.0.0/assets/notes.txt`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/releases/v1.0.0/assets/notes.txt`)[0]?.headers.get("Content-Type")).toBe("text/plain");

    await user.click(screen.getByRole("button", { name: "Download web.tar.gz" }));
    await waitFor(() => expect(fetchMock.find(`GET ${T}/releases/v1.0.0/assets/web.tar.gz`)).toHaveLength(1));
    expect(fetchMock.find(`GET ${T}/releases/v1.0.0/assets/web.tar.gz`)[0]?.headers.get("Authorization")).toBe("Bearer test-token");

    await user.click(screen.getByRole("button", { name: "Delete release v1.0.0" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Delete release" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${T}/releases/v1.0.0`)).toHaveLength(1));
  });

  it("explains when releases are not configured", async () => {
    renderApp({ route: RELEASES, api: treeRoutes({ [`GET ${T}/releases`]: apiError(501, "not_implemented", "releases are not configured") }) });
    expect(await screen.findByText("Not available on this server")).toBeInTheDocument();
    expect(screen.getByText("Releases are not enabled on this W0rkTree server.")).toBeInTheDocument();
  });
});
