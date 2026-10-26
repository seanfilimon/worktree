import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { BackupStatus } from "@/api/backups";
import {
  canRetryEmail,
  outboxEmailTime,
  retentionReportText,
  retentionRuleText,
  type OutboxEmail,
  type OutboxList,
  type RetentionReport,
  type RetentionStatus,
} from "@/api/serverops";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

// server-ops (docs/ENGINEERING_PLAN.md §16.11): email outbox and retention.

const backups: BackupStatus = {
  config: {
    dir: "/data/backups",
    target: "disk",
    location: "/data/backups",
    interval: "0s",
    interval_seconds: 0,
    keep: 0,
    max_age: "0s",
    max_age_seconds: 0,
  },
  store: { kind: "disk", root: "/data", compress: false },
  supported: true,
  running: false,
  backups: [],
};

const pending: OutboxEmail = {
  id: "em-1",
  to: "ada@example.com",
  subject: "[acme/web] Merge request !4 approved",
  notification_id: "nt-1",
  status: "pending",
  attempts: 2,
  last_error: "dial tcp 10.0.0.5:587: connection refused",
  created_at: "2026-10-26T09:00:00Z",
  updated_at: "2026-10-26T09:05:00Z",
  next_attempt_at: "2026-10-26T09:10:00Z",
  deadline_at: "2026-10-27T09:00:00Z",
};
const dead: OutboxEmail = {
  id: "em-2",
  to: "bob@example.com",
  subject: "Password reset",
  status: "dead",
  attempts: 14,
  last_error: "550 mailbox unavailable",
  created_at: "2026-10-24T08:00:00Z",
  updated_at: "2026-10-25T08:00:00Z",
  deadline_at: "2026-10-25T08:00:00Z",
  dead_at: "2026-10-25T08:00:00Z",
};
const sent: OutboxEmail = {
  id: "em-3",
  to: "cy@example.com",
  subject: "Welcome to W0rkTree",
  status: "sent",
  attempts: 1,
  created_at: "2026-10-23T08:00:00Z",
  updated_at: "2026-10-23T08:00:01Z",
  deadline_at: "2026-10-24T08:00:00Z",
  sent_at: "2026-10-23T08:00:01Z",
};

const outbox: OutboxList = {
  enabled: true,
  dead_after: "24h0m0s",
  dead_after_seconds: 86400,
  counts: { pending: 1, sent: 120, dead: 1 },
  emails: [pending, dead, sent],
};

const report: RetentionReport = {
  at: "2026-10-26T08:00:00Z",
  policy_versions_pruned: 3,
  audit_pruned: 10,
  webhook_deliveries_pruned: 0,
  backups_pruned: 1,
  took_ms: 12,
};

const retention: RetentionStatus = {
  interval: "1h0m0s",
  running: false,
  config: {
    policy_versions: { max_age: "2160h0m0s", max_age_seconds: 7776000, keep: 100 },
    audit: { max_age: "0s", max_age_seconds: 0 },
    webhook_deliveries: { max_age: "720h0m0s", max_age_seconds: 2592000, keep: 50 },
    backups: { max_age: "0s", max_age_seconds: 0, keep: 7 },
  },
  last_run_at: report.at,
  last_report: report,
};

const routes = {
  "GET /api/v1/admin/backups": backups,
  "GET /api/v1/admin/outbox": outbox,
  "GET /api/v1/admin/retention": retention,
};

