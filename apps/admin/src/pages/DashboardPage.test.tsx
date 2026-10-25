import { act, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FakeEventSource } from "@/test/fakeEventSource";
import * as fx from "@/test/fixtures";
import * as dx from "@/test/fixtures.dashboard";
import { renderApp } from "@/test/renderApp";

const pushes = [dx.auditEvents[0]!];

const adminApi = {
  "GET /api/v1/admin/stats": fx.stats,
  "GET /api/v1/audit": pushes,
  "GET /api/v1/events/recent": dx.recentEvents,
  "GET /metrics": () => new Response(dx.metricsText(), { headers: { "Content-Type": "text/plain" } }),
};

const memberMe = { ...fx.me, account: { ...fx.account, is_admin: false }, memberships: [{ tenant: fx.tenant, role: "developer" as const }] };

const staged = (actor: string, branch: string) => ({
  type: "snapshot.staged",
  tenant_id: fx.tenant.id,
  actor,
  payload: { branch_name: branch },
  at: new Date().toISOString(),
});

describe("DashboardPage", () => {
  it("shows server KPIs, recent pushes, quick actions and HTTP traffic to admins", async () => {
    const { fetchMock } = renderApp({ api: adminApi });

    const stats = await screen.findByRole("list", { name: "Server statistics" });
    expect(await within(stats).findByText("1,234")).toBeInTheDocument();
    expect(within(stats).getByRole("listitem", { name: "Storage" })).toHaveTextContent("5.0 GiB");
    expect(within(stats).getByRole("listitem", { name: "Accounts" })).toHaveTextContent("12");

    const recent = await screen.findByRole("list", { name: "Recent pushes" });
    expect(within(recent).getByText("bob")).toBeInTheDocument();
    expect(within(recent).getByRole("link", { name: "acme/web" })).toHaveAttribute("href", "/trees/acme/web");
    expect(within(recent).getByText(/2 snapshots/)).toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/audit")[0]?.url.searchParams.get("action")).toBe("branch.push");

    const actions = screen.getByRole("navigation", { name: "Quick actions" });
    expect(within(actions).getByRole("link", { name: "New tree" })).toHaveAttribute("href", "/trees?new=1");
    expect(within(actions).getByRole("link", { name: "New tenant" })).toHaveAttribute("href", "/tenants?new=1");

    expect(await screen.findByText("2.0.0-test")).toBeInTheDocument();
    expect(screen.getByText("HTTP traffic")).toBeInTheDocument();
    expect(await screen.findAllByText(/Collecting samples/)).toHaveLength(2);
    expect(fetchMock.find("GET /metrics")).not.toHaveLength(0);
  });

  it("shows tenants and trees with quick links to members", async () => {
    const { fetchMock } = renderApp({
      api: {
        "GET /api/v1/auth/me": memberMe,
        "GET /api/v1/audit": [],
        "GET /api/v1/events/recent": [],
        "GET /api/v1/tenants/acme/trees": [fx.summary],
      },
    });

    const tenants = await screen.findByRole("list", { name: "Your tenants" });
    expect(within(tenants).getByRole("link", { name: /Acme Corp/ })).toHaveAttribute("href", "/tenants/acme");
    expect(within(tenants).getByText("developer")).toBeInTheDocument();

    const trees = await screen.findByRole("list", { name: "Your trees" });
    expect(within(trees).getByRole("link", { name: "acme/web" })).toHaveAttribute("href", "/trees/acme/web");
    const quick = within(trees).getByRole("navigation", { name: "Quick links for acme/web" });
    expect(within(quick).getByRole("link", { name: "Merge requests (1)" })).toHaveAttribute("href", "/trees/acme/web/merge-requests");
    expect(within(quick).getByRole("link", { name: "Files" })).toHaveAttribute("href", "/trees/acme/web/files");

    expect(screen.getByText(/Server-wide statistics are visible to server administrators/)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Server statistics" })).not.toBeInTheDocument();
    expect(screen.queryByText("HTTP traffic")).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "System" })).not.toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/admin/stats")).toHaveLength(0);
    expect(fetchMock.find("GET /metrics")).toHaveLength(0);
  });

  it("seeds the feed from recent events and appends live SSE events", async () => {
    renderApp({ api: adminApi });

    const feed = await screen.findByRole("list", { name: "Live activity" });
    expect(within(feed).getAllByRole("listitem")).toHaveLength(2);
    expect(within(feed).getAllByRole("listitem")[0]).toHaveTextContent(/bob pushed — web · main/);
    expect(screen.getByText("2 events in the last 30 minutes")).toBeInTheDocument();

    const es = FakeEventSource.latest();
    expect(new URL(es.url, "http://x").searchParams.get("access_token")).toBe("test-token");
    act(() => {
      es.open();
      es.emit("snapshot.staged", staged("carol", "feature/search"));
      // A live copy of a seeded event is not shown twice.
      es.emit("branch.pushed", dx.recentEvents[1]);
    });

    const items = within(feed).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0]).toHaveTextContent("carol");
    expect(items[0]).toHaveTextContent(/staged work in progress — feature\/search/);
    expect(screen.getByText("Live")).toBeInTheDocument();
    expect(screen.getByText("3 events in the last 30 minutes")).toBeInTheDocument();
  });

  it("filters the feed by event type", async () => {
    const { user } = renderApp({ api: adminApi });
    const feed = await screen.findByRole("list", { name: "Live activity" });
    expect(within(feed).getAllByRole("listitem")).toHaveLength(2);

    await user.click(screen.getByRole("combobox", { name: "Event type" }));
    await user.click(await screen.findByRole("option", { name: "tree.created" }));

    const items = within(screen.getByRole("list", { name: "Live activity" })).getAllByRole("listitem");
    expect(items).toHaveLength(1);
    expect(items[0]).toHaveTextContent(/alice created a tree — docs/);

    await user.click(screen.getByRole("combobox", { name: "Event type" }));
    await user.click(await screen.findByRole("option", { name: "mr.merged" }));
    expect(screen.getByText("No matching events")).toBeInTheDocument();
  });

  it("pauses the feed and catches up on resume", async () => {
    const { user } = renderApp({ api: { ...adminApi, "GET /api/v1/events/recent": [] } });
    expect(await screen.findByText("Waiting for activity")).toBeInTheDocument();
    const es = FakeEventSource.latest();

    act(() => es.emit("snapshot.staged", staged("carol", "one")));
    expect(within(screen.getByRole("list", { name: "Live activity" })).getAllByRole("listitem")).toHaveLength(1);

    const pause = screen.getByRole("button", { name: "Pause" });
    await user.click(pause);
    expect(screen.getByRole("button", { name: "Resume" })).toHaveAttribute("aria-pressed", "true");

    act(() => {
      es.emit("snapshot.staged", staged("dave", "two"));
      es.emit("snapshot.staged", staged("erin", "three"));
    });
    expect(within(screen.getByRole("list", { name: "Live activity" })).getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByText("2 new events while paused")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Resume" }));
    const items = within(screen.getByRole("list", { name: "Live activity" })).getAllByRole("listitem");
    expect(items.map((li) => li.textContent)).toEqual([
      expect.stringContaining("erin"),
      expect.stringContaining("dave"),
      expect.stringContaining("carol"),
    ]);
    expect(screen.queryByText(/while paused/)).not.toBeInTheDocument();
  });
});
