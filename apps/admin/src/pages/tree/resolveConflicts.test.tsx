/** v0.8 conflict-ui (docs/ENGINEERING_PLAN.md §16.9): browser conflict resolution, banner, rebase + delete source branch. */
import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { hasConflictMarkers, markerOffsets, normConflictReport, type ConflictReport } from "@/api/conflicts";
import type { MergeRequest } from "@/api/types";
import * as mr from "@/test/fixtures.mr";
import { apiError, json, type MockRoute } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";
const MR = `${T}/merge-requests/7`;
const DETAIL = "/trees/acme/web/merge-requests/7";
const RESOLVE = `${DETAIL}/resolve`;

const markers =
  "line 1\n<<<<<<< feature/login\nours a\n||||||| base\nbase a\n=======\ntheirs a\n>>>>>>> main\nmiddle\n<<<<<<< feature/login\nours b\n=======\ntheirs b\n>>>>>>> main\n";

const side = (text?: string, exists = true) => ({ exists, size: text?.length ?? 0, executable: false, hash: "h", ...(text ? { text } : {}) });

const report: ConflictReport = {
  number: 7,
  state: "open",
  source_branch: "feature/login",
  target_branch: "main",
  source_tip: "snap-4",
  target_tip: "snap-3",
  merge_base: "snap-1",
  resolvable: true,
  reason: "",
  total: 2,
  truncated: false,
  files: [
    {
      path: "src/a.txt",
      kind: "content",
      binary: false,
      too_large: false,
      restricted: false,
      base: side("line 1\nbase a\nmiddle\n"),
      ours: side("line 1\nours a\nmiddle\nours b\n"),
      theirs: side("line 1\ntheirs a\nmiddle\ntheirs b\n"),
      markers,
      regions: [],
    },
    {
      path: "logo.png",
      kind: "binary",
      binary: true,
      too_large: false,
      restricted: false,
      base: side(undefined, false),
      ours: side(),
      theirs: side(),
      regions: [],
    },
  ],
};

function resolveRoutes(extra: Record<string, MockRoute> = {}): Record<string, MockRoute> {
  return treeRoutes({
    [`GET ${MR}/conflicts`]: report,
    [`GET ${MR}`]: mr.approvedMr,
    [`GET ${T}/compare`]: mr.compareReady,
    [`GET ${MR}/comments`]: [],
    ...extra,
  });
}

const resolvedCount = () => document.querySelector("[data-resolved-count]");
const fileButton = (path: string) => within(screen.getByRole("navigation", { name: "Conflicting files" })).getByRole("button", { name: new RegExp(path) });

describe("conflict helpers", () => {
  it("detects markers and normalises null lists", () => {
    expect(hasConflictMarkers(markers)).toBe(true);
    expect(hasConflictMarkers("a\n==== not a marker\n")).toBe(false);
    expect(hasConflictMarkers("a\n=======\n")).toBe(true);
    expect(markerOffsets(markers)).toEqual([7, markers.indexOf("<<<<<<<", 8)]);
    const r = normConflictReport({ ...report, files: [{ ...report.files[0]!, regions: null as never }] });
    expect(r.files[0]!.regions).toEqual([]);
    expect(normConflictReport({ files: null as never }).files).toEqual([]);
  });
});

