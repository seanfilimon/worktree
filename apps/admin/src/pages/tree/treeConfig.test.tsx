import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import { apiError, json } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";
import { diagnosticLocation, normState, ownerError, protectionSummary, type ConfigDiagnostic, type TreeConfigState } from "./treeConfigApi";

const C = "/api/v1/trees/acme/web/config";
const B = "/api/v1/trees/acme/web/branches";

const warning: ConfigDiagnostic = {
  code: "W3101",
  severity: "warning",
  file: ".wt-tree/protections.toml",
  line: 6,
  col: 1,
  message: "a protection for hotfix/* is managed through the API; this entry is ignored",
};

const state: TreeConfigState = {
  tree_id: "tree-web",
  default_branch: "main",
  applied: {
    snapshot_id: "snap-3aaaaaaaaaaaa",
    branch: "main",
    applied_by: "alice",
    applied_at: "2026-10-20T10:00:00Z",
    files: [
      { path: ".wt-tree/branches.toml", hash: "ab", size: 120 },
      { path: ".wt-tree/protections.toml", hash: "cd", size: 80 },
    ],
    branches: [{ name: "main", description: "Production web app", owner: "team:platform" }, { name: "release/1.0" }],
    protections: [
      { tree_id: "tree-web", pattern: "release/*", no_direct_push: true, required_approvals: 2, allowed_pushers: null, no_delete: true },
      { tree_id: "tree-web", pattern: "hotfix/*", no_direct_push: false, required_approvals: 0, allowed_pushers: [], no_delete: true },
    ],
    managed_patterns: ["release/*"],
    access_hash: "0123456789abcdef",
    changes: ["protection release/*: create", "branch main: description set, owner team:platform"],
    warnings: [warning],
  },
  last_attempt: {
    snapshot_id: "snap-9bbbbbbbbbbb",
    branch: "main",
    by: "bob",
    at: "2026-10-21T10:00:00Z",
    accepted: false,
    errors: [{ code: "E3008", severity: "error", file: ".wt-tree/branches.toml", line: 3, col: 1, message: "owner user:ghost does not exist in this tenant" }],
    warnings: [],
  },
};

describe("tree config helpers", () => {
  it("normalises null lists and formats", () => {
    const s = normState({ tree_id: "t", default_branch: "main", applied: { ...state.applied!, files: null as never, warnings: null as never }, last_attempt: null });
    expect(s.applied?.files).toEqual([]);
    expect(s.applied?.warnings).toEqual([]);
    expect(normState(null)).toEqual({ tree_id: "", default_branch: "", applied: null, last_attempt: null });
    expect(diagnosticLocation(warning)).toBe(".wt-tree/protections.toml:6:1");
    expect(diagnosticLocation({ ...warning, line: 0, col: 0 })).toBe(".wt-tree/protections.toml");
    expect(ownerError("")).toBe("");
    expect(ownerError("team:core")).toBe("");
    expect(ownerError("user:Alice")).toMatch(/user:NAME/);
    expect(ownerError("alice")).toMatch(/team:SLUG/);
    expect(protectionSummary(state.applied!.protections[0]!)).toBe("no direct push · 2 approvals · no delete");
    expect(protectionSummary({ ...state.applied!.protections[1]!, no_delete: false })).toBe("no restrictions");
    expect(protectionSummary({ ...state.applied!.protections[1]!, merge_methods: ["squash"], no_force_push: true })).toBe("no delete · no force push · merge: squash");
  });
});

