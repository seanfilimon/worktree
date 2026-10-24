import { act, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FakeEventSource } from "@/test/fakeEventSource";
import * as fx from "@/test/fixtures";
import { renderApp } from "@/test/renderApp";

const pushes = [
  {
    id: "a1",
    tenant_id: fx.tenant.id,
    actor_id: "acc-2",
    actor: "bob",
    action: "branch.push",
    resource_type: "branch",
    resource_id: "main",
    details: { branch: "main", tree: "acme/web" },
    at: new Date().toISOString(),
  },
];

describe("DashboardPage", () => {
  it("renders server statistics and recent pushes", async () => {
    const { fetchMock } = renderApp({ api: { "GET /api/v1/admin/stats": fx.stats, "GET /api/v1/audit": pushes } });

    const stats = await screen.findByRole("list", { name: "Server statistics" });
    expect(await within(stats).findByText("1,234")).toBeInTheDocument();
    expect(within(stats).getByText("5.0 GiB")).toBeInTheDocument();
    expect(within(stats).getByText("Accounts").nextSibling).toHaveTextContent("12");

    const recent = await screen.findByRole("list", { name: "Recent pushes" });
    expect(within(recent).getByText("bob")).toBeInTheDocument();
    expect(within(recent).getByText("to acme/web", { exact: false })).toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/audit")[0]?.url.searchParams.get("action")).toBe("branch.push");
    expect(await screen.findByText("2.0.0-test")).toBeInTheDocument();
  });

  it("streams live activity over SSE", async () => {
    renderApp({ api: { "GET /api/v1/admin/stats": fx.stats, "GET /api/v1/audit": [] } });
    expect(await screen.findByText("Waiting for activity")).toBeInTheDocument();

    const es = FakeEventSource.latest();
    expect(new URL(es.url, "http://x").searchParams.get("access_token")).toBe("test-token");
    act(() => {
      es.open();
      es.emit("snapshot.staged", {
        type: "snapshot.staged",
        tenant_id: fx.tenant.id,
        actor: "carol",
        payload: { branch_name: "feature/search" },
        at: new Date().toISOString(),
      });
    });

    const feed = await screen.findByRole("list", { name: "Live activity" });
    expect(within(feed).getByText("carol")).toBeInTheDocument();
    expect(within(feed).getByText(/staged work in progress/)).toBeInTheDocument();
    expect(within(feed).getByText(/feature\/search/)).toBeInTheDocument();
    expect(screen.getByText("Live")).toBeInTheDocument();
  });

  it("hides server stats from non-admins", async () => {
    renderApp({
      api: {
        "GET /api/v1/auth/me": { ...fx.me, account: { ...fx.account, is_admin: false } },
        "GET /api/v1/audit": [],
      },
    });
    expect(await screen.findByText(/Server-wide statistics are visible to server administrators/)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Server statistics" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Accounts" })).not.toBeInTheDocument();
  });
});
