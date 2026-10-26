import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";
import { mirrorFormErrors, normMirror, syncSummary, type Mirror } from "./mirrorsApi";

const M = "/api/v1/trees/acme/web/mirrors";

const mirror: Mirror = {
  id: "m-1",
  tenant_id: "tenant-acme",
  tree_id: "tree-web",
  url: "https://github.com/acme/web.git",
  has_credential: true,
  branches: ["main", "release/*"],
  tags: true,
  active: true,
  created_by: "alice",
  created_at: "2026-10-01T10:00:00Z",
  last_sync_at: "2026-10-02T10:00:00Z",
  last_status: "ok",
};

const failing: Mirror = {
  ...mirror,
  id: "m-2",
  url: "git@gitlab.example.com:acme/web.git",
  has_credential: false,
  branches: [],
  tags: false,
  last_status: "error",
  last_error: "git push failed: ! [rejected] main (stale info)",
};

const list = (mirrors: Mirror[], git = { available: true, version: "2.45.1" }) => ({ mirrors, git });

describe("mirror helpers", () => {
  it("validates the form", () => {
    expect(mirrorFormErrors("", [], "")).toEqual({ url: "Required.", branches: "", credential: "" });
    expect(mirrorFormErrors("https://github.com/acme/web.git", ["main", "release/*"], "tok")).toEqual({ url: "", branches: "", credential: "" });
    expect(mirrorFormErrors("git@github.com:acme/web.git", [], "").url).toBe("");
    expect(mirrorFormErrors("git@github.com:acme/web.git", [], "tok").credential).toMatch(/http\(s\)/);
    expect(mirrorFormErrors("ftp://x/y", [], "").url).toMatch(/Git remote/);
    expect(mirrorFormErrors("https://u:p@github.com/x.git", [], "").url).toMatch(/password/);
    expect(mirrorFormErrors("not a url", [], "").url).toMatch(/Enter a Git remote/);
    expect(mirrorFormErrors("https://x/y.git", ["a b", "ok"], "").branches).toMatch(/a b/);
    expect(mirrorFormErrors("https://x/y.git", ["x//y"], "").branches).toMatch(/x\/\/y/);
  });

  it("normalises null lists and summarises syncs", () => {
    expect(normMirror({ ...mirror, branches: null as unknown as string[] }).branches).toEqual([]);
    expect(syncSummary(undefined)).toBe("Sync started.");
    expect(syncSummary({ commits: 1, refs: [{ ref: "refs/heads/main", status: "updated" }, { ref: "refs/tags/v1", status: "up_to_date" }], pushed: true, excluded: ["secret/k"], skipped: [], duration_ms: 3 })).toBe(
      "1 new commit, 1 ref updated, 1 path not redistributable",
    );
    expect(syncSummary({ commits: 0, refs: [], pushed: true, excluded: [], skipped: [], duration_ms: 1 })).toBe("0 new commits, everything up to date");
    expect(syncSummary({ commits: 0, refs: [], pushed: false, excluded: [], skipped: [], error: "boom", duration_ms: 1 })).toBe("boom");
  });
});

