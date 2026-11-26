import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { forcePushes, normReflog, type ReflogEntry } from "@/api/historyV2";
import * as fx from "@/test/fixtures";
import { json } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

// v0.8 history-v2 (§16.10): no_force_push toggle and force-push markers.

const T = "/api/v1/trees/acme/web";

const forcePush: ReflogEntry = {
  tree_id: "tree-web",
  branch: "main",
  old_tip: "old-tip-1234567",
  new_tip: "snap-3",
  kind: "force_push",
  forced: true,
  bypass: true,
  bypassed: ["force push past require_linear"],
  actor: "olivia",
  actor_id: "acc-olivia",
  count: 1,
  at: new Date(Date.now() - 5 * 60_000).toISOString(),
};

const plainPush: ReflogEntry = { ...forcePush, kind: "push", forced: false, bypass: false, bypassed: [], new_tip: "snap-2", old_tip: "snap-1", actor: "bob" };

describe("reflog helpers", () => {
  it("normalises lists and picks force pushes", () => {
    const list = normReflog({ entries: [{ ...forcePush, bypassed: null }, plainPush] });
    expect(list[0]?.bypassed).toEqual([]);
    expect(forcePushes(list).map((e) => e.new_tip)).toEqual(["snap-3"]);
    expect(normReflog(null)).toEqual([]);
  });
});

describe("protections tab (v0.8 force pushes)", () => {
  it("saves the no_force_push toggle", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({ [`PUT ${T}/protections`]: (req) => json(req.body), [`GET ${T}/tag-protections`]: [] }),
    });
    await user.click(await screen.findByRole("button", { name: "Add protection" }));
    const form = await screen.findByRole("form", { name: "Add protection" });
    await user.type(within(form).getByLabelText("Branch pattern"), "release/*");
    const toggle = within(form).getByRole("switch", { name: "Prevent force pushes" });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    await user.click(screen.getByRole("button", { name: "Add protection" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/protections`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/protections`)[0]?.body).toMatchObject({ pattern: "release/*", no_force_push: true });
  });

  it("shows and edits the rule of existing protections", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({
        [`GET ${T}/protections`]: [{ ...fx.protection, no_force_push: true }],
        [`PUT ${T}/protections`]: (req) => json(req.body),
        [`GET ${T}/tag-protections`]: [],
      }),
    });
    const table = await screen.findByRole("table", { name: "Branch protections" });
    expect(await within(table).findByText("no force push")).toBeInTheDocument();
    await user.click(within(table).getByRole("button", { name: "Edit main" }));
    const form = await screen.findByRole("form", { name: "Edit protection" });
    const toggle = within(form).getByRole("switch", { name: "Prevent force pushes" });
    expect(toggle).toBeChecked();
    await user.click(toggle);
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/protections`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/protections`)[0]?.body).not.toHaveProperty("no_force_push");
  });
});

describe("history tab force-push markers", () => {
  it("lists force pushes and marks the snapshot they made the tip", async () => {
    const { fetchMock } = renderApp({
      route: "/trees/acme/web/history",
      api: treeRoutes({ [`GET ${T}/snapshots`]: fx.snapshots, [`GET ${T}/reflog`]: { entries: [forcePush, plainPush] } }),
    });
    const pushes = await screen.findByRole("region", { name: "Force pushes on main" });
    expect(pushes).toHaveTextContent("1 force push rewrote main");
    expect(pushes).toHaveTextContent("olivia");
    expect(within(pushes).getByTitle("Replaced tip old-tip-1234567")).toHaveClass("line-through");
    expect(within(pushes).getByRole("link", { name: "snap3" })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-3");
    expect(within(pushes).getByText("bypassed protection")).toBeInTheDocument();
    expect(screen.getAllByText("force-pushed")).toHaveLength(2); // the list row and the snapshot badge
    expect(fetchMock.find(`GET ${T}/reflog`)[0]?.url.searchParams.get("branch")).toBe("main");
  });

  it("restores the tip a force push replaced after confirmation", async () => {
    const restored: ReflogEntry = { ...forcePush, kind: "restore", old_tip: "snap-3", new_tip: "old-tip-1234567", actor: "alice" };
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/history",
      api: treeRoutes({
        [`GET ${T}/snapshots`]: fx.snapshots,
        [`GET ${T}/reflog`]: { entries: [forcePush, plainPush] },
        [`POST ${T}/reflog/restore`]: (req) =>
          json({ branch: "main", old_tip: "snap-3", new_tip: "old-tip-1234567", forced: true, bypass: false, bypassed: [], entry: restored, req: req.body }),
      }),
    });
    const pushes = await screen.findByRole("region", { name: "Force pushes on main" });
    await user.click(await within(pushes).findByRole("button", { name: "Restore main to oldtip12" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Restore main to oldtip12?");
    expect(dialog).toHaveTextContent("main moves from snap3 back to oldtip12");
    expect(fetchMock.find(`POST ${T}/reflog/restore`)).toHaveLength(0);
    await user.click(within(dialog).getByRole("button", { name: "Restore main" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/reflog/restore`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/reflog/restore`)[0]?.body).toEqual({ branch: "main", tip: "old-tip-1234567", expected_tip: "snap-3" });
    expect(await screen.findByText("Branch restored")).toBeInTheDocument();
  });

  it("reports a refused restore", async () => {
    const { user } = renderApp({
      route: "/trees/acme/web/history",
      api: treeRoutes({
        [`GET ${T}/snapshots`]: fx.snapshots,
        [`GET ${T}/reflog`]: { entries: [forcePush] },
        [`POST ${T}/reflog/restore`]: () =>
          json({ error: { code: "branch_protection", message: "branch 'main' is protected against force pushes (no_force_push)" } }, 403),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Restore main to oldtip12" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Restore main" }));
    expect(await screen.findByText(/protected against force pushes/)).toBeInTheDocument();
  });

  it("offers no restore without admin rights or for the current tip", async () => {
    renderApp({
      route: "/trees/acme/web/history",
      api: treeRoutes({
        "GET /api/v1/auth/me": fx.memberMe,
        [`GET ${T}/snapshots`]: fx.snapshots,
        [`GET ${T}/reflog`]: { entries: [forcePush] },
      }),
    });
    const pushes = await screen.findByRole("region", { name: "Force pushes on main" });
    expect(within(pushes).queryByRole("button", { name: /Restore/ })).not.toBeInTheDocument();
  });

  it("labels restores and hides restoring to the current tip", async () => {
    const restore: ReflogEntry = { ...forcePush, kind: "restore", old_tip: "snap-3", bypass: false };
    renderApp({
      route: "/trees/acme/web/history",
      api: treeRoutes({ [`GET ${T}/snapshots`]: fx.snapshots, [`GET ${T}/reflog`]: { entries: [restore] } }),
    });
    const pushes = await screen.findByRole("region", { name: "Force pushes on main" });
    expect(await within(pushes).findByText("restored")).toBeInTheDocument();
    expect(within(pushes).queryByRole("button", { name: /Restore/ })).not.toBeInTheDocument();
  });

  it("shows no markers when the server has no reflog", async () => {
    renderApp({ route: "/trees/acme/web/history", api: treeRoutes({ [`GET ${T}/snapshots`]: fx.snapshots }) });
    expect(await screen.findByRole("region", { name: "Snapshots on main" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Force pushes on main" })).not.toBeInTheDocument();
  });
});
