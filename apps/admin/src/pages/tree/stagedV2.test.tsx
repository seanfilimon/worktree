import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { StagedSnapshotV06 } from "@/api/v06";
import * as fx from "@/test/fixtures";
import { apiError, json, type MockRoute } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";
import type { StagedHistoryEntry } from "./v08/stagedApi";

const T = "/api/v1/trees/acme/web";
const recent = (minutesAgo: number) => new Date(Date.now() - minutesAgo * 60_000).toISOString();

function entry(over: Partial<StagedSnapshotV06>): StagedSnapshotV06 {
  return {
    snapshot_id: "staged-x",
    tree_id: "tree-web",
    tenant_id: fx.tenant.id,
    account_id: "acc-2",
    author: "bob",
    branch_name: "feature/login",
    files_changed: ["src/login.tsx"],
    files_added: 1,
    files_modified: 0,
    files_deleted: 0,
    message: "WIP",
    staged_at: recent(5),
    ...over,
  };
}

const live: StagedSnapshotV06[] = [
  entry({ snapshot_id: "staged-bob-spike", label: "spike", message: "Try OAuth", staged_at: recent(1), manifest_hash: "ab".repeat(32), files_changed: ["src/oauth.ts"] }),
  entry({ snapshot_id: "staged-bob", message: "WIP login form", staged_at: recent(3) }),
  entry({ snapshot_id: "staged-carol", account_id: "acc-3", author: "carol", branch_name: "main", message: "Docs", staged_at: recent(10), files_changed: ["docs/intro.md"] }),
  entry({ snapshot_id: "staged-alice", account_id: "acc-1", author: "alice", branch_name: "main", label: "", message: "Mine", staged_at: recent(20), files_changed: ["README.md"] }),
];

function hist(over: Partial<StagedHistoryEntry>): StagedHistoryEntry {
  return { ...entry({}), id: "h-1", status: "pushed", ended_at: recent(30), ended_by: "bob", ...over };
}

const history: StagedHistoryEntry[] = [
  hist({ id: "h-1", status: "pushed", pushed_tip: "snap-9", files_changed: ["a.ts", "b.ts"] }),
  hist({ id: "h-2", status: "cleared", author: "carol", account_id: "acc-3", branch_name: "main", label: "exp", ended_by: "alice", files_changed: ["c.ts"] }),
  hist({ id: "h-3", status: "expired", ended_by: "system", files_changed: [] }),
];

function routes(extra: Record<string, MockRoute> = {}) {
  return treeRoutes({
    [`GET ${T}/staged`]: live,
    [`GET ${T}/staged/summary`]: [],
    [`GET ${T}/staged/history`]: (req) => {
      const status = req.url.searchParams.get("status");
      return json({ history: status ? history.filter((h) => h.status === status) : history });
    },
    [`DELETE ${T}/staged`]: (req) => json({ removed: live.filter((s) => s.branch_name === req.url.searchParams.get("branch")).slice(0, 1) }),
    ...extra,
  });
}