describe("mirrors tab", () => {
  it("lists mirrors with status and last error, and syncs one now", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/mirrors",
      api: treeRoutes({
        [`GET ${M}`]: list([mirror, failing]),
        [`POST ${M}/m-2/sync`]: json({
          mirror: { ...failing, last_status: "ok", last_error: "" },
          result: { commits: 2, refs: [{ ref: "refs/heads/main", new: "abc", status: "forced" }], pushed: true, excluded: null, skipped: null, duration_ms: 40 },
        }),
      }),
    });
    const table = await screen.findByRole("table", { name: "Mirrors" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("https://github.com/acme/web.git");
    expect(rows[0]).toHaveTextContent("credential");
    expect(rows[0]).toHaveTextContent("release/*");
    expect(rows[0]).toHaveTextContent("Synced");
    expect(rows[1]).toHaveTextContent("all branches");
    expect(rows[1]).toHaveTextContent("Failing");
    expect(within(rows[1]!).getByLabelText(/Last error: git push failed/)).toBeInTheDocument();
    expect(screen.getByText(/git 2\.45\.1/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Sync git@gitlab.example.com:acme/web.git now" }));
    await waitFor(() => expect(fetchMock.find(`POST ${M}/m-2/sync`)).toHaveLength(1));
    expect(await screen.findByText("1 ref updated", { exact: false })).toBeInTheDocument();
  });

  it("adds a mirror with branch globs and a credential", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/mirrors",
      api: treeRoutes({
        [`GET ${M}`]: list([]),
        [`POST ${M}`]: (req) => json({ ...mirror, ...(req.body as object), has_credential: true }, 201),
      }),
    });
    expect(await screen.findByText("No mirrors")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add mirror" }));
    const dialog = await screen.findByRole("dialog", { name: "Add mirror" });
    await user.click(within(dialog).getByRole("button", { name: "Add mirror" }));
    expect(within(dialog).getByText("Required.")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Remote URL"), "https://github.com/acme/web.git");
    await user.type(within(dialog).getByLabelText("Branches"), "main{Enter}release/*{Enter}");
    await user.type(within(dialog).getByLabelText("Credential"), "ghp_secret");
    await user.click(within(dialog).getByLabelText("Mirror tags"));
    await user.click(within(dialog).getByRole("button", { name: "Add mirror" }));
    await waitFor(() => expect(fetchMock.find(`POST ${M}`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${M}`)[0]?.body).toEqual({
      url: "https://github.com/acme/web.git",
      branches: ["main", "release/*"],
      tags: false,
      active: true,
      credential: "ghp_secret",
    });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Add mirror" })).not.toBeInTheDocument());
  });

  it("edits, pauses and deletes a mirror", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/mirrors",
      api: treeRoutes({
        [`GET ${M}`]: list([mirror]),
        [`PATCH ${M}/m-1`]: (req) => json({ ...mirror, ...(req.body as object) }),
        [`DELETE ${M}/m-1`]: noContent(),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Edit https://github.com/acme/web.git" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit mirror" });
    expect(within(dialog).getByText("Leave empty to keep the current credential.")).toBeInTheDocument();
    await user.click(within(dialog).getByLabelText("Remove the stored credential"));
    await user.click(within(dialog).getByRole("button", { name: "Save mirror" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${M}/m-1`)).toHaveLength(1));
    expect(fetchMock.find(`PATCH ${M}/m-1`)[0]?.body).toEqual({ branches: ["main", "release/*"], tags: true, active: true, credential: "" });

    await user.click(screen.getByRole("switch", { name: "Active: https://github.com/acme/web.git" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${M}/m-1`)).toHaveLength(2));
    expect(fetchMock.find(`PATCH ${M}/m-1`)[1]?.body).toEqual({ active: false });

    await user.click(screen.getByRole("button", { name: "Delete https://github.com/acme/web.git" }));
    const confirm = await screen.findByRole("alertdialog");
    await user.click(within(confirm).getByRole("button", { name: "Delete mirror" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${M}/m-1`)).toHaveLength(1));
  });

  it("explains when git is missing on the server", async () => {
    renderApp({
      route: "/trees/acme/web/mirrors",
      api: treeRoutes({ [`GET ${M}`]: list([{ ...mirror, last_status: "disabled" }], { available: false, error: 'git binary "git" not found' } as never) }),
    });
    expect(await screen.findByText("Mirrors are disabled on this server")).toBeInTheDocument();
    expect(screen.getByText('git binary "git" not found')).toBeInTheDocument();
    expect(screen.getByText("Disabled")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sync https://github.com/acme/web.git now" })).toBeDisabled();
  });

  it("shows a notice when the server has no mirror support", async () => {
    renderApp({
      route: "/trees/acme/web/mirrors",
      api: treeRoutes({ [`GET ${M}`]: () => apiError(501, "not_implemented", "Git mirrors is not available on this server") }),
    });
    expect(await screen.findByText("Git mirrors is not available on this server")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add mirror" })).not.toBeInTheDocument();
  });
});
