import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MergeRequest } from "@/api/types";
import * as fx from "@/test/fixtures";
import * as mr from "@/test/fixtures.mr";
import * as v5 from "@/test/fixtures.v05";
import { apiError, json, type MockRoute } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";
const MR = `${T}/merge-requests/7`;
const DETAIL = "/trees/acme/web/merge-requests/7";

function routes(m: MergeRequest, extra: Record<string, MockRoute> = {}): Record<string, MockRoute> {
  return treeRoutes({
    [`GET ${MR}`]: m,
    [`GET ${T}/compare`]: mr.compareReady,
    [`GET ${MR}/comments`]: [],
    ...extra,
  });
}

const readiness = () => screen.findByRole("list", { name: "Merge readiness" });
const check = (list: HTMLElement, id: string) => list.querySelector(`[data-check="${id}"]`)!;

describe("merge request reviews (v0.5)", () => {
  it("marks approvals of an older source tip as stale and does not count them", async () => {
    renderApp({ route: DETAIL, api: routes(v5.staleMr) });
    const approvals = await screen.findByRole("list", { name: "Approvals" });
    const list = await readiness();
    await waitFor(() => expect(check(list, "approvals")).toHaveTextContent("1 of 2 required approvals"));
    const carol = within(approvals).getByText("carol").closest("[data-review]")!;
    expect(carol).toHaveAttribute("data-review", "stale");
    expect(within(carol as HTMLElement).getByText("stale")).toHaveClass("line-through");
    expect(within(approvals).getByText("dave").closest("[data-review]")).toHaveAttribute("data-review", "approved");
    expect(screen.getByText(/1 stale approval was given for an older source tip/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Merge" })).toHaveAccessibleDescription("1 more approval needed.");
  });

  it("blocks merging while changes are requested", async () => {
    renderApp({ route: DETAIL, api: routes(v5.changesRequestedMr) });
    const others = await screen.findByRole("list", { name: "Other reviews" });
    expect(within(others).getByText("changes requested")).toBeInTheDocument();
    expect(within(others).getByText("Please add tests.")).toBeInTheDocument();
    const list = await readiness();
    await waitFor(() => expect(check(list, "fast_forward")).toHaveAttribute("data-status", "pass"));
    expect(check(list, "approvals")).toHaveTextContent("2 of 2 required approvals");
    expect(screen.getByRole("button", { name: "Merge" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Merge" })).toHaveAccessibleDescription("Changes requested by erin.");
  });

  it("shows server blockers and CI checks", async () => {
    const { fetchMock } = renderApp({
      route: DETAIL,
      api: routes(mr.approvedMr, {
        [`GET ${MR}/checks`]: v5.blockedChecks,
        [`GET ${T}/ci/status`]: { snapshot: "snap-4", state: "failure", checks: v5.ciChecks },
      }),
    });
    const server = await screen.findByRole("region", { name: "Server merge checks" });
    expect(server).toHaveTextContent("Server: merging is blocked");
    expect(within(server).getByRole("list", { name: "Merge blockers" })).toHaveTextContent("ci_failed Check ci/test is failure.");
    expect(server).toHaveTextContent("1 open");
    await waitFor(() => expect(screen.getByRole("button", { name: "Merge" })).toHaveAccessibleDescription("Check ci/test is failure."));

    const ci = await screen.findByRole("list", { name: "CI checks" });
    await waitFor(() => expect(ci.querySelectorAll("[data-ci-context]")).toHaveLength(3));
    expect(ci.querySelector('[data-ci-context="ci/test"]')).toHaveAttribute("data-state", "failure");
    expect(within(ci).getByRole("link", { name: /ci\/test/ })).toHaveAttribute("href", "https://ci.example.com/run/9");
    expect(ci.querySelector('[data-ci-context="ci/lint"]')).toHaveTextContent("Required · not reported");
    expect(screen.getByText("Failed").closest("[data-ci-state]")).toHaveAttribute("data-ci-state", "failure");
    // The source tip snapshot is queried, not the branch.
    expect(fetchMock.find(`GET ${T}/ci/status`).at(-1)?.url.searchParams.get("snapshot")).toBe("snap-4");
  });

  it("degrades when the review endpoints are not implemented", async () => {
    renderApp({
      route: DETAIL,
      api: routes(mr.approvedMr, {
        [`GET ${MR}/checks`]: apiError(501, "not_implemented", "reviews are not configured"),
        [`GET ${T}/ci/status`]: apiError(501, "not_implemented", "ci is not configured"),
        [`GET ${MR}/comments`]: apiError(501, "not_implemented", "reviews are not configured"),
      }),
    });
    expect(await screen.findByText("CI checks: not available on this server.")).toBeInTheDocument();
    expect(await screen.findByText("Server merge checks: not available on this server.")).toBeInTheDocument();
    expect(await screen.findByText("Comments: not available on this server.")).toBeInTheDocument();
    const list = await readiness();
    await waitFor(() => expect(check(list, "fast_forward")).toHaveAttribute("data-status", "pass"));
    expect(screen.getByRole("button", { name: "Merge" })).toBeEnabled();
  });

  it("submits a review decision with a comment", async () => {
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: routes(fx.mergeRequest, {
        [`POST ${MR}/review`]: (req) => json({ ...fx.mergeRequest, approvals: [{ ...v5.review("alice", "acc-1"), ...(req.body as object) }] }),
      }),
    });
    const form = await screen.findByRole("form", { name: "Submit review" });
    await user.click(within(form).getByRole("radio", { name: /Request changes/ }));
    await user.click(within(form).getByRole("button", { name: "Submit review" }));
    expect(within(form).getByText("Add a comment explaining your review.")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${MR}/review`)).toHaveLength(0);

    await user.type(within(form).getByLabelText("Review comment"), "Needs tests");
    await user.click(within(form).getByRole("button", { name: "Submit review" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/review`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/review`)[0]?.body).toEqual({ decision: "changes_requested", body: "Needs tests" });
  });

  it("hides the review form from the author", async () => {
    renderApp({ route: DETAIL, api: routes(mr.ownMr) });
    await readiness();
    expect(screen.queryByRole("form", { name: "Submit review" })).not.toBeInTheDocument();
    expect(await screen.findByRole("form", { name: "Add comment" })).toBeInTheDocument();
  });

  it("shows comment threads and adds, resolves and replies", async () => {
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: routes(fx.mergeRequest, {
        [`GET ${MR}/comments`]: { comments: v5.comments },
        [`POST ${MR}/comments`]: (req) => json({ ...v5.comments[0], id: "c3", ...(req.body as object) }, 201),
        [`PATCH ${MR}/comments/c2`]: (req) => json({ ...v5.comments[1], ...(req.body as object) }),
      }),
    });
    const threads = await screen.findByRole("list", { name: "Comment threads" });
    const general = within(threads).getByRole("article", { name: "Thread General comment" });
    expect(general).toHaveTextContent("Looks good overall.");
    const file = within(threads).getByRole("article", { name: "Thread src/login.tsx:12" });
    expect(within(file).getByText(/outdated/)).toBeInTheDocument();
    expect(screen.getByText(/2 threads · 2 open/)).toBeInTheDocument();

    // alice wrote the file comment: she may resolve it.
    await user.click(within(file).getByRole("button", { name: "Resolve" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${MR}/comments/c2`)[0]?.body).toEqual({ resolved: true }));

    await user.click(within(file).getByRole("button", { name: "Reply" }));
    const reply = within(file).getByRole("form", { name: "Reply to src/login.tsx:12" });
    await user.type(within(reply).getByLabelText("Reply"), "Done");
    await user.click(within(reply).getByRole("button", { name: "Send reply" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/comments`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/comments`)[0]?.body).toEqual({ body: "Done", path: "src/login.tsx", line: 12, snapshot_id: "snap-4" });

    const form = screen.getByRole("form", { name: "Add comment" });
    await user.type(within(form).getByLabelText("Line"), "3");
    await user.type(within(form).getByLabelText("New comment"), "Typo here");
    await user.click(within(form).getByRole("button", { name: "Add comment" }));
    expect(within(form).getByText("Choose a file for a line comment.")).toBeInTheDocument();
    await user.type(within(form).getByLabelText("File (optional)"), "src/auth.ts");
    await user.click(within(form).getByRole("button", { name: "Add comment" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/comments`)).toHaveLength(2));
    expect(fetchMock.find(`POST ${MR}/comments`)[1]?.body).toEqual({ body: "Typo here", path: "src/auth.ts", line: 3, snapshot_id: "snap-4" });
  });

  it("shows the linked group and merges it", async () => {
    const linked = { ...mr.approvedMr, linked_group: "grp-1" } as MergeRequest;
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: routes(linked, {
        "GET /api/v1/tenants/acme/linked-groups/grp-1": v5.linkedGroup,
        "GET /api/v1/tenants/acme/trees": [fx.summary, { ...fx.summary, tree: { ...fx.summary.tree, id: "tree-api", name: "api" } }],
        "POST /api/v1/tenants/acme/linked-groups/grp-1/merge": { ...v5.linkedGroup, state: "merged" },
      }),
    });
    const card = await screen.findByRole("region", { name: "Linked group" });
    expect(await within(card).findByText("login rollout")).toBeInTheDocument();
    const members = within(card).getByRole("list", { name: "Group members" });
    expect(await within(members).findByRole("link", { name: "acme/api!3" })).toHaveAttribute("href", "/trees/acme/api/merge-requests/3");
    expect(within(members).getByText("(this tree)")).toBeInTheDocument();

    const list = await readiness();
    await waitFor(() => expect(check(list, "fast_forward")).toHaveAttribute("data-status", "pass"));
    expect(screen.getByRole("button", { name: "Merge" })).toHaveAccessibleDescription("Part of a linked group: merge the group instead.");

    await user.click(within(card).getByRole("button", { name: "Merge group" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Merge all" }));
    await waitFor(() => expect(fetchMock.find("POST /api/v1/tenants/acme/linked-groups/grp-1/merge")).toHaveLength(1));
  });

  it("maps merge_blocked rejections", async () => {
    const { user } = renderApp({
      route: DETAIL,
      api: routes(mr.approvedMr, { [`POST ${MR}/merge`]: apiError(409, "merge_blocked", "required check ci/test has not passed") }),
    });
    const list = await readiness();
    await waitFor(() => expect(check(list, "fast_forward")).toHaveAttribute("data-status", "pass"));
    await user.click(screen.getByRole("button", { name: "Merge" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Merge" }));
    const heading = await screen.findByText("Merge blocked");
    expect(heading.closest('[role="alert"]')).toHaveTextContent("required check ci/test has not passed");
  });
});
