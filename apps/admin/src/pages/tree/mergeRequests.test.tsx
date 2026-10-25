import { screen, waitFor, within } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";
import type { CompareResult, MergeRequest } from "@/api/types";
import * as fx from "@/test/fixtures";
import * as mr from "@/test/fixtures.mr";
import { apiError, json, type MockRoute } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";
const MR = `${T}/merge-requests/7`;
const LIST = "/trees/acme/web/merge-requests";
const DETAIL = `${LIST}/7`;

beforeAll(() => {
  // Radix Select / Popper rely on layout APIs jsdom does not implement.
  const stubs = { hasPointerCapture: () => false, releasePointerCapture: () => undefined, scrollIntoView: () => undefined };
  for (const [name, value] of Object.entries(stubs)) {
    if (!(name in Element.prototype)) Object.defineProperty(Element.prototype, name, { value, configurable: true });
  }
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

/** Detail-page mocks: the MR (GET reflects the last action) and a comparison. */
function detailRoutes(
  initial: MergeRequest,
  compare: CompareResult = mr.compareReady,
  actions: Partial<Record<"approve" | "merge" | "close", MergeRequest | Response>> = {},
): Record<string, MockRoute> {
  let current = initial;
  const routes: Record<string, MockRoute> = {
    [`GET ${MR}`]: () => json(current),
    [`GET ${T}/compare`]: compare,
  };
  for (const [action, result] of Object.entries(actions)) {
    routes[`POST ${MR}/${action}`] = () => {
      if (result instanceof Response) return result;
      current = result;
      return json(result);
    };
  }
  return treeRoutes(routes);
}

const readiness = () => screen.findByRole("list", { name: "Merge readiness" });
const check = (list: HTMLElement, id: string) => list.querySelector(`[data-check="${id}"]`)!;

describe("merge request list", () => {
  const listApi = () => treeRoutes({ [`GET ${T}/merge-requests`]: mr.mergeRequestList });

  it("shows open merge requests with counts, approvals and state", async () => {
    const { fetchMock } = renderApp({ route: LIST, api: listApi() });
    const table = await screen.findByRole("table", { name: "Merge requests" });
    // One request for every state; filtering happens client side.
    expect(fetchMock.find(`GET ${T}/merge-requests`)[0]?.url.searchParams.has("state")).toBe(false);

    expect(screen.getByRole("radio", { name: "Open (2)" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "Merged (1)" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Closed (1)" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "All (4)" })).toBeInTheDocument();

    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    // Most recently updated first.
    expect(rows[0]).toHaveTextContent("Update README");
    const login = within(table).getByRole("link", { name: "Add login page" });
    expect(login).toHaveAttribute("href", "/trees/acme/web/merge-requests/7");
    const row = login.closest("tr")!;
    expect(within(row).getByText("!7")).toBeInTheDocument();
    // main requires 2 approvals (protection "main").
    expect(within(row).getByLabelText("1 of 2 required approvals")).toHaveTextContent("1/2");
    expect(within(row).getByText("Open").closest("[data-state]")).toHaveAttribute("data-state", "open");
  });

  it("filters by state", async () => {
    const { user, router } = renderApp({ route: LIST, api: listApi() });
    await screen.findByRole("table", { name: "Merge requests" });

    await user.click(screen.getByRole("radio", { name: "Merged (1)" }));
    const table = screen.getByRole("table", { name: "Merge requests" });
    expect(within(table).getAllByRole("row")).toHaveLength(2);
    expect(within(table).getByRole("link", { name: "Fix typo in footer" })).toBeInTheDocument();
    expect(router.state.location.search).toBe("?state=merged");

    await user.click(screen.getByRole("radio", { name: "Closed (1)" }));
    const closed = within(screen.getByRole("table", { name: "Merge requests" })).getByText("Closed");
    expect(closed.closest("[data-state]")).toHaveClass("line-through");
    // Release branches are unprotected: no required count.
    expect(screen.getByLabelText("0 approvals")).toHaveTextContent("0");

    await user.click(screen.getByRole("radio", { name: "All (4)" }));
    expect(within(screen.getByRole("table", { name: "Merge requests" })).getAllByRole("row")).toHaveLength(5);
  });

  it("searches by title, author and branch", async () => {
    const { user } = renderApp({ route: `${LIST}?state=all`, api: listApi() });
    const search = await screen.findByRole("searchbox", { name: "Search merge requests" });
    const rows = () => within(screen.getByRole("table", { name: "Merge requests" })).getAllByRole("row").slice(1);

    await user.type(search, "carol");
    expect(rows()).toHaveLength(1);
    expect(rows()[0]).toHaveTextContent("Fix typo in footer");

    await user.clear(search);
    await user.type(search, "release/");
    expect(rows()).toHaveLength(1);
    expect(rows()[0]).toHaveTextContent("Spike: new renderer");

    await user.clear(search);
    await user.type(search, "readme");
    expect(rows()[0]).toHaveTextContent("Update README");

    await user.clear(search);
    await user.type(search, "nothing-like-this");
    expect(screen.getByText("No merge requests match “nothing-like-this”")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(search).toHaveValue("");
    expect(rows()).toHaveLength(4);
  });

  it("shows empty states", async () => {
    const { user } = renderApp({ route: LIST, api: treeRoutes({ [`GET ${T}/merge-requests`]: null }) });
    expect(await screen.findByText("No merge requests yet")).toBeInTheDocument();
    // The empty state offers its own call to action next to the toolbar's.
    const buttons = screen.getAllByRole("button", { name: "New merge request" });
    expect(buttons).toHaveLength(2);
    await user.click(buttons[1]!);
    expect(await screen.findByRole("dialog", { name: "New merge request" })).toBeInTheDocument();
  });
});

describe("new merge request dialog", () => {
  it("previews the change summary and submits the payload", async () => {
    const { user, router, fetchMock } = renderApp({
      route: LIST,
      api: treeRoutes({
        [`GET ${T}/merge-requests`]: [],
        [`GET ${T}/compare`]: mr.compareReady,
        [`POST ${T}/merge-requests`]: (req) => json({ ...fx.mergeRequest, ...(req.body as object) }, 201),
        [`GET ${MR}`]: fx.mergeRequest,
      }),
    });
    await screen.findByText("No merge requests yet");
    await user.click(screen.getAllByRole("button", { name: "New merge request" })[0]!);
    const dialog = await screen.findByRole("dialog", { name: "New merge request" });
    const form = within(dialog).getByRole("form", { name: "Create merge request" });

    expect(within(form).getByRole("combobox", { name: "Source branch" })).toHaveTextContent("feature/login");
    expect(within(form).getByRole("combobox", { name: "Target branch" })).toHaveTextContent("main");
    // Title suggested from the source branch.
    expect(within(form).getByLabelText("Title")).toHaveValue("Login");

    const summary = within(form).getByRole("region", { name: "Change summary" });
    await within(summary).findByText("2 files changed");
    expect(summary).toHaveTextContent("+12");
    expect(summary).toHaveTextContent("−1");
    expect(summary).toHaveTextContent("1 snapshot");
    const cmp = fetchMock.find(`GET ${T}/compare`)[0]!;
    expect(cmp.url.searchParams.get("from")).toBe("snap-3");
    expect(cmp.url.searchParams.get("to")).toBe("snap-4");

    await user.clear(within(form).getByLabelText("Title"));
    await user.type(within(form).getByLabelText("Title"), "Add login page");
    await user.type(within(form).getByLabelText("Description"), "Line one\nLine two");
    await user.click(within(form).getByRole("button", { name: "Open merge request" }));

    expect(await screen.findByRole("heading", { name: /Add login page/, level: 2 })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe(DETAIL);
    expect(fetchMock.find(`POST ${T}/merge-requests`)[0]?.body).toEqual({
      title: "Add login page",
      description: "Line one\nLine two",
      source_branch: "feature/login",
      target_branch: "main",
    });
  });

  it("warns about diverged and already merged branches", async () => {
    let result: CompareResult = mr.compareDiverged;
    const { user } = renderApp({
      route: LIST,
      api: treeRoutes({ [`GET ${T}/merge-requests`]: [], [`GET ${T}/compare`]: () => json(result) }),
    });
    await screen.findByText("No merge requests yet");
    await user.click(screen.getAllByRole("button", { name: "New merge request" })[0]!);
    const dialog = await screen.findByRole("dialog", { name: "New merge request" });
    expect(await within(dialog).findByText("Branches have diverged")).toBeInTheDocument();
    expect(within(dialog).getByText(/merge base snap2/)).toBeInTheDocument();

    // Same branch on both sides: no comparison.
    result = mr.compareUpToDate;
    await user.click(within(dialog).getByRole("combobox", { name: "Source branch" }));
    await user.click(await screen.findByRole("option", { name: "main" }));
    expect(within(dialog).getByLabelText("Title")).toHaveValue("Main");
    expect(within(dialog).getByText("Choose two different branches to preview the changes.")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Open merge request" }));
    expect(within(dialog).getByText("Source and target must differ.")).toBeInTheDocument();
  });

  it("validates the title and maps server errors", async () => {
    const { user } = renderApp({
      route: LIST,
      api: treeRoutes({
        [`GET ${T}/merge-requests`]: [],
        [`GET ${T}/compare`]: mr.compareUpToDate,
        [`POST ${T}/merge-requests`]: apiError(
          409,
          "conflict",
          "merge request !7 from 'feature/login' into 'main' is already open: conflict",
        ),
      }),
    });
    await screen.findByText("No merge requests yet");
    await user.click(screen.getAllByRole("button", { name: "New merge request" })[0]!);
    const dialog = await screen.findByRole("dialog", { name: "New merge request" });
    expect(await within(dialog).findByText("Already merged")).toBeInTheDocument();

    await user.clear(within(dialog).getByLabelText("Title"));
    await user.click(within(dialog).getByRole("button", { name: "Open merge request" }));
    expect(within(dialog).getByText("A title is required.")).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText("Title"), "Again");
    await user.click(within(dialog).getByRole("button", { name: "Open merge request" }));
    const alert = await within(dialog).findByText("A merge request is already open");
    expect(alert.closest("[data-kind]")).toHaveAttribute("data-kind", "already_open");
  });
});

describe("merge request detail", () => {
  it("renders the header, overview and a ready checklist", async () => {
    const { user, fetchMock } = renderApp({ route: DETAIL, api: detailRoutes({ ...mr.approvedMr, description: "First line\nSecond line" }) });
    expect(await screen.findByRole("heading", { name: /Add login page\s*!7/, level: 2 })).toBeInTheDocument();
    expect(screen.getByText("First line\nSecond line", { collapseWhitespace: false, trim: false })).toHaveClass("whitespace-pre-wrap");

    const timeline = screen.getByRole("list", { name: "Timeline" });
    expect(within(timeline).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      expect.stringContaining("bob opened this merge request"),
      expect.stringContaining("carol approved"),
      expect.stringContaining("dave approved"),
    ]);

    const list = await readiness();
    await waitFor(() => expect(check(list, "fast_forward")).toHaveAttribute("data-status", "pass"));
    expect(check(list, "open")).toHaveAttribute("data-status", "pass");
    expect(check(list, "approvals")).toHaveTextContent("2 of 2 required approvals");
    expect(screen.getByRole("button", { name: "Merge" })).toBeEnabled();
    expect(screen.getByRole("progressbar", { name: "Required approvals" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Protection rules" })).toHaveTextContent("main · 2 approvals · no direct push · no delete");

    const cmp = fetchMock.find(`GET ${T}/compare`)[0]!;
    expect(cmp.url.searchParams.get("from")).toBe("snap-3");
    expect(cmp.url.searchParams.get("to")).toBe("snap-4");

    await user.click(screen.getByRole("tab", { name: /Changes/ }));
    const changes = screen.getByRole("tabpanel", { name: /Changes/ });
    expect(changes).toHaveTextContent("2 files changed");
    expect(changes).toHaveTextContent("src/login.tsx");

    await user.click(screen.getByRole("tab", { name: /Snapshots/ }));
    const snaps = screen.getByRole("list", { name: "Snapshots" });
    expect(within(snaps).getByRole("link", { name: "Add login form" })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-4");
  });

  it("blocks merging until enough approvals", async () => {
    renderApp({ route: DETAIL, api: detailRoutes(fx.mergeRequest) });
    const list = await readiness();
    await waitFor(() => expect(check(list, "approvals")).toHaveAttribute("data-status", "fail"));
    expect(check(list, "approvals")).toHaveTextContent("0 of 2 required approvals");
    const merge = screen.getByRole("button", { name: "Merge" });
    expect(merge).toBeDisabled();
    expect(merge).toHaveAccessibleDescription("2 more approvals needed.");
  });

  it("explains diverged branches", async () => {
    renderApp({ route: DETAIL, api: detailRoutes(mr.approvedMr, mr.compareDiverged) });
    const list = await readiness();
    await waitFor(() => expect(check(list, "fast_forward")).toHaveAttribute("data-status", "fail"));
    expect(check(list, "fast_forward")).toHaveTextContent("Branches have diverged");
    expect(screen.getByRole("button", { name: "Merge" })).toHaveAccessibleDescription(
      "Diverged: merge target into source locally, push, then merge.",
    );
  });

  it("shows a merged merge request against its merged snapshot", async () => {
    const { user, fetchMock } = renderApp({ route: DETAIL, api: detailRoutes(mr.mergedMr, mr.compareMerged) });
    await screen.findByRole("heading", { name: /Add login page/, level: 2 });
    expect(screen.queryByRole("list", { name: "Merge readiness" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Merge" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    const outcome = screen.getByRole("complementary", { name: "Review" });
    expect(within(outcome).getByRole("link", { name: "snap4" })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-4");
    expect(within(screen.getByRole("list", { name: "Timeline" })).getByText(/Merged into/)).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: /Changes/ }));
    expect(await screen.findByText(/in the merged snapshot/)).toBeInTheDocument();
    const cmp = fetchMock.find(`GET ${T}/compare`)[0]!;
    expect(cmp.url.searchParams.get("to")).toBe("snap-4");
    expect(cmp.url.searchParams.has("from")).toBe(false);
  });

  it("shows a closed merge request without actions", async () => {
    renderApp({ route: DETAIL, api: detailRoutes(mr.closedMr) });
    await screen.findByRole("heading", { name: /Add login page/, level: 2 });
    expect(screen.queryByRole("button", { name: "Close" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Merge" })).not.toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Timeline" })).getByText(/Closed without merging/)).toBeInTheDocument();
    expect(screen.getAllByText("Closed")[0]!.closest("[data-state]")).toHaveAttribute("data-state", "closed");
  });

  it("approves with the button and the keyboard", async () => {
    const approved = { ...fx.mergeRequest, approvals: [mr.approval("alice", "acc-1", 0)] };
    const { user, fetchMock } = renderApp({ route: DETAIL, api: detailRoutes(fx.mergeRequest, mr.compareReady, { approve: approved }) });
    await readiness();
    await user.keyboard("a");
    const approvals = await screen.findByRole("list", { name: "Approvals" });
    expect(within(approvals).getByText("alice")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "You approved" })).toBeDisabled();
    expect(fetchMock.find(`POST ${MR}/approve`)).toHaveLength(1);

    // Already approved: the shortcut is inactive.
    await user.keyboard("a");
    expect(fetchMock.find(`POST ${MR}/approve`)).toHaveLength(1);
  });

  it("hides Approve from the author", async () => {
    const { user, fetchMock } = renderApp({ route: DETAIL, api: detailRoutes(mr.ownMr, mr.compareReady, { approve: mr.ownMr }) });
    await readiness();
    expect(screen.queryByRole("button", { name: /Approve/ })).not.toBeInTheDocument();
    await user.keyboard("a");
    expect(fetchMock.find(`POST ${MR}/approve`)).toHaveLength(0);
  });

  it("merges after confirming with the m shortcut", async () => {
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: detailRoutes(mr.approvedMr, mr.compareReady, { merge: mr.mergedMr }),
    });
    const list = await readiness();
    await waitFor(() => expect(check(list, "fast_forward")).toHaveAttribute("data-status", "pass"));

    await user.keyboard("m");
    const confirm = await screen.findByRole("alertdialog", { name: "Merge !7 into main?" });
    expect(confirm).toHaveTextContent("main will be fast-forwarded to the tip of feature/login.");
    // Shortcuts are ignored while the dialog is open.
    await user.keyboard("m");
    expect(fetchMock.find(`POST ${MR}/merge`)).toHaveLength(0);
    await user.click(within(confirm).getByRole("button", { name: "Merge" }));

    const review = screen.getByRole("complementary", { name: "Review" });
    expect(await within(review).findByRole("link", { name: "snap4" })).toBeInTheDocument();
    expect(fetchMock.find(`POST ${MR}/merge`)).toHaveLength(1);
    expect(screen.queryByRole("button", { name: "Merge" })).not.toBeInTheDocument();
  });

  it.each([
    {
      name: "review_required",
      response: () => apiError(403, "review_required", "1 of 2 approvals"),
      title: "Review required",
      body: ["does not have enough approvals", "2 of 2 required approvals.", "Server: 1 of 2 approvals"],
    },
    {
      name: "diverged",
      response: () => apiError(409, "conflict", "merge request !7: target has diverged; merge target into source locally and push: conflict"),
      title: "Branches have diverged",
      body: ["wt merge"],
    },
    {
      name: "target moved",
      response: () => apiError(409, "conflict", "merge request !7: target branch 'main' moved during the merge; retry: conflict"),
      title: "Target branch moved",
      body: ["retry"],
    },
  ])("maps a $name merge failure to an alert", async ({ response, title, body }) => {
    const { user } = renderApp({ route: DETAIL, api: detailRoutes(mr.approvedMr, mr.compareReady, { merge: response() }) });
    const list = await readiness();
    await waitFor(() => expect(check(list, "fast_forward")).toHaveAttribute("data-status", "pass"));
    await user.click(screen.getByRole("button", { name: "Merge" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Merge" }));

    const heading = await screen.findByText(title);
    const alert = heading.closest('[role="alert"]')!;
    for (const text of body) expect(alert).toHaveTextContent(text);
  });

  it("maps a self-approval rejection", async () => {
    const { user } = renderApp({
      route: DETAIL,
      api: detailRoutes(fx.mergeRequest, mr.compareReady, {
        approve: apiError(403, "forbidden", "merge request !7: authors cannot approve their own merge request: forbidden"),
      }),
    });
    await readiness();
    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(await screen.findByText("You cannot approve your own merge request")).toBeInTheDocument();
  });

  it("closes after confirmation", async () => {
    const { user, fetchMock } = renderApp({ route: DETAIL, api: detailRoutes(fx.mergeRequest, mr.compareReady, { close: mr.closedMr }) });
    await readiness();
    await user.click(screen.getByRole("button", { name: "Close" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Close !7?" });
    expect(fetchMock.find(`POST ${MR}/close`)).toHaveLength(0);
    await user.click(within(confirm).getByRole("button", { name: "Close merge request" }));

    const review = screen.getByRole("complementary", { name: "Review" });
    expect(await within(review).findByText(/Closed without merging/)).toBeInTheDocument();
    expect(fetchMock.find(`POST ${MR}/close`)).toHaveLength(1);
    expect(screen.queryByRole("list", { name: "Merge readiness" })).not.toBeInTheDocument();
  });
});
