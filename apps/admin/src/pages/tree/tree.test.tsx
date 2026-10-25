import { act, screen, within, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { installDomPolyfills } from "@/test/dom.history";
import { FakeEventSource } from "@/test/fakeEventSource";
import * as fx from "@/test/fixtures";
import { json } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";

installDomPolyfills();

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

  it("browses the manifest as a directory tree and shows text with line numbers", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/files",
      api: treeRoutes({
        [`GET ${T}/snapshots/snap-3`]: fx.snapshotDetail,
        [`GET ${T}/blob/snap-3/src/app.ts`]: new Response("const a = 1;\nexport default a;\n"),
      }),
    });

    const root = await screen.findByRole("table", { name: "Files in root" });
    expect(within(root).getByRole("link", { name: "src/" })).toBeInTheDocument();
    expect(within(root).getByRole("link", { name: /README\.md/ })).toBeInTheDocument();

    await user.click(within(root).getByRole("link", { name: "src/" }));
    const src = await screen.findByRole("table", { name: "Files in src" });
    expect(within(src).getByRole("link", { name: "lib/" })).toBeInTheDocument();

    await user.click(within(src).getByRole("link", { name: /app\.ts/ }));
    const code = await screen.findByRole("region", { name: "Contents of src/app.ts" });
    expect(within(code).getByText("const a = 1;")).toBeInTheDocument();
    expect(within(code).getByText("export default a;")).toBeInTheDocument();
    expect(within(code).getByText("1")).toBeInTheDocument();
    expect(within(code).getByText("2")).toBeInTheDocument();
    expect(screen.getByText("· 2 lines")).toBeInTheDocument();
    expect(fetchMock.find(`GET ${T}/blob/snap-3/src/app.ts`)[0]?.headers.get("Authorization")).toBe("Bearer test-token");
  });

  it("offers binary files as a download instead of a preview", async () => {
    renderApp({
      route: "/trees/acme/web/files?snapshot=snap-3&path=assets&file=assets%2Flogo.png",
      api: treeRoutes({
        [`GET ${T}/snapshots/snap-3`]: fx.snapshotDetail,
        [`GET ${T}/blob/snap-3/assets/logo.png`]: new Response(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x00, 0x1a])),
      }),
    });
    expect(await screen.findByText("Binary file")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeEnabled();
  });

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

  it("saves a branch protection via PUT", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({ [`PUT ${T}/protections`]: (req) => json(req.body) }),
    });
    await user.click(await screen.findByRole("button", { name: "Add protection" }));
    const form = screen.getByRole("form", { name: "Add protection" });
    await user.type(within(form).getByLabelText("Branch pattern"), "release/*");
    await user.clear(within(form).getByLabelText("Required approvals"));
    await user.type(within(form).getByLabelText("Required approvals"), "3");
    await user.type(within(form).getByLabelText("Allowed pushers"), "alice, bob");
    await user.click(within(form).getByLabelText("Prevent deletion"));
    await user.click(within(form).getByRole("button", { name: "Add protection" }));

    await screen.findByText("Protection added");
    expect(fetchMock.find(`PUT ${T}/protections`)[0]?.body).toEqual({
      tree_id: "tree-web",
      pattern: "release/*",
      no_direct_push: true,
      required_approvals: 3,
      allowed_pushers: ["alice", "bob"],
      no_delete: false,
    });
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
