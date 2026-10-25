import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ApiError } from "@/api/client";
import { defaultMergeMessage, isUnavailable, mergeConflictsOf } from "@/api/v06";
import { blameRunStarts } from "@/components/files/blame";
import { lineDiff } from "@/lib/lineDiff";
import { codeHitHref, groupHits, highlightParts } from "@/pages/search/codeSearch";
import { applyMergeability, defaultMethod } from "@/pages/tree/v06/mergeUtil";
import { webhookFormErrors } from "@/pages/webhooks/webhookForm";
import * as fx from "@/test/fixtures";
import * as ffx from "@/test/fixtures.files";
import * as ix from "@/test/fixtures.iam";
import * as mr from "@/test/fixtures.mr";
import * as v6 from "@/test/fixtures.v06";
import { apiError, json, noContent, type MockRoute } from "@/test/mockFetch";
import type { ReadinessCheck } from "@/components/mr/model";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";
const TEN = "/api/v1/tenants/acme";
const noEndpoint = (method: string, path: string) => () => apiError(404, "not_found", `no such endpoint: ${method} ${path}`);

describe("v0.6 helpers", () => {
  it("detects unavailable backends", () => {
    expect(isUnavailable(new ApiError(501, "not_implemented", "x"))).toBe(true);
    expect(isUnavailable(new ApiError(404, "not_found", "no such endpoint: GET /api/v1/search/code"))).toBe(true);
    expect(isUnavailable(new ApiError(405, "method_not_allowed", "x"))).toBe(true);
    expect(isUnavailable(new ApiError(404, "not_found", "webhook not found"))).toBe(false);
    expect(isUnavailable(new Error("x"))).toBe(false);
  });

  it("reads merge conflicts from a 409", () => {
    const err = new ApiError(409, "merge_conflict", "conflicts", { conflicts: [{ path: "a.txt", kind: "both_modified" }] });
    expect(mergeConflictsOf(err)).toEqual([{ path: "a.txt", kind: "both_modified" }]);
    expect(mergeConflictsOf(new ApiError(409, "conflict", "x"))).toBeNull();
  });

  it("builds default merge messages", () => {
    const m = { source_branch: "feature/login", target_branch: "main", number: 7, title: "Add login" };
    expect(defaultMergeMessage("merge", m)).toBe("Merge branch 'feature/login' into main (!7)");
    expect(defaultMergeMessage("squash", m)).toBe("Add login");
  });

  it("picks a default method and applies mergeability to readiness", () => {
    expect(defaultMethod(undefined)).toBeUndefined();
    expect(defaultMethod({ fast_forward: true, methods: ["merge", "fast-forward"], conflicts: [] })).toBe("fast-forward");
    expect(defaultMethod({ fast_forward: false, methods: ["squash", "merge"], conflicts: [] })).toBe("squash");

    const diverged: ReadinessCheck[] = [{ id: "fast_forward", label: "Branches have diverged", status: "fail", detail: "Diverged" }];
    expect(applyMergeability(diverged, undefined, "main")).toBe(diverged);
    expect(applyMergeability(diverged, { fast_forward: false, methods: ["merge"], conflicts: [] }, "main")[0]).toMatchObject({ status: "pass" });
    expect(
      applyMergeability(diverged, { fast_forward: false, methods: ["merge"], conflicts: [{ path: "a", kind: "x" }] }, "main")[0],
    ).toMatchObject({ status: "fail", label: "1 conflicting path" });
    expect(applyMergeability(diverged, { fast_forward: false, methods: [], conflicts: [] }, "main")[0]).toMatchObject({
      status: "fail",
      label: "No merge method available",
    });
  });

  it("highlights literal and regex matches", () => {
    expect(highlightParts("Hello hello", "hello", false, false)).toEqual([
      { text: "Hello", match: true },
      { text: " ", match: false },
      { text: "hello", match: true },
    ]);
    expect(highlightParts("Hello hello", "hello", false, true).filter((p) => p.match)).toHaveLength(1);
    expect(highlightParts("a.b axb", "a.b", false, false).filter((p) => p.match)).toHaveLength(1);
    expect(highlightParts("func New(x)", "New\\(", true, true).find((p) => p.match)?.text).toBe("New(");
    expect(highlightParts("abc", "(", true, false)).toEqual([{ text: "abc", match: false }]);
  });

  it("groups hits by tree and file and links to the file line", () => {
    const groups = groupHits(v6.codeResults.hits);
    expect(groups.map((g) => g.tree)).toEqual(["acme/web", "acme/api"]);
    expect(groups[0]!.files).toHaveLength(1);
    expect(groups[0]!.files[0]!.hits).toHaveLength(2);
    expect(codeHitHref(v6.codeResults.hits[0]!, 2)).toBe("/trees/acme/web/files?snapshot=snap-3&path=src%2Fapp.ts#L2");
  });

  it("diffs lines", () => {
    expect(lineDiff("a\nb\nc", "a\nx\nc")).toEqual([
      { kind: "same", text: "a" },
      { kind: "del", text: "b" },
      { kind: "add", text: "x" },
      { kind: "same", text: "c" },
    ]);
    expect(lineDiff("", "a")).toEqual([{ kind: "add", text: "a" }]);
  });

  it("validates webhook forms", () => {
    expect(webhookFormErrors("", [])).toEqual({ url: "Required.", events: "Add at least one event (or * for every event)." });
    expect(webhookFormErrors("ftp://x", ["*"]).url).toMatch(/http/);
    expect(webhookFormErrors("https://x.example", ["mr.*", "branch.pushed"])).toEqual({ url: "", events: "" });
    expect(webhookFormErrors("https://x.example", ["Bad Event"]).events).toMatch(/not an event pattern/);
  });

  it("finds blame runs", () => {
    const l = (line: number, snapshot_id: string) => ({ line, text: "", snapshot_id, author: "a", message: "", at: "" });
    expect([...blameRunStarts([l(1, "a"), l(2, "a"), l(3, "b"), l(4, "a")])]).toEqual([0, 2, 3]);
  });
});

