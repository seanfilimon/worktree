import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MergeRequest } from "@/api/types";
import type { MergeChecks, ReviewComment } from "@/api/v05tree";
import type { MROwners, OwnersView, ReviewCommentV07 } from "@/api/v07review";
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

const draftMr = { ...mr.approvedMr, draft: true } as MergeRequest;

const comment = (id: string, extra: Partial<ReviewCommentV07>): ReviewCommentV07 => ({
  id,
  tree_id: "tree-web",
  mr_number: 7,
  author_id: "acc-3",
  author: "carol",
  body: "",
  created_at: new Date(Date.now() - 60_000).toISOString(),
  updated_at: new Date(Date.now() - 60_000).toISOString(),
  ...extra,
});

const suggestion = comment("s1", {
  body: "Name it better",
  path: "src/app.ts",
  line: 1,
  snapshot_id: "snap-4",
  suggestion: { path: "src/app.ts", start_line: 1, end_line: 1, replacement: "newer\n", snapshot_id: "snap-4" },
});

const toastTitles = (toasts: { getSnapshot: () => readonly { title: string }[] }) => toasts.getSnapshot().map((t) => t.title);

describe("draft merge requests (v0.7)", () => {
  it("shows the draft badge, blocks merging and marks the request ready", async () => {
    let current: MergeRequest = draftMr;
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: routes(draftMr, {
        [`GET ${MR}`]: () => json(current),
        [`POST ${MR}/ready`]: () => {
          current = { ...draftMr, draft: false } as MergeRequest;
          return json(current);
        },
      }),
    });
    const heading = await screen.findByRole("heading", { name: /Add login page/, level: 2 });
    const header = heading.closest("header")!;
    expect(within(header).getByText("Draft").closest("[data-draft]")).toHaveAttribute("data-variant", "outline");
    const merge = screen.getByRole("button", { name: "Merge" });
    expect(merge).toBeDisabled();
    expect(merge).toHaveAccessibleDescription("Draft: mark it ready for review before merging.");

    await user.click(screen.getByRole("button", { name: "Ready for review" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/ready`)).toHaveLength(1));
    await waitFor(() => expect(within(header).queryByText("Draft")).not.toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Ready for review" })).not.toBeInTheDocument();
  });

  it("marks drafts in the list", async () => {
    renderApp({
      route: "/trees/acme/web/merge-requests",
      api: treeRoutes({ [`GET ${T}/merge-requests`]: [draftMr, { ...mr.mergeRequestList[1]! }] }),
    });
    const table = await screen.findByRole("table", { name: "Merge requests" });
    const row = within(table).getByRole("link", { name: "Add login page" }).closest("tr")!;
    expect(within(row).getByText("Draft")).toBeInTheDocument();
    const other = within(table).getByRole("link", { name: "Update README" }).closest("tr")!;
    expect(within(other).queryByText("Draft")).not.toBeInTheDocument();
  });

  it("creates a draft with reviewers", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/merge-requests",
      api: treeRoutes({
        [`GET ${T}/merge-requests`]: [],
        [`GET ${T}/compare`]: mr.compareReady,
        [`POST ${T}/merge-requests`]: (req) => json({ ...fx.mergeRequest, ...(req.body as object) }, 201),
        [`GET ${MR}`]: draftMr,
        [`GET ${MR}/comments`]: [],
      }),
    });
    await screen.findByText("No merge requests yet");
    await user.click(screen.getAllByRole("button", { name: "New merge request" })[0]!);
    const dialog = await screen.findByRole("dialog", { name: "New merge request" });
    await user.type(within(dialog).getByLabelText("Reviewers"), "carol, @acme/core");
    await user.click(within(dialog).getByRole("checkbox", { name: "Create as draft" }));
    await user.click(within(dialog).getByRole("button", { name: "Open merge request" }));
    await waitFor(() => expect(fetchMock.find(`POST ${T}/merge-requests`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/merge-requests`)[0]?.body).toEqual({
      title: "Login",
      description: "",
      source_branch: "feature/login",
      target_branch: "main",
      draft: true,
      reviewers: ["carol", "@acme/core"],
    });
  });

  it("maps quota and owner merge refusals", async () => {
    const { user } = renderApp({
      route: DETAIL,
      api: routes(mr.approvedMr, { [`POST ${MR}/merge`]: apiError(409, "quota_exceeded", "tenant storage quota exceeded") }),
    });
    const merge = await screen.findByRole("button", { name: "Merge" });
    await waitFor(() => expect(merge).toBeEnabled());
    await user.click(merge);
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Merge" }));
    const heading = await screen.findByText("Storage quota exceeded");
    expect(heading.closest('[role="alert"]')).toHaveTextContent("tenant storage quota exceeded");
  });

  it("labels draft and owners blockers", async () => {
    const checks: MergeChecks = {
      ...v5.blockedChecks,
      blockers: [
        { kind: "draft", message: "merge request is a draft" },
        { kind: "owners", message: "src/login.tsx needs an owner approval" },
      ],
    };
    renderApp({ route: DETAIL, api: routes(mr.approvedMr, { [`GET ${MR}/checks`]: checks }) });
    const list = await screen.findByRole("list", { name: "Merge blockers" });
    expect(list.querySelector('[data-blocker="draft"]')).toHaveTextContent("Draft merge request is a draft");
    expect(list.querySelector('[data-blocker="owners"]')).toHaveTextContent("Owner approval src/login.tsx needs an owner approval");
  });
});