describe("ResolveConflictsPage", () => {
  it("renders the files and the three panes", async () => {
    renderApp({ route: RESOLVE, api: resolveRoutes() });
    const result = await screen.findByRole("textbox", { name: "Result for src/a.txt" });
    expect(result).toHaveValue(markers);
    expect(resolvedCount()).toHaveTextContent("0 of 2 resolved");
    expect(screen.getByRole("region", { name: "Ours (feature/login)" })).toHaveTextContent("ours b");
    expect(screen.getByRole("region", { name: "Theirs (main)" })).toHaveTextContent("theirs b");
    expect(document.querySelector("[data-marker-count]")).toHaveTextContent("2 conflicts left");
    expect(fileButton("src/a.txt")).toHaveAttribute("aria-current", "true");
    expect(fileButton("logo.png")).toHaveAttribute("data-resolved", "false");
    expect(screen.getByRole("button", { name: "Take base" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Commit resolution" })).toBeDisabled();
  });

  it("navigates between conflict markers and resolves by editing", async () => {
    const { user } = renderApp({ route: RESOLVE, api: resolveRoutes() });
    const result = await screen.findByRole<HTMLTextAreaElement>("textbox", { name: "Result for src/a.txt" });
    const [first, second] = markerOffsets(markers);
    await user.click(screen.getByRole("button", { name: "Next conflict" }));
    expect(result.selectionStart).toBe(first);
    await user.click(screen.getByRole("button", { name: "Next conflict" }));
    expect(result.selectionStart).toBe(second);
    await user.click(screen.getByRole("button", { name: "Previous conflict" }));
    expect(result.selectionStart).toBe(first);

    await user.clear(result);
    await user.type(result, "line 1\nmerged\n");
    expect(document.querySelector("[data-marker-count]")).toHaveTextContent("No conflict markers");
    expect(resolvedCount()).toHaveTextContent("1 of 2 resolved");
    expect(fileButton("src/a.txt")).toHaveAttribute("data-resolved", "true");
  });

  it("handles binary files with take / delete only", async () => {
    const { user } = renderApp({ route: RESOLVE, api: resolveRoutes() });
    await screen.findByRole("textbox", { name: "Result for src/a.txt" });
    await user.click(fileButton("logo.png"));
    const section = screen.getByRole("region", { name: "Resolve logo.png" });
    expect(within(section).queryByRole("textbox")).not.toBeInTheDocument();
    expect(section).toHaveTextContent("Binary file: it cannot be edited in the browser.");
    expect(within(section).queryByRole("button", { name: "Take base" })).not.toBeInTheDocument();
    expect(within(section).queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
    await user.click(within(section).getByRole("button", { name: "Take ours" }));
    expect(within(section).getByRole("button", { name: "Take ours" })).toHaveAttribute("aria-pressed", "true");
    expect(resolvedCount()).toHaveTextContent("1 of 2 resolved");
  });

  it("submits take / content / delete resolutions with the tips and returns to the merge request", async () => {
    const { user, fetchMock, toasts } = renderApp({
      route: RESOLVE,
      api: resolveRoutes({
        [`POST ${MR}/resolve`]: { merge_request: mr.approvedMr, snapshot: { id: "snap-9", branch_name: "feature/login" }, paths: null },
      }),
    });
    await screen.findByRole("textbox", { name: "Result for src/a.txt" });
    const submit = screen.getByRole("button", { name: "Commit resolution" });
    await user.click(screen.getByRole("button", { name: "Take theirs" }));
    expect(resolvedCount()).toHaveTextContent("1 of 2 resolved");
    expect(screen.getByText(/Resolved with the/)).toHaveTextContent("theirs version (main)");
    expect(submit).toBeDisabled();

    // Back to editing keeps the markers: unresolved again.
    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(resolvedCount()).toHaveTextContent("0 of 2 resolved");
    await user.click(screen.getByRole("button", { name: "Take theirs" }));

    await user.click(fileButton("logo.png"));
    await user.click(screen.getByRole("button", { name: "Delete file" }));
    expect(resolvedCount()).toHaveTextContent("2 of 2 resolved");
    await user.type(screen.getByLabelText("Snapshot message"), "Fix conflicts");
    expect(submit).toBeEnabled();
    await user.click(submit);

    await waitFor(() => expect(fetchMock.find(`POST ${MR}/resolve`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/resolve`)[0]?.body).toEqual({
      files: [
        { path: "src/a.txt", take: "theirs" },
        { path: "logo.png", delete: true },
      ],
      message: "Fix conflicts",
      source_tip: "snap-4",
      target_tip: "snap-3",
    });
    expect(await screen.findByRole("heading", { name: /Add login page/, level: 2 })).toBeInTheDocument();
    expect(toasts.getSnapshot().map((t) => t.title)).toContain("Resolved 2 conflicts of !7");
  });

  it("sends edited content", async () => {
    const { user, fetchMock } = renderApp({
      route: RESOLVE,
      api: resolveRoutes({ [`POST ${MR}/resolve`]: { merge_request: mr.approvedMr, snapshot: { id: "snap-9" }, paths: ["src/a.txt", "logo.png"] } }),
    });
    const result = await screen.findByRole("textbox", { name: "Result for src/a.txt" });
    await user.clear(result);
    await user.type(result, "merged\n");
    await user.click(fileButton("logo.png"));
    await user.click(screen.getByRole("button", { name: "Take theirs" }));
    await user.click(screen.getByRole("button", { name: "Commit resolution" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/resolve`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/resolve`)[0]?.body).toEqual({
      files: [
        { path: "src/a.txt", content: "merged\n" },
        { path: "logo.png", take: "theirs" },
      ],
      source_tip: "snap-4",
      target_tip: "snap-3",
    });
  });

  it("offers a reload on 409 resolve_outdated", async () => {
    const { user, fetchMock } = renderApp({
      route: RESOLVE,
      api: resolveRoutes({ [`POST ${MR}/resolve`]: apiError(409, "resolve_outdated", "source branch moved") }),
    });
    await screen.findByRole("textbox", { name: "Result for src/a.txt" });
    await user.click(screen.getByRole("button", { name: "Take ours" }));
    await user.click(fileButton("logo.png"));
    await user.click(screen.getByRole("button", { name: "Take ours" }));
    await user.click(screen.getByRole("button", { name: "Commit resolution" }));
    const title = await screen.findByText("The branches moved");
    const alert = title.closest('[role="alert"]') as HTMLElement;
    expect(alert).toHaveTextContent("source branch moved");
    await user.click(within(alert).getByRole("button", { name: "Reload conflicts" }));
    await waitFor(() => expect(fetchMock.find(`GET ${MR}/conflicts`)).toHaveLength(2));
    await waitFor(() => expect(resolvedCount()).toHaveTextContent("0 of 2 resolved"));
    expect(screen.queryByText("The branches moved")).not.toBeInTheDocument();
  });

  it("shows 400 refusals inline", async () => {
    const { user } = renderApp({
      route: RESOLVE,
      api: resolveRoutes({ [`POST ${MR}/resolve`]: apiError(400, "bad_request", "unresolved path src/b.txt") }),
    });
    await screen.findByRole("textbox", { name: "Result for src/a.txt" });
    await user.click(screen.getByRole("button", { name: "Take ours" }));
    await user.click(fileButton("logo.png"));
    await user.click(screen.getByRole("button", { name: "Delete file" }));
    await user.click(screen.getByRole("button", { name: "Commit resolution" }));
    const title = await screen.findByText("Resolution refused");
    const alert = title.closest('[role="alert"]') as HTMLElement;
    expect(alert).toHaveTextContent("unresolved path src/b.txt");
    expect(within(alert).queryByRole("button", { name: "Reload conflicts" })).not.toBeInTheDocument();
  });

  it("explains unresolvable conflicts", async () => {
    renderApp({
      route: RESOLVE,
      api: resolveRoutes({ [`GET ${MR}/conflicts`]: { ...report, resolvable: false, reason: "the source branch is in a fork" } }),
    });
    expect(await screen.findByText("These conflicts cannot be resolved in the browser")).toBeInTheDocument();
    expect(screen.getByText("the source branch is in a fork")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to the merge request" })).toHaveAttribute("href", DETAIL);
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("shows an empty state without conflicts", async () => {
    renderApp({
      route: RESOLVE,
      api: resolveRoutes({ [`GET ${MR}/conflicts`]: { ...report, files: null, total: 0, resolvable: false, reason: "" } }),
    });
    expect(await screen.findByText("No conflicts")).toBeInTheDocument();
  });
});

describe("merge request detail (v0.8)", () => {
  const detailRoutes = (extra: Record<string, MockRoute> = {}) =>
    treeRoutes({ [`GET ${MR}`]: mr.approvedMr, [`GET ${T}/compare`]: mr.compareReady, [`GET ${MR}/comments`]: [], ...extra });

  it("shows a conflict banner linking to the resolver", async () => {
    const conflicts = ["a", "b", "c", "d", "e", "f"].map((p) => ({ path: `src/${p}.ts`, kind: "content" }));
    renderApp({
      route: DETAIL,
      api: detailRoutes({ [`GET ${MR}/mergeability`]: { fast_forward: false, methods: ["merge"], conflicts } }),
    });
    const title = await screen.findByText("This merge request has conflicts");
    const banner = title.closest("[data-conflict-banner]") as HTMLElement;
    expect(banner).toHaveTextContent("6 conflicting paths");
    const list = within(banner).getByRole("list", { name: "Conflicting paths" });
    expect(list).toHaveTextContent("src/a.ts");
    expect(list).not.toHaveTextContent("src/f.ts");
    expect(list).toHaveTextContent("and 1 more");
    expect(within(banner).getByRole("link", { name: "Resolve conflicts" })).toHaveAttribute("href", RESOLVE);
  });

  it("has no banner without conflicts", async () => {
    renderApp({ route: DETAIL, api: detailRoutes({ [`GET ${MR}/mergeability`]: { fast_forward: true, methods: ["fast-forward"], conflicts: null } }) });
    await screen.findByRole("list", { name: "Merge readiness" });
    await waitFor(() => expect(screen.getByRole("button", { name: "Merge" })).toBeEnabled());
    expect(screen.queryByText("This merge request has conflicts")).not.toBeInTheDocument();
  });

  it("merges with rebase and deletes the source branch", async () => {
    let current: MergeRequest = mr.approvedMr;
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: detailRoutes({
        [`GET ${MR}`]: () => json(current),
        [`GET ${MR}/mergeability`]: { fast_forward: true, methods: ["fast-forward", "merge", "squash", "rebase"], conflicts: [] },
        [`POST ${MR}/merge`]: () => {
          current = { ...mr.mergedMr, merge_method: "rebase" } as MergeRequest;
          return json(current);
        },
      }),
    });
    const merge = await screen.findByRole("button", { name: "Merge" });
    await waitFor(() => expect(merge).toBeEnabled());
    await user.click(merge);
    const confirm = await screen.findByRole("alertdialog", { name: "Merge !7 into main?" });
    await user.click(within(confirm).getByRole("radio", { name: "Rebase" }));
    expect(confirm).toHaveTextContent("The snapshots of feature/login are replayed onto main on the server.");
    expect(within(confirm).queryByLabelText(/message/i)).not.toBeInTheDocument();
    await user.click(within(confirm).getByRole("checkbox", { name: "Delete source branch" }));
    await user.click(within(confirm).getByRole("button", { name: "Merge" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/merge`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/merge`)[0]?.body).toEqual({ method: "rebase", delete_source_branch: true });
    const review = screen.getByRole("complementary", { name: "Review" });
    expect(await within(review).findByText("got the rebased snapshots", { exact: false })).toBeInTheDocument();
  });

  it("omits delete_source_branch unless checked", async () => {
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: detailRoutes({
        [`GET ${MR}/mergeability`]: { fast_forward: true, methods: ["fast-forward"], conflicts: [] },
        [`POST ${MR}/merge`]: mr.mergedMr,
      }),
    });
    const merge = await screen.findByRole("button", { name: "Merge" });
    await waitFor(() => expect(merge).toBeEnabled());
    await user.click(merge);
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Merge" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/merge`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/merge`)[0]?.body).toEqual({ method: "fast-forward" });
  });
});