describe("webhooks", () => {
  const tenantRoute = "/tenants/acme?tab=webhooks";
  const tenantApi = (extra: Record<string, MockRoute> = {}) => ({ ...ix.tenantIamRoutes(), ...extra });

  it("lists tenant webhooks with status and toggles one", async () => {
    const { user, fetchMock } = renderApp({
      route: tenantRoute,
      api: tenantApi({
        [`GET ${TEN}/webhooks`]: [v6.webhook, v6.failingWebhook],
        [`PATCH ${TEN}/webhooks/wh-1`]: (req) => json({ ...v6.webhook, ...(req.body as object) }),
      }),
    });
    const table = await screen.findByRole("table", { name: "Webhooks" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent("https://ci.example.com/hooks/wt");
    expect(rows[0]).toHaveTextContent("signed");
    expect(rows[0]).toHaveTextContent("mr.*");
    expect(rows[1]).toHaveTextContent("20 failing");
    expect(within(rows[1]!).getByText("https://chat.example.com/notify")).toHaveClass("line-through");

    await user.click(within(rows[0]!).getByRole("switch", { name: "Active: https://ci.example.com/hooks/wt" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${TEN}/webhooks/wh-1`)).toHaveLength(1));
    expect(fetchMock.find(`PATCH ${TEN}/webhooks/wh-1`)[0]?.body).toEqual({ active: false });
  });

  it("creates a webhook and shows the generated secret once", async () => {
    const { user, fetchMock } = renderApp({
      route: tenantRoute,
      api: tenantApi({
        [`GET ${TEN}/webhooks`]: [],
        [`POST ${TEN}/webhooks`]: (req) => json({ ...v6.webhook, ...(req.body as object), id: "wh-9", secret: "whsec_generated123" }, 201),
      }),
    });
    expect(await screen.findByText("No webhooks")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add webhook" }));
    const dialog = await screen.findByRole("dialog", { name: "Add webhook" });
    const form = within(dialog).getByRole("form", { name: "Add webhook" });
    // Validation first.
    await user.click(within(dialog).getByRole("button", { name: "Add webhook" }));
    expect(within(form).getByText("Required.")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${TEN}/webhooks`)).toHaveLength(0);

    await user.type(within(form).getByLabelText("Payload URL"), "https://deploy.example.com/hook");
    await user.type(within(form).getByLabelText("Description"), "Deploys");
    await user.click(within(form).getByRole("button", { name: "Add event mr.merged" }));
    await user.click(within(dialog).getByRole("button", { name: "Add webhook" }));

    await waitFor(() => expect(fetchMock.find(`POST ${TEN}/webhooks`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${TEN}/webhooks`)[0]?.body).toEqual({
      url: "https://deploy.example.com/hook",
      events: ["branch.pushed", "mr.merged"],
      active: true,
      description: "Deploys",
    });
    const created = await screen.findByRole("dialog", { name: "Webhook created" });
    expect(within(created).getByLabelText("Signing secret")).toHaveValue("whsec_generated123");
    expect(within(created).getByText("Copy the secret now")).toBeInTheDocument();
    await user.click(within(created).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("shows deliveries, their response, and redelivers and pings", async () => {
    const { user, fetchMock } = renderApp({
      route: tenantRoute,
      api: tenantApi({
        [`GET ${TEN}/webhooks`]: [v6.webhook],
        [`GET ${TEN}/webhooks/wh-1/deliveries`]: [v6.delivery, v6.failedDelivery],
        [`GET ${TEN}/webhooks/wh-1/deliveries/dl-2`]: v6.failedDelivery,
        [`POST ${TEN}/webhooks/wh-1/deliveries/dl-2/redeliver`]: { ...v6.failedDelivery, id: "dl-3", redelivery: true },
        [`POST ${TEN}/webhooks/wh-1/test`]: { ...v6.delivery, id: "dl-4", event: "ping" },
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Deliveries for https://ci.example.com/hooks/wt" }));
    const sheet = await screen.findByRole("dialog", { name: "Recent deliveries" });
    const list = await within(sheet).findByRole("list", { name: "Deliveries" });
    const items = within(list).getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("200");
    expect(items[0]).toHaveTextContent("87 ms");
    expect(items[1]).toHaveTextContent("network error");
    expect(items[1]).toHaveTextContent("3 attempts");
    expect(fetchMock.find(`GET ${TEN}/webhooks/wh-1/deliveries`)[0]?.url.searchParams.get("limit")).toBe("50");

    await user.click(within(items[1]!).getByRole("button", { expanded: false }));
    const detail = await within(sheet).findByRole("group", { name: "Delivery dl-2" });
    expect(detail).toHaveTextContent("dial tcp: connection refused");
    expect(within(detail).getByLabelText("Payload")).toHaveTextContent('"event": "branch.pushed"');
    await user.click(within(detail).getByRole("button", { name: "Redeliver" }));
    await waitFor(() => expect(fetchMock.find(`POST ${TEN}/webhooks/wh-1/deliveries/dl-2/redeliver`)).toHaveLength(1));

    await user.click(within(sheet).getByRole("button", { name: "Send test ping" }));
    await waitFor(() => expect(fetchMock.find(`POST ${TEN}/webhooks/wh-1/test`)).toHaveLength(1));
  });

  it("deletes after confirmation", async () => {
    const { user, fetchMock } = renderApp({
      route: tenantRoute,
      api: tenantApi({ [`GET ${TEN}/webhooks`]: [v6.webhook], [`DELETE ${TEN}/webhooks/wh-1`]: noContent() }),
    });
    await user.click(await screen.findByRole("button", { name: "Delete https://ci.example.com/hooks/wt" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Delete this webhook?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete webhook" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${TEN}/webhooks/wh-1`)).toHaveLength(1));
  });

  it("explains when webhooks are not available", async () => {
    renderApp({ route: tenantRoute, api: tenantApi({ [`GET ${TEN}/webhooks`]: noEndpoint("GET", `${TEN}/webhooks`) }) });
    expect(await screen.findByText("Webhooks is not available on this server")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add webhook" })).not.toBeInTheDocument();
  });

  it("manages tree webhooks in the tree settings", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/settings",
      api: treeRoutes({
        [`GET ${T}/webhooks`]: [{ ...v6.webhook, tree_id: "tree-web" }],
        [`PATCH ${T}/webhooks/wh-1`]: (req) => json({ ...v6.webhook, ...(req.body as object) }),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Edit https://ci.example.com/hooks/wt" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit webhook" });
    expect(within(dialog).getByText("Leave empty to keep the current secret.")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Events"), "tag.pushed{Enter}");
    await user.click(within(dialog).getByRole("button", { name: "Save webhook" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${T}/webhooks/wh-1`)).toHaveLength(1));
    expect(fetchMock.find(`PATCH ${T}/webhooks/wh-1`)[0]?.body).toEqual({
      url: "https://ci.example.com/hooks/wt",
      description: "Trigger CI",
      events: ["branch.pushed", "mr.*", "tag.pushed"],
      active: true,
    });
  });
});

describe("code search", () => {
  it("searches from the URL and groups results with links to the file view", async () => {
    const { fetchMock } = renderApp({
      route: "/search/code?q=greeting&tenant=acme",
      api: { "GET /api/v1/search/code": v6.codeResults },
    });
    const web = await screen.findByRole("region", { name: "Results in acme/web" });
    const file = within(web).getByRole("article", { name: "src/app.ts" });
    expect(file).toHaveTextContent("2 matches");
    expect(within(file).getByRole("link", { name: "src/app.ts" })).toHaveAttribute("href", "/trees/acme/web/files?snapshot=snap-3&path=src%2Fapp.ts");
    expect(within(file).getByRole("link", { name: "Line 2" })).toHaveAttribute("href", "/trees/acme/web/files?snapshot=snap-3&path=src%2Fapp.ts#L2");
    expect(within(file).getByRole("link", { name: "Line 1" })).toBeInTheDocument(); // context line
    expect(within(file).getAllByText("greeting", { selector: "mark" }).length).toBeGreaterThan(0);
    expect(screen.getByRole("region", { name: "Results in acme/api" })).toHaveTextContent("server/greeting.go");
    expect(screen.getByText(/searched 4 trees/)).toBeInTheDocument();

    const q = fetchMock.find("GET /api/v1/search/code")[0]!.url.searchParams;
    expect(q.get("q")).toBe("greeting");
    expect(q.get("tenant")).toBe("acme");
    expect(q.get("context")).toBe("2");
    expect(q.has("regex")).toBe(false);
  });

  it("submits filters and toggles into the URL", async () => {
    const { user, router, fetchMock } = renderApp({ route: "/search/code", api: { "GET /api/v1/search/code": v6.codeResults } });
    expect(await screen.findByText("Search code across your trees")).toBeInTheDocument();
    const form = screen.getByRole("search", { name: "Code search" });
    await user.type(within(form).getByLabelText("Search code"), "Greet\\w+");
    await user.click(within(form).getByRole("button", { name: "Regular expression" }));
    await user.click(within(form).getByRole("button", { name: "Match case" }));
    await user.type(within(form).getByLabelText("Path filter"), "src/**");
    await user.click(within(form).getByRole("button", { name: "Search" }));
    await waitFor(() => expect(router.state.location.search).toBe("?q=Greet%5Cw%2B&path=src%2F**&regex=1&case=1"));
    await waitFor(() => expect(fetchMock.find("GET /api/v1/search/code")).toHaveLength(1));
    const q = fetchMock.find("GET /api/v1/search/code")[0]!.url.searchParams;
    expect(q.get("regex")).toBe("true");
    expect(q.get("case")).toBe("true");
    expect(q.get("path")).toBe("src/**");
  });

  it("does not search short literal queries", async () => {
    const { fetchMock } = renderApp({ route: "/search/code?q=ab", api: { "GET /api/v1/search/code": v6.codeResults } });
    expect(await screen.findByText("Query too short")).toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/search/code")).toHaveLength(0);
  });

  it("explains when code search is not available", async () => {
    renderApp({ route: "/search/code?q=greeting", api: { "GET /api/v1/search/code": apiError(501, "not_implemented", "no index") } });
    expect(await screen.findByText("Code search is not available on this server")).toBeInTheDocument();
  });

  it("is reachable from the sidebar and the command palette", async () => {
    const { user, router } = renderApp({ route: "/nowhere", api: { "GET /api/v1/search": { tenants: [], trees: [], branches: [], accounts: [] } } });
    await screen.findByRole("heading", { name: "Page not found" });
    expect(within(screen.getByRole("navigation", { name: "Main" })).getByRole("link", { name: "Code search" })).toHaveAttribute(
      "href",
      "/search/code",
    );
    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.type(within(dialog).getByRole("combobox"), "handleBlob");
    await user.click(await within(dialog).findByRole("option", { name: /Search code for/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/search/code"));
    expect(router.state.location.search).toBe("?q=handleBlob");
  });
});

describe("blame", () => {
  it("shows a gutter with snapshot, author and age per run of lines", async () => {
    const at = new Date(Date.now() - 3 * 86_400_000).toISOString();
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/files?path=src/app.ts",
      api: treeRoutes({
        [`GET ${T}/snapshots/snap-3`]: ffx.filesDetail,
        [`GET ${T}/blob/snap-3/src/app.ts`]: () => new Response(ffx.appSource),
        [`GET ${T}/blame/snap-3/src/app.ts`]: {
          snapshot: "snap-3",
          lines: [
            { line: 1, text: "// entry point", snapshot_id: "snap-1", author: "alice", message: "Initial import", at },
            { line: 2, text: "const greeting = 'hello';", snapshot_id: "snap-1", author: "alice", message: "Initial import", at },
            { line: 3, text: "export default greeting;", snapshot_id: "snap-3", author: "bob", message: "Export greeting", at },
          ],
        },
      }),
    });
    await user.click(await screen.findByRole("tab", { name: "Blame" }));
    const region = await screen.findByRole("region", { name: "Blame of src/app.ts" });
    const rows = within(region).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(within(rows[0]!).getByRole("link", { name: /snap1/ })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-1");
    expect(rows[0]).toHaveTextContent("alice");
    expect(rows[0]).toHaveTextContent("3 days ago");
    expect(within(rows[1]!).queryByRole("link")).not.toBeInTheDocument();
    expect(within(rows[2]!).getByRole("link", { name: /snap3/ })).toBeInTheDocument();
    expect(rows[2]).toHaveTextContent("bob");
    expect(fetchMock.find(`GET ${T}/blame/snap-3/src/app.ts`)).toHaveLength(1);
  });

  it("explains when blame is not available", async () => {
    const { user } = renderApp({
      route: "/trees/acme/web/files?path=src/app.ts",
      api: treeRoutes({
        [`GET ${T}/snapshots/snap-3`]: ffx.filesDetail,
        [`GET ${T}/blob/snap-3/src/app.ts`]: () => new Response(ffx.appSource),
        [`GET ${T}/blame/snap-3/src/app.ts`]: noEndpoint("GET", `${T}/blame/snap-3/src/app.ts`),
      }),
    });
    await user.click(await screen.findByRole("tab", { name: "Blame" }));
    expect(await screen.findByText("Blame is not available on this server")).toBeInTheDocument();
  });
});

describe("server-side merge", () => {
  const MR = `${T}/merge-requests/7`;
  const DETAIL = "/trees/acme/web/merge-requests/7";
  const routes = (extra: Record<string, MockRoute> = {}) =>
    treeRoutes({ [`GET ${MR}`]: mr.approvedMr, [`GET ${T}/compare`]: mr.compareDiverged, ...extra });
  const readiness = () => screen.findByRole("list", { name: "Merge readiness" });

  it("merges diverged branches with a picked method and message", async () => {
    let current: object = mr.approvedMr;
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: routes({
        [`GET ${MR}`]: () => json(current),
        [`GET ${MR}/mergeability`]: { fast_forward: false, methods: ["merge", "squash"], conflicts: null },
        [`POST ${MR}/merge`]: () => {
          current = { ...mr.mergedMr, merge_method: "squash" };
          return json(current);
        },
      }),
    });
    const list = await readiness();
    await waitFor(() => expect(list.querySelector('[data-check="fast_forward"]')).toHaveAttribute("data-status", "pass"));
    expect(list).toHaveTextContent("Mergeable on the server (merge, squash)");
    await user.click(screen.getByRole("button", { name: "Merge" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Merge !7 into main?" });
    expect(within(confirm).getByRole("radio", { name: "Fast-forward" })).toBeDisabled();
    expect(within(confirm).getByRole("radio", { name: "Merge snapshot" })).toBeChecked();
    expect(confirm).toHaveTextContent("A merge snapshot of feature/login into main is created on the server.");
    expect(within(confirm).getByLabelText("Merge message")).toHaveAttribute("placeholder", "Merge branch 'feature/login' into main (!7)");

    await user.click(within(confirm).getByRole("radio", { name: "Squash" }));
    expect(confirm).toHaveTextContent("squashed into one new snapshot");
    await user.type(within(confirm).getByLabelText("Squash message"), "Login page");
    await user.click(within(confirm).getByRole("button", { name: "Merge" }));

    await waitFor(() => expect(fetchMock.find(`POST ${MR}/merge`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/merge`)[0]?.body).toEqual({ method: "squash", message: "Login page" });
    const review = screen.getByRole("complementary", { name: "Review" });
    expect(await within(review).findByText("Squash")).toBeInTheDocument();
  });

  it("blocks on conflicts reported by mergeability", async () => {
    renderApp({
      route: DETAIL,
      api: routes({ [`GET ${MR}/mergeability`]: { fast_forward: false, methods: ["merge"], conflicts: [{ path: "src/a.ts", kind: "both_modified" }] } }),
    });
    const list = await readiness();
    await waitFor(() => expect(list).toHaveTextContent("1 conflicting path"));
    expect(screen.getByRole("button", { name: "Merge" })).toBeDisabled();
  });

  it("lists conflicts from a 409 merge_conflict", async () => {
    const { user } = renderApp({
      route: DETAIL,
      api: routes({
        [`GET ${MR}/mergeability`]: { fast_forward: false, methods: ["merge"], conflicts: [] },
        [`POST ${MR}/merge`]: apiError(409, "merge_conflict", "merge conflicts", {
          conflicts: [
            { path: "src/login.tsx", kind: "both_modified" },
            { path: "logo.png", kind: "binary" },
          ],
        }),
      }),
    });
    await readiness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Merge" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "Merge" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Merge" }));
    const heading = await screen.findByText("Merge conflicts");
    const alert = heading.closest('[role="alert"]')!;
    const conflicts = within(alert as HTMLElement).getByRole("list", { name: "Conflicts" });
    expect(conflicts).toHaveTextContent("src/login.tsx");
    expect(conflicts).toHaveTextContent("binary");
  });

  it("keeps the fast-forward flow on servers without mergeability", async () => {
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: treeRoutes({
        [`GET ${MR}`]: mr.approvedMr,
        [`GET ${T}/compare`]: mr.compareReady,
        [`GET ${MR}/mergeability`]: noEndpoint("GET", `${MR}/mergeability`),
        [`POST ${MR}/merge`]: mr.mergedMr,
      }),
    });
    await readiness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Merge" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "Merge" }));
    const confirm = await screen.findByRole("alertdialog");
    expect(within(confirm).queryByRole("radio")).not.toBeInTheDocument();
    await user.click(within(confirm).getByRole("button", { name: "Merge" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/merge`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/merge`)[0]?.body).toBeUndefined();
  });

  it("saves merge methods on a protection", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({
        [`GET ${T}/protections`]: [{ ...fx.protection, merge_methods: ["squash"] }],
        [`PUT ${T}/protections`]: (req) => json(req.body),
        [`GET ${T}/tag-protections`]: [],
      }),
    });
    const table = await screen.findByRole("table", { name: "Branch protections" });
    expect(within(table).getByText("squash only")).toBeInTheDocument();
    await user.click(within(table).getByRole("button", { name: "Edit main" }));
    const form = await screen.findByRole("form", { name: "Edit protection" });
    expect(within(form).getByRole("checkbox", { name: "Allow Squash" })).toBeChecked();
    await user.click(within(form).getByRole("checkbox", { name: "Allow Fast-forward" }));
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/protections`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/protections`)[0]?.body).toMatchObject({ pattern: "main", merge_methods: ["fast-forward", "squash"] });
  });
});