describe("staged tab (staged-v2)", () => {
  it("groups live entries by user, one row per branch and label", async () => {
    renderApp({ route: "/trees/acme/web/staged", api: routes() });
    const list = await screen.findByRole("list", { name: "Staged snapshots" });
    const groups = within(list)
      .getAllByRole("listitem")
      .filter((li) => li.getAttribute("aria-label")?.startsWith("Staged by "));
    expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual(["Staged by bob", "Staged by carol", "Staged by alice"]);

    const bob = within(list).getByRole("list", { name: "Snapshots staged by bob" });
    const rows = within(bob)
      .getAllByRole("listitem")
      .filter((li) => li.parentElement === bob);
    expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual(["feature/login · default", "feature/login · spike"]);
    expect(within(rows[0]!).getByText("default")).toBeInTheDocument();
    expect(within(rows[0]!).getByText("WIP login form")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("spike")).toBeInTheDocument();
    expect(within(rows[1]!).getByRole("list", { name: "Files staged by bob on feature/login (spike)" })).toHaveTextContent("src/oauth.ts");
    expect(within(groups[0]!).getByText(/2 snapshots · 1 branch/)).toBeInTheDocument();

    // The signed-in user's own group is marked.
    expect(within(groups[2]!).getByText("you")).toBeInTheDocument();
    expect(within(groups[0]!).queryByText("you")).not.toBeInTheDocument();
  });

  it("renders the history table and filters it by status", async () => {
    const { user, fetchMock } = renderApp({ route: "/trees/acme/web/staged", api: routes() });
    const table = await screen.findByRole("table", { name: "Staged history" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(3);
    expect(within(rows[0]!).getByText("pushed")).toBeInTheDocument();
    expect(within(rows[0]!).getByRole("link")).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-9");
    expect(within(rows[0]!).getByText("2")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("cleared")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("exp")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("alice")).toBeInTheDocument();
    expect(within(rows[2]!).getByText("expired")).toBeInTheDocument();
    expect(within(rows[2]!).getByText("system")).toBeInTheDocument();
    expect(fetchMock.find(`GET ${T}/staged/history`)[0]?.url.searchParams.get("status")).toBeNull();

    expect(screen.getByRole("radio", { name: "All" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: "Cleared" }));
    await waitFor(() => expect(within(screen.getByRole("table", { name: "Staged history" })).getAllByRole("row")).toHaveLength(2));
    expect(within(screen.getByRole("table", { name: "Staged history" })).getByText("cleared")).toBeInTheDocument();
    expect(fetchMock.find(`GET ${T}/staged/history`).at(-1)?.url.searchParams.get("status")).toBe("cleared");

    fetchMock.on(`GET ${T}/staged/history`, { history: null });
    await user.click(screen.getByRole("radio", { name: "Expired" }));
    expect(await screen.findByText("No expired staged snapshots")).toBeInTheDocument();
  });

  it("shows empty states and an unavailable notice on older servers", async () => {
    renderApp({
      route: "/trees/acme/web/staged",
      api: routes({
        [`GET ${T}/staged`]: [],
        [`GET ${T}/staged/history`]: () => apiError(501, "not_implemented", "staged history is not enabled on this server"),
      }),
    });
    expect(await screen.findByText("Nobody has staged work")).toBeInTheDocument();
    expect(await screen.findByText("Staged history is not available on this server")).toBeInTheDocument();
  });

  it("unstages one labeled slot of another user as tree admin", async () => {
    const { user, fetchMock } = renderApp({ route: "/trees/acme/web/staged", api: routes() });
    await screen.findByRole("list", { name: "Staged snapshots" });
    // alice is a tenant owner: every entry can be unstaged.
    expect(screen.getAllByRole("button", { name: /^Unstage / })).toHaveLength(4);

    await user.click(screen.getByRole("button", { name: "Unstage feature/login (spike) by bob" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Unstage feature/login (spike)?" });
    await user.click(within(dialog).getByRole("button", { name: "Unstage" }));

    await waitFor(() => expect(fetchMock.find(`DELETE ${T}/staged`)).toHaveLength(1));
    const url = fetchMock.find(`DELETE ${T}/staged`)[0]!.url;
    expect(url.searchParams.get("branch")).toBe("feature/login");
    expect(url.searchParams.get("label")).toBe("spike");
    expect(url.searchParams.get("account")).toBe("acc-2");
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("lets a member unstage only their own default slot (label= sent empty)", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/staged",
      api: { "GET /api/v1/auth/me": fx.memberMe, ...routes() },
    });
    await screen.findByRole("list", { name: "Staged snapshots" });
    const buttons = await screen.findAllByRole("button", { name: /^Unstage / });
    expect(buttons.map((b) => b.getAttribute("aria-label"))).toEqual(["Unstage main by alice"]);

    await user.click(buttons[0]!);
    const dialog = await screen.findByRole("alertdialog", { name: "Unstage main?" });
    await user.click(within(dialog).getByRole("button", { name: "Unstage" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${T}/staged`)).toHaveLength(1));
    const url = fetchMock.find(`DELETE ${T}/staged`)[0]!.url;
    expect(url.search).toContain("label=");
    expect(url.searchParams.get("label")).toBe("");
    expect(url.searchParams.get("branch")).toBe("main");
    expect(url.searchParams.has("account")).toBe(false);
  });

  it("opens the staged file browser for a labeled snapshot", async () => {
    const S = `${T}/staged/staged-bob-spike/files`;
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/staged",
      api: routes({
        [`GET ${S}`]: {
          staged: live[0],
          files: [{ path: "src/oauth.ts", hash: "11".repeat(32), size: 12, executable: false }],
          restricted_count: 0,
        },
        [`GET ${S}/src/oauth.ts`]: () => new Response("export {};\n"),
      }),
    });
    const browse = await screen.findAllByRole("button", { name: /^Browse files staged by/ });
    expect(browse.map((b) => b.getAttribute("aria-label"))).toEqual(["Browse files staged by bob on feature/login (spike)"]);
    await user.click(browse[0]!);
    const sheet = await screen.findByRole("dialog", { name: "Staged files" });
    expect(sheet).toHaveTextContent("label spike");
    expect(await within(sheet).findByRole("region", { name: "Staged contents of src/oauth.ts" })).toHaveTextContent("export {};");
    expect(fetchMock.find(`GET ${S}`)).toHaveLength(1);
  });
});
