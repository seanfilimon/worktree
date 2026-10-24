import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MergeRequest } from "@/api/types";
import * as fx from "@/test/fixtures";
import { apiError, json } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";
const MR = `${T}/merge-requests/7`;

/** GET returns the MR as last updated by the given action (like the real server). */
function statefulMr(action: "approve" | "merge" | "close", after: MergeRequest) {
  let current: MergeRequest = fx.mergeRequest;
  return treeRoutes({
    [`GET ${MR}`]: () => json(current),
    [`POST ${MR}/${action}`]: () => {
      current = after;
      return json(after);
    },
  });
}

describe("merge requests", () => {
  it("lists merge requests and filters by state", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/merge-requests",
      api: treeRoutes({
        [`GET ${T}/merge-requests`]: (req) =>
          json(req.url.searchParams.get("state") === "merged" ? [] : [fx.mergeRequest]),
      }),
    });
    const table = await screen.findByRole("table", { name: "Merge requests" });
    expect(within(table).getByRole("link", { name: /!7\s*Add login page/ })).toHaveAttribute("href", "/trees/acme/web/merge-requests/7");
    expect(fetchMock.find(`GET ${T}/merge-requests`)[0]?.url.searchParams.get("state")).toBe("open");

    await user.click(screen.getByRole("button", { name: "Merged" }));
    expect(await screen.findByText("No merged merge requests")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Merged" })).toHaveAttribute("aria-pressed", "true");
  });

  it("creates a merge request and opens it", async () => {
    const { user, router, fetchMock } = renderApp({
      route: "/trees/acme/web/merge-requests",
      api: treeRoutes({
        [`GET ${T}/merge-requests`]: [],
        [`POST ${T}/merge-requests`]: (req) => json({ ...fx.mergeRequest, ...(req.body as object) }, 201),
        [`GET ${MR}`]: fx.mergeRequest,
      }),
    });
    await user.click(await screen.findByRole("button", { name: "New merge request" }));
    const form = screen.getByRole("form", { name: "Create merge request" });
    expect(within(form).getByLabelText("Source branch")).toHaveValue("feature/login");
    expect(within(form).getByLabelText("Target branch")).toHaveValue("main");
    await user.type(within(form).getByLabelText("Title"), "Add login page");
    await user.click(within(form).getByRole("button", { name: "Open merge request" }));

    expect(await screen.findByRole("heading", { name: /Add login page/, level: 2 })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/trees/acme/web/merge-requests/7");
    expect(fetchMock.find(`POST ${T}/merge-requests`)[0]?.body).toEqual({
      title: "Add login page",
      description: "",
      source_branch: "feature/login",
      target_branch: "main",
    });
  });

  it("approves a merge request", async () => {
    const approved = { ...fx.mergeRequest, approvals: [{ account_id: "acc-1", username: "alice", at: new Date().toISOString() }] };
    const { user } = renderApp({
      route: "/trees/acme/web/merge-requests/7",
      api: statefulMr("approve", approved),
    });
    expect(await screen.findByText(/of 2 required/)).toHaveTextContent("0 of 2 required");
    await user.click(screen.getByRole("button", { name: "Approve" }));
    const approvals = await screen.findByRole("list", { name: "Approvals" });
    expect(within(approvals).getByText("alice")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "You approved" })).toBeDisabled();
  });

  it("explains review_required when merging without enough approvals", async () => {
    const { user } = renderApp({
      route: "/trees/acme/web/merge-requests/7",
      api: treeRoutes({
        [`GET ${MR}`]: fx.mergeRequest,
        [`POST ${MR}/merge`]: apiError(403, "forbidden", "push denied (review_required): 0 of 2 approvals"),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Merge (fast-forward)" }));
    const alert = await screen.findByText("Review required");
    const box = alert.closest("[role]")!;
    expect(box).toHaveTextContent("does not have enough approvals");
    expect(box).toHaveTextContent("0 of 2 required approvals");
    expect(box).toHaveTextContent("Server: push denied (review_required): 0 of 2 approvals");
  });

  it("explains diverged branches on a conflicting merge", async () => {
    const { user } = renderApp({
      route: "/trees/acme/web/merge-requests/7",
      api: treeRoutes({
        [`GET ${MR}`]: fx.mergeRequest,
        [`POST ${MR}/merge`]: apiError(409, "conflict", "target main is not an ancestor of feature/login"),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Merge (fast-forward)" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Branches have diverged");
    expect(alert).toHaveTextContent("wt merge");
  });

  it("merges and shows the merged state", async () => {
    const merged = { ...fx.mergeRequest, state: "merged" as const, merged_at: new Date().toISOString(), merged_snapshot: "snap-4" };
    const { user } = renderApp({
      route: "/trees/acme/web/merge-requests/7",
      api: statefulMr("merge", merged),
    });
    await user.click(await screen.findByRole("button", { name: "Merge (fast-forward)" }));
    expect(await screen.findByText("Merged", { selector: "p" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Merge (fast-forward)" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "snap4" })).toBeInTheDocument();
  });

  it("closes a merge request after confirmation", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/merge-requests/7",
      api: statefulMr("close", { ...fx.mergeRequest, state: "closed" }),
    });
    await user.click(await screen.findByRole("button", { name: "Close" }));
    expect(fetchMock.find(`POST ${MR}/close`)).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Close merge request" }));
    expect(await screen.findByText("closed")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${MR}/close`)).toHaveLength(1);
  });
});