describe("policy history", () => {
  const route = "/tenants/acme?tab=policies";
  const P = `${TEN}/policies/pol-1`;

  it("lists versions, diffs them and rolls back with a comment", async () => {
    const { user, fetchMock } = renderApp({
      route,
      api: {
        ...ix.tenantIamRoutes(),
        [`GET ${P}/versions`]: v6.policyVersions,
        [`GET ${P}/diff`]: (req) =>
          json({
            from: Number(req.url.searchParams.get("from")),
            to: Number(req.url.searchParams.get("to")),
            changes: [
              {
                op: "change",
                sid: "fri",
                before: { ...ix.policy.statements[0], sid: "fri", actions: ["branch:push", "tag:push"] },
                after: { ...ix.policy.statements[0], sid: "fri" },
              },
            ],
          }),
        [`POST ${P}/rollback`]: { ...ix.policy, version: 4 },
      },
    });
    await user.click(await screen.findByRole("button", { name: "History of no-friday-deploys" }));
    const sheet = await screen.findByRole("dialog", { name: "Version history" });
    const versions = await within(sheet).findByRole("list", { name: "Policy versions" });
    const items = within(versions).getAllByRole("listitem");
    expect(items.map((i) => i.textContent?.slice(0, 2))).toEqual(["v3", "v2", "v1"]);
    expect(items[0]).toHaveTextContent("current");
    expect(items[1]).toHaveTextContent("also block tags");

    // Default comparison: v2 → v3, with the client-side metadata change.
    const diff = await within(sheet).findByRole("region", { name: "Changes from v2 to v3" });
    expect(within(diff).getByText("Enabled").nextElementSibling).toHaveTextContent("no");
    const changes = await within(diff).findByRole("list", { name: "Statement changes" });
    expect(changes).toHaveTextContent("changed");
    const removed = [...changes.querySelectorAll('[data-kind="del"]')].map((e) => e.textContent?.trim());
    expect(removed.some((t) => /^-\s*"tag:push"$/.test(t ?? ""))).toBe(true);
    expect(changes.querySelector('[data-kind="add"]')).toHaveClass("font-bold");
    expect(fetchMock.find(`GET ${P}/diff`).at(-1)?.url.searchParams.get("from")).toBe("2");

    await user.click(within(sheet).getByRole("button", { name: "Compare v1 with v2" }));
    await within(sheet).findByRole("region", { name: "Changes from v1 to v2" });

    await user.click(within(sheet).getByRole("button", { name: "Roll back to v1" }));
    const dialog = await screen.findByRole("dialog", { name: "Roll back no-friday-deploys to v1?" });
    await user.type(within(dialog).getByLabelText("Comment"), "tags were fine");
    await user.click(within(dialog).getByRole("button", { name: "Roll back to v1" }));
    await waitFor(() => expect(fetchMock.find(`POST ${P}/rollback`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${P}/rollback`)[0]?.body).toEqual({ version: 1, comment: "tags were fine" });
  });

  it("offers an owner override for config-managed policies", async () => {
    let calls = 0;
    const { user, fetchMock } = renderApp({
      route,
      api: {
        ...ix.tenantIamRoutes(),
        [`GET ${P}/versions`]: v6.policyVersions,
        [`GET ${P}/diff`]: { from: 2, to: 3, changes: [] },
        [`POST ${P}/rollback`]: () => (++calls === 1 ? apiError(409, "managed_by_config", "policy is managed by config") : json(ix.policy)),
      },
    });
    await user.click(await screen.findByRole("button", { name: "History of no-friday-deploys" }));
    const sheet = await screen.findByRole("dialog", { name: "Version history" });
    await user.click(await within(sheet).findByRole("button", { name: "Roll back to v2" }));
    const dialog = await screen.findByRole("dialog", { name: "Roll back no-friday-deploys to v2?" });
    await user.click(within(dialog).getByRole("button", { name: "Roll back to v2" }));
    expect(await within(dialog).findByText("Managed by config")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Roll back to v2" })).toBeDisabled();
    await user.click(within(dialog).getByRole("checkbox"));
    await user.click(within(dialog).getByRole("button", { name: "Roll back to v2" }));
    await waitFor(() => expect(fetchMock.find(`POST ${P}/rollback`)).toHaveLength(2));
    expect(fetchMock.find(`POST ${P}/rollback`)[1]?.url.searchParams.get("force")).toBe("true");
  });

  it("lists deleted policies and explains a missing history backend", async () => {
    const { user } = renderApp({
      route,
      api: {
        ...ix.tenantIamRoutes(),
        [`GET ${TEN}/policies/deleted`]: [{ ...ix.policy, id: "pol-old", name: "old-policy" }],
        [`GET ${TEN}/policies/pol-old/versions`]: noEndpoint("GET", `${TEN}/policies/pol-old/versions`),
      },
    });
    await user.click(await screen.findByRole("button", { name: "Deleted policies" }));
    const sheet = await screen.findByRole("dialog", { name: "Deleted policies" });
    await user.click(await within(sheet).findByRole("button", { name: "History of old-policy" }));
    expect(await screen.findByText("Policy history is not available on this server")).toBeInTheDocument();
  });
});

describe("staged files", () => {
  const S = `${T}/staged/staged-bob/files`;
  const stagedRoutes = (extra: Record<string, MockRoute> = {}) =>
    treeRoutes({
      [`GET ${T}/staged`]: [{ ...ffx.stagedWithOverlap[0]!, manifest_hash: "ab".repeat(32) }, { ...ffx.stagedWithOverlap[1]! }],
      [`GET ${T}/staged/summary`]: [],
      ...extra,
    });

  it("browses a staged entry's files and contents", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/staged",
      api: stagedRoutes({
        [`GET ${S}`]: {
          staged: { ...ffx.stagedWithOverlap[0]!, manifest_hash: "ab".repeat(32) },
          files: [
            { path: "README.md", hash: "11".repeat(32), size: 6, executable: false },
            { path: "src/auth.ts", hash: "22".repeat(32), size: 20, executable: false },
            { path: "src/login.tsx", hash: "33".repeat(32), size: 30, executable: false },
          ],
          restricted_count: 1,
        },
        [`GET ${S}/src/auth.ts`]: () => new Response("export const auth = 1;\n"),
        [`GET ${S}/src/login.tsx`]: () => apiError(403, "path_denied", "reading src/login.tsx is denied by policy hide"),
        [`GET ${S}/README.md`]: () => new Response("# Hi\n"),
      }),
    });
    const buttons = await screen.findAllByRole("button", { name: /^Browse files staged by/ });
    expect(buttons).toHaveLength(1);
    await user.click(buttons[0]!);
    const sheet = await screen.findByRole("dialog", { name: "Staged files" });
    expect(await within(sheet).findByRole("alert", { name: "Restricted paths" })).toHaveTextContent("1 path is hidden");
    const list = within(sheet).getByRole("list", { name: "Staged files" });
    // Only changed files by default.
    expect(within(list).getAllByRole("button").map((b) => b.textContent)).toEqual(["src/auth.ts", "src/login.tsx"]);
    const view = await within(sheet).findByRole("region", { name: "Staged contents of src/auth.ts" });
    expect(view).toHaveTextContent("export const auth = 1;");

    await user.click(within(list).getByRole("button", { name: "src/login.tsx" }));
    expect(await within(sheet).findByText(/denied by policy hide/)).toBeInTheDocument();

    await user.click(within(sheet).getByRole("button", { name: /^All/ }));
    expect(within(list).getAllByRole("button")).toHaveLength(3);
    fireEvent.change(within(sheet).getByLabelText("Filter staged files"), { target: { value: "read" } });
    expect(within(list).getAllByRole("button").map((b) => b.textContent)).toEqual(["README.md"]);
    expect(fetchMock.find(`GET ${S}`)).toHaveLength(1);
  });

  it("explains when browsing is not available", async () => {
    const { user } = renderApp({
      route: "/trees/acme/web/staged",
      api: stagedRoutes({ [`GET ${S}`]: noEndpoint("GET", S) }),
    });
    await user.click(await screen.findByRole("button", { name: /^Browse files staged by/ }));
    expect(await screen.findByText("Browsing staged files is not available on this server")).toBeInTheDocument();
  });
});

describe("tree access config", () => {
  const A = `${T}/access-config`;
  const DOC = JSON.stringify({ policies: [{ name: "hide", enabled: true, statements: [{ sid: "s", effect: "deny", actions: ["tree:read"], resources: ["wt:path/acme/web/secret/**"], principals: ["*"] }] }] });

  it("plans and applies a tree document", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/access",
      api: treeRoutes({
        [`GET ${T}/grants`]: [],
        [`POST ${A}/plan`]: { accepted: true, errors: null, changes: ["create policy hide"] },
        [`POST ${A}/apply`]: { accepted: true, errors: [], changes: ["create policy hide"] },
      }),
    });
    expect(await screen.findByText("No document applied yet")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Access document (JSON)"), { target: { value: DOC } });
    await user.click(screen.getByRole("button", { name: "Plan" }));
    expect(await screen.findByRole("list", { name: "Changes" })).toHaveTextContent("create policy hide");
    expect(fetchMock.find(`POST ${A}/plan`)[0]?.body).toEqual({ content: JSON.parse(DOC) as unknown });
    await user.click(screen.getByRole("button", { name: "Apply" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Apply this access document?" });
    expect(confirm).toHaveTextContent("acme/web");
    await user.click(within(confirm).getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(fetchMock.find(`POST ${A}/apply`)).toHaveLength(1));
  });

  it("shows the last applied tree document and handles a missing backend", async () => {
    renderApp({
      route: "/trees/acme/web/access",
      api: treeRoutes({ [`GET ${T}/grants`]: [], [`GET ${A}`]: noEndpoint("GET", A) }),
    });
    expect(await screen.findByText("Tree access config is not available on this server")).toBeInTheDocument();
  });
});