describe("requested reviewers (v0.7)", () => {
  const reviewers = ["user:carol", "user:erin", "user:dave", "team:core"];
  const withReviewers = {
    ...mr.approvedMr,
    approvals: [
      v5.review("carol", "acc-3", { snapshot_id: "snap-4" }),
      v5.review("erin", "acc-5", { snapshot_id: "snap-4", decision: "changes_requested", body: "No." }),
    ],
    reviewers,
  } as MergeRequest;

  it("lists reviewers with their status, requests and removes reviewers", async () => {
    const { user, fetchMock } = renderApp({
      route: DETAIL,
      api: routes(withReviewers, {
        [`POST ${MR}/reviewers`]: json({ ...withReviewers, reviewers: [...reviewers, "user:frank"] }),
        [`DELETE ${MR}/reviewers`]: json({ ...withReviewers, reviewers: ["user:carol", "user:erin", "team:core"] }),
      }),
    });
    const panel = await screen.findByRole("region", { name: "Requested reviewers" });
    const list = within(panel).getByRole("list", { name: "Reviewers" });
    const status = (r: string) => list.querySelector(`[data-reviewer="${r}"]`)!.getAttribute("data-status");
    await waitFor(() => expect(status("user:carol")).toBe("approved"));
    expect(status("user:erin")).toBe("changes_requested");
    expect(status("user:dave")).toBe("pending");
    expect(status("team:core")).toBe("team");
    expect(list.querySelector('[data-reviewer="team:core"]')).toHaveTextContent("core");

    const form = within(panel).getByRole("form", { name: "Request reviewers" });
    await user.type(within(form).getByLabelText("Request review from"), "frank @acme/qa");
    await user.click(within(form).getByRole("button", { name: "Request" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/reviewers`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/reviewers`)[0]?.body).toEqual({ reviewers: ["frank", "@acme/qa"] });
    await waitFor(() => expect(list.querySelector('[data-reviewer="user:frank"]')).toBeInTheDocument());

    await user.click(within(panel).getByRole("button", { name: "Remove reviewer user:dave" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${MR}/reviewers`)).toHaveLength(1));
    const del = fetchMock.find(`DELETE ${MR}/reviewers`)[0]!;
    expect(del.url.searchParams.getAll("reviewer")).toEqual(["user:dave"]);
    expect(del.body).toEqual({ reviewers: ["user:dave"] });
    await waitFor(() => expect(list.querySelector('[data-reviewer="user:dave"]')).not.toBeInTheDocument());
  });

  it("shows a toast when a request is refused", async () => {
    const { user, toasts } = renderApp({
      route: DETAIL,
      api: routes(withReviewers, { [`POST ${MR}/reviewers`]: apiError(400, "bad_request", "unknown user nobody") }),
    });
    const panel = await screen.findByRole("region", { name: "Requested reviewers" });
    await user.type(within(panel).getByLabelText("Request review from"), "nobody");
    await user.click(within(panel).getByRole("button", { name: "Request" }));
    await waitFor(() => expect(toastTitles(toasts)).toContain("Could not request review"));
  });
});

describe("code owners panel (v0.7)", () => {
  const owners: MROwners = {
    source: "config",
    required: true,
    satisfied: false,
    paths: [
      { path: "src/app.ts", owners: ["user:carol"], approved: true, approved_by: ["carol"] },
      { path: "src/login.tsx", owners: ["team:core", "user:dave"], approved: false, approved_by: [] },
      { path: "README.md", owners: [], approved: false, approved_by: [] },
    ],
    missing: ["src/login.tsx"],
  };

  it("shows owners per changed path and the requirement", async () => {
    renderApp({ route: DETAIL, api: routes(mr.approvedMr, { [`GET ${MR}/owners`]: owners }) });
    const panel = await screen.findByRole("region", { name: "Code owners" });
    const summary = await within(panel).findByText(/Owner approval required/);
    expect(summary).toHaveTextContent("Owner approval required · 1 path missing");
    expect(panel.querySelector("[data-owners-summary]")).toHaveAttribute("data-owners-summary", "required");
    const list = within(panel).getByRole("list", { name: "Owned paths" });
    expect(list.querySelector('[data-path="src/app.ts"]')).toHaveAttribute("data-owner-state", "approved");
    expect(list.querySelector('[data-path="src/app.ts"]')).toHaveTextContent("approved by carol");
    expect(list.querySelector('[data-path="src/login.tsx"]')).toHaveAttribute("data-owner-state", "missing");
    expect(list.querySelector('[data-path="src/login.tsx"]')).toHaveTextContent("core");
    expect(list.querySelector('[data-path="src/login.tsx"]')).toHaveTextContent("@dave");
    expect(list.querySelector('[data-path="README.md"]')).toHaveAttribute("data-owner-state", "unowned");
    expect(within(panel).getByText("server config")).toBeInTheDocument();
  });

  it("reports satisfied owner approval", async () => {
    renderApp({
      route: DETAIL,
      api: routes(mr.approvedMr, {
        [`GET ${MR}/owners`]: { ...owners, satisfied: true, missing: [], paths: [owners.paths[0]!] },
      }),
    });
    const panel = await screen.findByRole("region", { name: "Code owners" });
    expect(await within(panel).findByText("Owner approval satisfied")).toBeInTheDocument();
  });
});

describe("suggested changes (v0.7)", () => {
  it("renders the replaced lines and the replacement, and applies it", async () => {
    const { user, fetchMock, toasts } = renderApp({
      route: DETAIL,
      api: routes(mr.approvedMr, {
        [`GET ${MR}/comments`]: [suggestion],
        [`GET ${T}/blob/snap-4/src/app.ts`]: () => new Response("new\nlogin()\n"),
        [`POST ${MR}/comments/s1/apply`]: {
          comment: { ...suggestion, resolved: true },
          snapshot: { ...mr.featureSnapshot, id: "snap-5" },
        },
      }),
    });
    const block = await screen.findByRole("figure", { name: "Suggested change to src/app.ts line 1" });
    await waitFor(() => expect(block.querySelector('[data-kind="del"]')).toHaveTextContent("new"));
    expect(block.querySelectorAll('[data-kind="del"]')).toHaveLength(1);
    expect(block.querySelector('[data-kind="add"]')).toHaveTextContent("newer");

    await user.click(within(block).getByRole("button", { name: "Apply suggestion" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/comments/s1/apply`)).toHaveLength(1));
    await waitFor(() => expect(toastTitles(toasts)).toContain("Suggestion applied"));
    // MR, comments, checks and the diff are refetched.
    await waitFor(() => expect(fetchMock.find(`GET ${MR}/comments`).length).toBeGreaterThan(1));
    expect(fetchMock.find(`GET ${MR}`).length).toBeGreaterThan(1);
    expect(fetchMock.find(`GET ${T}/compare`).length).toBeGreaterThan(1);
  });

  it("falls back to the diff for the replaced lines and explains outdated suggestions", async () => {
    const { user, toasts } = renderApp({
      route: DETAIL,
      api: routes(mr.approvedMr, {
        [`GET ${MR}/comments`]: [suggestion],
        [`POST ${MR}/comments/s1/apply`]: apiError(409, "suggestion_outdated", "the source changed lines 1-1"),
      }),
    });
    const block = await screen.findByRole("figure", { name: "Suggested change to src/app.ts line 1" });
    // No blob: line 1 of the source tip is known from the diff.
    await waitFor(() => expect(block.querySelector('[data-kind="del"]')).toHaveTextContent("new"));
    await user.click(within(block).getByRole("button", { name: "Apply suggestion" }));
    await waitFor(() => expect(toastTitles(toasts)).toContain("Suggestion is outdated"));
  });

  it("disables applied suggestions", async () => {
    renderApp({
      route: DETAIL,
      api: routes(mr.approvedMr, {
        [`GET ${MR}/comments`]: [
          { ...suggestion, suggestion: { ...suggestion.suggestion!, applied_snapshot: "abcdef1234567890", applied_by: "bob" } },
        ],
      }),
    });
    const block = await screen.findByRole("figure", { name: "Suggested change to src/app.ts line 1" });
    expect(within(block).getByRole("button", { name: "Applied as abcdef12" })).toBeDisabled();
    expect(block).toHaveTextContent("applied by bob");
  });
});

describe("inline line comments (v0.7)", () => {
  const lineComment = comment("c9", { body: "Why login() here?", path: "src/app.ts", line: 2, snapshot_id: "snap-4" });

  it("shows threads under their line and posts a comment anchored to path and line", async () => {
    const { user, fetchMock } = renderApp({
      route: `${DETAIL}?tab=changes`,
      api: routes(mr.approvedMr, {
        [`GET ${MR}/comments`]: [lineComment] satisfies ReviewComment[],
        [`POST ${MR}/comments`]: (req) => json({ ...lineComment, id: "c10", ...(req.body as object) }, 201),
      }),
    });
    const thread = await screen.findByRole("group", { name: "Discussion on src/app.ts line 2" });
    expect(within(thread).getByText("Why login() here?")).toBeInTheDocument();
    expect(within(thread).getByRole("button", { name: "Resolve" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Comment on src/app.ts line 1" }));
    const form = await screen.findByRole("form", { name: "Comment on src/app.ts line 1" });
    await user.type(within(form).getByLabelText("Line comment"), "Rename");
    await user.click(within(form).getByRole("button", { name: "Add comment" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/comments`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/comments`)[0]?.body).toEqual({ body: "Rename", path: "src/app.ts", line: 1, snapshot_id: "snap-4" });
    await waitFor(() => expect(screen.queryByRole("form", { name: "Comment on src/app.ts line 1" })).not.toBeInTheDocument());
  });

  it("suggests a change over a range, prefilled with the current lines", async () => {
    const { user, fetchMock } = renderApp({
      route: `${DETAIL}?tab=changes`,
      api: routes(mr.approvedMr, {
        [`POST ${MR}/comments`]: (req) => json({ ...suggestion, id: "s2", ...(req.body as object) }, 201),
      }),
    });
    await user.click(await screen.findByRole("button", { name: "Comment on src/app.ts line 1" }));
    const form = await screen.findByRole("form", { name: "Comment on src/app.ts line 1" });
    await user.click(within(form).getByRole("radio", { name: "Suggest change" }));
    const replacement = within(form).getByLabelText("Replacement");
    expect(replacement).toHaveValue("new");
    const end = within(form).getByLabelText("End line");
    await user.clear(end);
    await user.type(end, "2");
    expect(replacement).toHaveValue("new\nlogin()");
    await user.clear(replacement);
    await user.type(replacement, "renewed{Enter}login(user)");
    await user.click(within(form).getByRole("button", { name: "Add suggestion" }));
    await waitFor(() => expect(fetchMock.find(`POST ${MR}/comments`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${MR}/comments`)[0]?.body).toEqual({
      body: "",
      path: "src/app.ts",
      line: 1,
      snapshot_id: "snap-4",
      suggestion: { path: "src/app.ts", start_line: 1, end_line: 2, replacement: "renewed\nlogin(user)", snapshot_id: "snap-4" },
    });
  });

  it("rejects an end line before the start line", async () => {
    const { user, fetchMock } = renderApp({ route: `${DETAIL}?tab=changes`, api: routes(mr.approvedMr) });
    await user.click(await screen.findByRole("button", { name: "Comment on src/app.ts line 2" }));
    const form = await screen.findByRole("form", { name: "Comment on src/app.ts line 2" });
    await user.click(within(form).getByRole("radio", { name: "Suggest change" }));
    const end = within(form).getByLabelText("End line");
    await user.clear(end);
    await user.type(end, "1");
    expect(within(form).getByText("An end line from 2.")).toBeInTheDocument();
    await user.click(within(form).getByRole("button", { name: "Add suggestion" }));
    expect(fetchMock.find(`POST ${MR}/comments`)).toHaveLength(0);
  });
});

describe("owners tab (v0.7)", () => {
  const fileView: OwnersView = {
    tree_id: "tree-web",
    rules: [{ pattern: "docs/**", owners: ["user:alice"] }],
    source: "file",
    errors: ["OWNERS:3: no owners"],
  };

  it("shows the source and parse errors, edits rules and saves them", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/owners",
      api: treeRoutes({
        [`GET ${T}/owners`]: fileView,
        [`PUT ${T}/owners`]: (req) =>
          json({ ...fileView, ...(req.body as object), source: "config", errors: [], updated_by: "alice", updated_at: new Date().toISOString() }),
      }),
    });
    const table = await screen.findByRole("table", { name: "Owner rules" });
    expect(screen.getByText("OWNERS file").closest("[data-source]")).toHaveAttribute("data-source", "file");
    expect(within(screen.getByRole("list", { name: "OWNERS file errors" })).getByText("OWNERS:3: no owners")).toBeInTheDocument();
    expect(within(table).getByLabelText("Pattern of rule 1")).toHaveValue("docs/**");
    expect(within(table).getByLabelText("Owners of rule 1")).toHaveValue("user:alice");
    expect(screen.getAllByText(/last matching rule wins/i).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Add rule" }));
    await user.type(within(table).getByLabelText("Pattern of rule 2"), "**/*.go");
    await user.type(within(table).getByLabelText("Owners of rule 2"), "@bob, team:core");
    await user.click(within(table).getByRole("button", { name: "Move rule 2 up" }));
    expect(within(table).getByLabelText("Pattern of rule 1")).toHaveValue("**/*.go");

    // A blank pattern with owners is rejected.
    await user.click(screen.getByRole("button", { name: "Add rule" }));
    await user.type(within(table).getByLabelText("Owners of rule 3"), "carol");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(within(table).getByText("Pattern required.")).toBeInTheDocument();
    expect(fetchMock.find(`PUT ${T}/owners`)).toHaveLength(0);
    await user.click(within(table).getByRole("button", { name: "Delete rule 3" }));

    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/owners`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/owners`)[0]?.body).toEqual({
      rules: [
        { pattern: "**/*.go", owners: ["@bob", "team:core"] },
        { pattern: "docs/**", owners: ["user:alice"] },
      ],
    });
    expect(await screen.findByText("Server config")).toBeInTheDocument();
  });

  it("removes the server configuration after confirmation", async () => {
    const configView: OwnersView = { ...fileView, source: "config", errors: [], updated_by: "alice", updated_at: new Date().toISOString() };
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/owners",
      api: treeRoutes({
        [`GET ${T}/owners`]: configView,
        [`PUT ${T}/owners`]: json({ ...fileView, rules: [], source: "none", errors: [] }),
      }),
    });
    await screen.findByRole("table", { name: "Owner rules" });
    expect(screen.getByText("Server config")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove server config" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Remove configuration" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/owners`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/owners`)[0]?.body).toEqual({ rules: [] });
    expect(await screen.findByText("No owner rules")).toBeInTheDocument();
  });

  it("degrades when owners are not available", async () => {
    renderApp({
      route: "/trees/acme/web/owners",
      api: treeRoutes({ [`GET ${T}/owners`]: apiError(501, "not_implemented", "owners are not configured") }),
    });
    expect(await screen.findByText("Path owners are not enabled on this W0rkTree server.")).toBeInTheDocument();
  });
});

describe("protections: owner approval (v0.7)", () => {
  it("saves and lists require_owner_approval", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({
        [`GET ${T}/protections`]: [{ ...fx.protection, require_owner_approval: true }],
        [`PUT ${T}/protections`]: (req) => json(req.body),
        [`GET ${T}/tag-protections`]: [],
      }),
    });
    const table = await screen.findByRole("table", { name: "Branch protections" });
    expect(within(table).getByText("owner approval")).toBeInTheDocument();

    await user.click(within(table).getByRole("button", { name: "Edit main" }));
    const form = await screen.findByRole("form", { name: "Edit protection" });
    expect(within(form).getByRole("switch", { name: "Require owner approval" })).toBeChecked();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Add protection" }));
    const add = await screen.findByRole("form", { name: "Add protection" });
    await user.type(within(add).getByLabelText("Branch pattern"), "release/*");
    await user.click(within(add).getByRole("switch", { name: "Require owner approval" }));
    await user.click(screen.getByRole("button", { name: "Add protection" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/protections`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/protections`)[0]?.body).toMatchObject({ pattern: "release/*", require_owner_approval: true });
  });
});

describe("linked group merge method (v0.7)", () => {
  const linked = { ...mr.approvedMr, linked_group: "grp-1" } as MergeRequest;
  const groupRoutes = (extra: Record<string, MockRoute> = {}) =>
    routes(linked, {
      "GET /api/v1/tenants/acme/linked-groups/grp-1": v5.linkedGroup,
      "GET /api/v1/tenants/acme/trees": [fx.summary],
      "POST /api/v1/tenants/acme/linked-groups/grp-1/merge": { ...v5.linkedGroup, state: "merged" },
      ...extra,
    });

  it("sends the chosen method", async () => {
    const { user, fetchMock } = renderApp({ route: DETAIL, api: groupRoutes() });
    const card = await screen.findByRole("region", { name: "Linked group" });
    await user.click(await within(card).findByRole("button", { name: "Merge group" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByRole("combobox", { name: "Merge method" })).toHaveTextContent("Default");
    await user.click(within(dialog).getByRole("combobox", { name: "Merge method" }));
    await user.click(await screen.findByRole("option", { name: "Squash" }));
    await user.click(within(dialog).getByRole("button", { name: "Merge all" }));
    await waitFor(() => expect(fetchMock.find("POST /api/v1/tenants/acme/linked-groups/grp-1/merge")).toHaveLength(1));
    expect(fetchMock.find("POST /api/v1/tenants/acme/linked-groups/grp-1/merge")[0]?.body).toEqual({ method: "squash" });
  });

  it("omits the method by default", async () => {
    const { user, fetchMock } = renderApp({ route: DETAIL, api: groupRoutes() });
    const card = await screen.findByRole("region", { name: "Linked group" });
    await user.click(await within(card).findByRole("button", { name: "Merge group" }));
    await user.click(within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Merge all" }));
    await waitFor(() => expect(fetchMock.find("POST /api/v1/tenants/acme/linked-groups/grp-1/merge")).toHaveLength(1));
    expect(fetchMock.find("POST /api/v1/tenants/acme/linked-groups/grp-1/merge")[0]?.body).toBeUndefined();
  });
});
