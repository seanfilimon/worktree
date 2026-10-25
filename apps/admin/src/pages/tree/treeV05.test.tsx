import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as ffx from "@/test/fixtures.files";
import * as hx from "@/test/fixtures.history";
import * as v5 from "@/test/fixtures.v05";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";

describe("protections tab (v0.5 rules)", () => {
  it("saves CI, signing, linear and TODO requirements", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({ [`PUT ${T}/protections`]: (req) => json(req.body), [`GET ${T}/tag-protections`]: [] }),
    });
    await user.click(await screen.findByRole("button", { name: "Add protection" }));
    const form = await screen.findByRole("form", { name: "Add protection" });
    await user.type(within(form).getByLabelText("Branch pattern"), "release/*");
    await user.click(within(form).getByRole("switch", { name: "Require CI to pass" }));
    await user.type(within(form).getByLabelText("Required checks"), "ci/build{Enter}ci/test,");
    await user.click(within(form).getByRole("switch", { name: "Require signed snapshots" }));
    await user.click(within(form).getByRole("switch", { name: "Require linear history" }));
    await user.click(within(form).getByRole("switch", { name: "Block on open TODOs" }));
    await user.click(screen.getByRole("button", { name: "Add protection" }));

    await waitFor(() => expect(fetchMock.find(`PUT ${T}/protections`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/protections`)[0]?.body).toEqual({
      tree_id: "tree-web",
      pattern: "release/*",
      no_direct_push: true,
      required_approvals: 1,
      allowed_pushers: [],
      no_delete: true,
      require_ci_pass: true,
      required_checks: ["ci/build", "ci/test"],
      require_signed: true,
      require_linear: true,
      block_on_todos: true,
    });
  });

  it("shows the v0.5 rules of existing protections", async () => {
    renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({
        [`GET ${T}/protections`]: [{ ...fx.protection, require_ci_pass: true, required_checks: ["ci/build"], require_signed: true, block_on_todos: true }],
        [`GET ${T}/tag-protections`]: [],
      }),
    });
    const table = await screen.findByRole("table", { name: "Branch protections" });
    expect(within(table).getByText("CI: ci/build")).toBeInTheDocument();
    expect(within(table).getByText("signed only")).toBeInTheDocument();
    expect(within(table).getByText("blocks on TODOs")).toBeInTheDocument();
    expect(within(table).queryByText("linear")).not.toBeInTheDocument();
  });

  it("lists, adds and deletes tag protections", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({
        [`GET ${T}/tag-protections`]: [v5.tagProtection],
        [`GET ${T}/tags`]: [{ tree_id: "tree-web", name: "v1.0.0", target: "snap-3", message: "", tagger: "alice", created_at: new Date().toISOString() }],
        [`PUT ${T}/tag-protections`]: (req) => json(req.body),
        [`DELETE ${T}/tag-protections`]: noContent(),
      }),
    });
    const table = await screen.findByRole("table", { name: "Tag protections" });
    const row = within(table).getByText("v*").closest("tr")!;
    expect(row).toHaveTextContent("no move");
    expect(row).toHaveTextContent("no delete");
    expect(row).toHaveTextContent("release-bot");
    expect(await within(row).findByText("1 tag")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Add tag protection" }));
    const form = await screen.findByRole("form", { name: "Add tag protection" });
    await user.type(within(form).getByLabelText("Tag pattern"), "release-*");
    await user.click(within(form).getByRole("switch", { name: "Prevent moving" }));
    await user.type(within(form).getByLabelText("Allowed taggers"), "bob{Enter}");
    await user.click(screen.getByRole("button", { name: "Add tag protection" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/tag-protections`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/tag-protections`)[0]?.body).toEqual({
      tree_id: "tree-web",
      pattern: "release-*",
      no_delete: true,
      no_move: false,
      allowed_taggers: ["bob"],
    });

    await user.click(screen.getByRole("button", { name: "Delete tag protection v*" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${T}/tag-protections`)).toHaveLength(1));
    expect(fetchMock.find(`DELETE ${T}/tag-protections`)[0]?.url.searchParams.get("pattern")).toBe("v*");
  });

  it("explains when tag protections are not available", async () => {
    renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({ [`GET ${T}/tag-protections`]: apiError(501, "not_implemented", "not configured") }),
    });
    expect(await screen.findByText("Tag protections are not enabled on this W0rkTree server.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add tag protection" })).not.toBeInTheDocument();
  });
});