describe("Email outbox", () => {
  it("lists queued emails with counts and dead-lettering", async () => {
    renderApp({ route: "/backups", api: routes });
    const table = await screen.findByRole("table", { name: "Email outbox" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(4);
    expect(rows[1]).toHaveTextContent("ada@example.com");
    expect(rows[1]).toHaveTextContent("pending");
    expect(rows[1]).toHaveTextContent("Next attempt");
    expect(rows[1]).toHaveTextContent("connection refused");
    expect(rows[2]).toHaveTextContent("Dead-lettered");
    expect(rows[2]).toHaveTextContent("14");
    expect(rows[3]).toHaveTextContent("Sent");

    // Retry only where it makes sense; delete everywhere.
    expect(within(rows[1]!).getByRole("button", { name: "Retry em-1" })).toBeInTheDocument();
    expect(within(rows[2]!).getByRole("button", { name: "Retry em-2" })).toBeInTheDocument();
    expect(within(rows[3]!).queryByRole("button", { name: "Retry em-3" })).not.toBeInTheDocument();
    expect(within(rows[3]!).getByRole("button", { name: "Delete em-3" })).toBeInTheDocument();

    const counts = screen.getByLabelText("Outbox counts");
    expect(counts).toHaveTextContent("Pending1");
    expect(counts).toHaveTextContent("Sent120");
    expect(counts).toHaveTextContent("Dead1");
    expect(screen.getByText(/Emails that still fail after 24h0m0s are dead-lettered/)).toBeInTheDocument();
    expect(screen.queryByText(/Email is not configured/)).not.toBeInTheDocument();
  });

  it("filters by status through the query string", async () => {
    const { user, fetchMock } = renderApp({
      route: "/backups",
      api: {
        ...routes,
        "GET /api/v1/admin/outbox": (req) =>
          req.url.searchParams.get("status") === "dead" ? json({ ...outbox, emails: [dead] }) : json(outbox),
      },
    });
    await screen.findByRole("table", { name: "Email outbox" });
    expect(fetchMock.find("GET /api/v1/admin/outbox")[0]!.url.searchParams.has("status")).toBe(false);

    await user.click(screen.getByRole("tab", { name: "Dead" }));
    await waitFor(() => expect(within(screen.getByRole("table", { name: "Email outbox" })).getAllByRole("row")).toHaveLength(2));
    const calls = fetchMock.find("GET /api/v1/admin/outbox");
    expect(calls.at(-1)!.url.searchParams.get("status")).toBe("dead");
    expect(screen.getByRole("table", { name: "Email outbox" })).toHaveTextContent("bob@example.com");
  });

  it("retries one email", async () => {
    const { user, fetchMock } = renderApp({
      route: "/backups",
      api: { ...routes, "POST /api/v1/admin/outbox/em-2/retry": { ...dead, status: "pending", attempts: 0 } },
    });
    await user.click(await screen.findByRole("button", { name: "Retry em-2" }));
    expect(await screen.findByText("Email requeued")).toBeInTheDocument();
    const req = fetchMock.find("POST /api/v1/admin/outbox/em-2/retry");
    expect(req).toHaveLength(1);
    expect(req[0]!.body).toBeUndefined();
    await waitFor(() => expect(fetchMock.find("GET /api/v1/admin/outbox").length).toBeGreaterThan(1));
  });

  it("explains an email that was already sent", async () => {
    const { user } = renderApp({
      route: "/backups",
      api: { ...routes, "POST /api/v1/admin/outbox/em-1/retry": apiError(409, "already_sent", "email em-1 was already sent") },
    });
    await user.click(await screen.findByRole("button", { name: "Retry em-1" }));
    expect(await screen.findByText("Email already sent")).toBeInTheDocument();
  });

  it("retries every dead email", async () => {
    const { user, fetchMock } = renderApp({
      route: "/backups",
      api: { ...routes, "POST /api/v1/admin/outbox/retry": { retried: 1 } },
    });
    await user.click(await screen.findByRole("button", { name: "Retry all dead" }));
    expect(await screen.findByText("Dead emails requeued")).toBeInTheDocument();
    expect(screen.getByText("1 email")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/admin/outbox/retry")).toHaveLength(1);
  });

  it("disables retry-all without dead emails", async () => {
    renderApp({
      route: "/backups",
      api: { ...routes, "GET /api/v1/admin/outbox": { ...outbox, counts: { pending: 0, sent: 3, dead: 0 }, emails: [sent] } },
    });
    await screen.findByRole("table", { name: "Email outbox" });
    expect(screen.getByRole("button", { name: "Retry all dead" })).toBeDisabled();
  });

  it("deletes an email only after confirmation", async () => {
    let list = outbox;
    const { user, fetchMock } = renderApp({
      route: "/backups",
      api: {
        ...routes,
        "GET /api/v1/admin/outbox": () => json(list),
        "DELETE /api/v1/admin/outbox/em-2": () => {
          list = { ...outbox, counts: { ...outbox.counts, dead: 0 }, emails: [pending, sent] };
          return noContent();
        },
      },
    });
    await user.click(await screen.findByRole("button", { name: "Delete em-2" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Delete this email?" });
    expect(confirm).toHaveTextContent("bob@example.com");
    expect(fetchMock.find("DELETE /api/v1/admin/outbox/em-2")).toHaveLength(0);
    await user.click(within(confirm).getByRole("button", { name: "Delete email" }));
    expect(await screen.findByText("Email deleted")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("table", { name: "Email outbox" })).not.toHaveTextContent("bob@example.com"));
  });

  it("explains that SMTP is not configured", async () => {
    renderApp({
      route: "/backups",
      api: { ...routes, "GET /api/v1/admin/outbox": { ...outbox, enabled: false, emails: null } },
    });
    expect(await screen.findByText(/Email is not configured/)).toBeInTheDocument();
    expect(screen.getByText("No emails")).toBeInTheDocument();
  });

  it("explains servers without the outbox or retention", async () => {
    renderApp({
      route: "/backups",
      api: {
        ...routes,
        "GET /api/v1/admin/outbox": apiError(501, "not_implemented", "outbox is not available"),
        "GET /api/v1/admin/retention": apiError(501, "not_implemented", "retention is not available"),
      },
    });
    expect(await screen.findByText("Email outbox is not available on this server")).toBeInTheDocument();
    expect(await screen.findByText("Retention is not available on this server")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry all dead" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Run now" })).not.toBeInTheDocument();
    // The backups part of the page is unaffected.
    expect(screen.getByLabelText("Backup configuration")).toBeInTheDocument();
  });
});

describe("Retention", () => {
  it("shows the policy and the last run", async () => {
    renderApp({ route: "/backups", api: routes });
    const policy = await screen.findByLabelText("Retention policy");
    expect(policy).toHaveTextContent("Policy versionsOlder than 2160h0m0s or beyond 100 per policy");
    expect(policy).toHaveTextContent("Audit eventsKept forever");
    expect(policy).toHaveTextContent("Webhook deliveriesOlder than 720h0m0s or beyond 50 per webhook");
    expect(policy).toHaveTextContent("Scheduled backupsBeyond 7 backups");
    const runs = screen.getByLabelText("Retention runs");
    expect(runs).toHaveTextContent("Every 1h0m0s");
    expect(runs).toHaveTextContent("3 policy versions, 10 audit events, 0 webhook deliveries, 1 backup");
    expect(runs).toHaveTextContent("took 12 ms");
    expect(screen.getByText(/--policy-version-retention/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reports a failed pass", async () => {
    renderApp({
      route: "/backups",
      api: { ...routes, "GET /api/v1/admin/retention": { ...retention, last_error: "bolt: database not open" } },
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("bolt: database not open");
  });

  it("runs a pass now and refreshes retention and backups", async () => {
    const fresh: RetentionReport = {
      ...report,
      at: "2026-10-26T09:30:00Z",
      audit_pruned: 2,
      policy_versions_pruned: 0,
      backups_pruned: 0,
      took_ms: 4,
    };
    const { user, fetchMock } = renderApp({
      route: "/backups",
      api: { ...routes, "POST /api/v1/admin/retention/run": fresh },
    });
    await screen.findByLabelText("Retention policy");
    const before = {
      retention: fetchMock.find("GET /api/v1/admin/retention").length,
      backups: fetchMock.find("GET /api/v1/admin/backups").length,
    };
    await user.click(screen.getByRole("button", { name: "Run now" }));
    expect(await screen.findByText("Retention pass finished")).toBeInTheDocument();
    expect(screen.getByText(/Pruned 0 policy versions, 2 audit events, 0 webhook deliveries, 0 backups in 4 ms/)).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/admin/retention/run")).toHaveLength(1);
    await waitFor(() => {
      expect(fetchMock.find("GET /api/v1/admin/retention").length).toBeGreaterThan(before.retention);
      expect(fetchMock.find("GET /api/v1/admin/backups").length).toBeGreaterThan(before.backups);
    });
  });
});

describe("server-ops helpers", () => {
  it("describes retention rules and reports", () => {
    expect(retentionRuleText({ max_age: "0s", max_age_seconds: 0, keep: 0 })).toBe("Kept forever");
    expect(retentionRuleText({ max_age: "24h0m0s", max_age_seconds: 86400 })).toBe("Older than 24h0m0s");
    expect(retentionRuleText({ max_age: "0s", max_age_seconds: 0, keep: 5 }, "per policy")).toBe("Beyond 5 per policy");
    expect(retentionReportText({ ...report, audit_pruned: 1 })).toBe("3 policy versions, 1 audit event, 0 webhook deliveries, 1 backup");
  });

  it("picks the relevant time and retry eligibility", () => {
    expect(outboxEmailTime(pending)).toEqual({ label: "Next attempt", at: pending.next_attempt_at });
    expect(outboxEmailTime(sent)).toEqual({ label: "Sent", at: sent.sent_at });
    expect(outboxEmailTime(dead)).toEqual({ label: "Dead-lettered", at: dead.dead_at });
    expect([pending, dead, sent].map(canRetryEmail)).toEqual([true, true, false]);
  });
});