describe("config tab", () => {
  it("shows the applied configuration and the rejected attempt", async () => {
    renderApp({ route: "/trees/acme/web/config", api: treeRoutes({ [`GET ${C}`]: state }) });
    expect(await screen.findByText("Applied configuration")).toBeInTheDocument();
    const rejected = screen.getByRole("alert");
    expect(rejected).toHaveTextContent("The last configuration push was rejected");
    const errors = within(rejected).getByRole("table", { name: "Errors of the last attempt" });
    expect(errors).toHaveTextContent(".wt-tree/branches.toml:3:1");
    expect(errors).toHaveTextContent("E3008");
    expect(errors).toHaveTextContent("owner user:ghost does not exist");

    const branches = screen.getByRole("table", { name: "Declared branches" });
    expect(within(branches).getAllByRole("row")).toHaveLength(3);
    expect(branches).toHaveTextContent("team:platform");
    expect(branches).toHaveTextContent("Production web app");
    const prots = within(screen.getByRole("table", { name: "Declared protections" })).getAllByRole("row");
    expect(prots[1]).toHaveTextContent("release/*");
    expect(prots[1]).toHaveTextContent("config");
    expect(prots[2]).toHaveTextContent("API (ignored)");
    expect(screen.getByRole("table", { name: "Warnings" })).toHaveTextContent("W3101");
    expect(screen.getByText(/Tree access document applied/)).toHaveTextContent("0123456789ab");
    expect(screen.getByText("protection release/*: create")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: ".wt-tree/branches.toml" })).toHaveAttribute("href", expect.stringContaining("snap-3aaaaaaaaaaaa"));
  });

  it("explains how to start and that the server lacks the feature", async () => {
    renderApp({ route: "/trees/acme/web/config", api: treeRoutes({ [`GET ${C}`]: { tree_id: "tree-web", default_branch: "main", applied: null, last_attempt: null } }) });
    expect(await screen.findByText("No configuration applied yet")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows a notice when the server has no config-as-code", async () => {
    renderApp({ route: "/trees/acme/web/config", api: treeRoutes({ [`GET ${C}`]: apiError(501, "not_implemented", "config-as-code is not available") }) });
    expect(await screen.findByText("Configuration as code is not available on this server")).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Validate configuration" })).not.toBeInTheDocument();
  });

  it("validates files without pushing", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/config",
      api: treeRoutes({
        [`GET ${C}`]: { ...state, applied: null, last_attempt: null },
        [`POST ${C}/validate`]: json({
          valid: false,
          diagnostics: [{ code: "E3004", severity: "error", file: ".wt-tree/branches.toml", line: 2, col: 1, message: "branch name contains whitespace" }],
          branches: null,
          protections: null,
          access_changes: null,
        }),
      }),
    });
    const form = await screen.findByRole("form", { name: "Validate configuration" });
    expect(within(form).getByRole("button", { name: "Validate" })).toBeDisabled();
    await user.type(within(form).getByLabelText(".wt-tree/branches.toml"), '[[[[branch]]{Enter}name = "a b"');
    await user.click(within(form).getByRole("radio", { name: /protections\.toml/ }));
    expect(within(form).getByLabelText(".wt-tree/protections.toml")).toHaveValue("");
    await user.click(within(form).getByRole("button", { name: "Validate 1 file" }));
    await waitFor(() => expect(fetchMock.find(`POST ${C}/validate`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${C}/validate`)[0]?.body).toEqual({ files: { ".wt-tree/branches.toml": '[[branch]]\nname = "a b"' } });
    expect(await within(form).findByRole("status")).toHaveTextContent("1 error(s)");
    expect(within(form).getByRole("table", { name: "Validation results" })).toHaveTextContent(".wt-tree/branches.toml:2:1");
  });
});

describe("branch description and owner", () => {
  const branches = [{ ...fx.branches[0]!, description: "Production web app", owner: "team:platform" }, fx.branches[1]!];

  it("shows and edits description and owner", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/branches",
      api: treeRoutes({
        [`GET ${B}`]: branches,
        [`PATCH ${B}/feature/login`]: (req) => json({ ...fx.branches[1], ...(req.body as object) }),
      }),
    });
    const table = await screen.findByRole("table", { name: "Branches" });
    expect(table).toHaveTextContent("Production web app");
    expect(table).toHaveTextContent("team:platform");

    await user.click(screen.getByRole("button", { name: "Edit details of feature/login" }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText("Description"), "Login page");
    await user.type(within(dialog).getByLabelText("Owner"), "alice");
    expect(within(dialog).getByText(/Use user:NAME or team:SLUG/)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Save" })).toBeDisabled();
    await user.clear(within(dialog).getByLabelText("Owner"));
    await user.type(within(dialog).getByLabelText("Owner"), "user:alice");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${B}/feature/login`)).toHaveLength(1));
    expect(fetchMock.find(`PATCH ${B}/feature/login`)[0]?.body).toEqual({ description: "Login page", owner: "user:alice" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("explains config-managed branches", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/branches",
      api: treeRoutes({
        [`GET ${B}`]: branches,
        [`PATCH ${B}/main`]: apiError(409, "config_managed", "branch main is declared in .wt-tree/branches.toml"),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Edit details of main" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByLabelText("Description")).toHaveValue("Production web app");
    await user.type(within(dialog).getByLabelText("Description"), " (edited)");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${B}/main`)).toHaveLength(1));
    expect(fetchMock.find(`PATCH ${B}/main`)[0]?.body).toEqual({ description: "Production web app (edited)" });
    expect(await within(dialog).findByText(/change the file and push it instead/)).toBeInTheDocument();
  });
});