describe("snapshot detail (v0.5)", () => {
  it("shows the verified signature, revert link and dependencies with their TODOs", async () => {
    const { fetchMock } = renderApp({
      route: "/trees/acme/web/snapshots/snap-3",
      api: treeRoutes({
        [`GET ${T}/snapshots/snap-3`]: v5.signedDetail,
        [`GET ${T}/compare`]: hx.compareResult,
        [`GET ${T}/dependencies`]: {
          outgoing: [
            {
              snapshot_id: "snap-3",
              dependency: v5.signedDetail.snapshot.dependencies![0],
              todo: { tenant: "acme", tree: "api", number: 4, status: "in_progress" },
            },
          ],
          incoming: null,
        },
      }),
    });
    expect(await screen.findByText("Verified")).toBeInTheDocument();
    expect(screen.getByText("key key-7")).toBeInTheDocument();
    // Header badge too.
    expect(screen.getByText("verified")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "snap1" })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-1");
    expect(screen.getByText(/2 restricted paths hidden/)).toBeInTheDocument();

    const deps = screen.getByRole("table", { name: "Dependencies" });
    expect(deps).toHaveTextContent("acme/api");
    expect(deps).toHaveTextContent("Update API client");
    expect(within(deps).getByText("blocking")).toBeInTheDocument();
    expect(await within(deps).findByRole("link", { name: "acme/api#4" })).toHaveAttribute("href", "/trees/acme/api/todos");
    expect(within(deps).getByText("In progress")).toBeInTheDocument();
    expect(fetchMock.find(`GET ${T}/dependencies`)[0]?.url.searchParams.get("branch")).toBe("main");
  });

  it("marks unsigned and signed-but-unverified snapshots", async () => {
    renderApp({
      route: "/trees/acme/web/snapshots/snap-3",
      api: treeRoutes({ [`GET ${T}/snapshots/snap-3`]: fx.snapshotDetail, [`GET ${T}/compare`]: hx.compareResult }),
    });
    expect(await screen.findByText("Unsigned")).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "Dependencies" })).not.toBeInTheDocument();
  });

  it("shows signature and revert badges in the history list", async () => {
    renderApp({
      route: "/trees/acme/web/history",
      api: treeRoutes({
        [`GET ${T}/snapshots`]: [{ ...fx.snapshots[0], signature: "c2ln", reverts: "snap-1" }, { ...fx.snapshots[1], verified: true }, fx.snapshots[2]],
      }),
    });
    const list = await screen.findByRole("region", { name: "Snapshots on main" });
    const items = within(list).getAllByRole("listitem");
    expect(within(items[0]!).getByText("signed")).toBeInTheDocument();
    expect(within(items[0]!).getByText("revert")).toBeInTheDocument();
    expect(within(items[1]!).getByText("verified")).toBeInTheDocument();
    expect(within(items[2]!).queryByText(/signed|verified/)).not.toBeInTheDocument();
  });
});

describe("files tab (v0.5)", () => {
  it("notes paths hidden by access policies or license rules", async () => {
    renderApp({
      route: "/trees/acme/web/files",
      api: treeRoutes({ [`GET ${T}/snapshots/snap-3`]: { ...ffx.filesDetail, restricted_count: 3 } }),
    });
    const notice = await screen.findByRole("alert", { name: "Restricted paths" });
    expect(notice).toHaveTextContent("3 paths are hidden");
  });

  it("shows no notice without restricted paths", async () => {
    renderApp({ route: "/trees/acme/web/files", api: treeRoutes({ [`GET ${T}/snapshots/snap-3`]: ffx.filesDetail }) });
    await screen.findByRole("navigation", { name: "File path" });
    expect(screen.queryByRole("alert", { name: "Restricted paths" })).not.toBeInTheDocument();
  });
});
