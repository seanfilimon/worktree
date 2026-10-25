import { act, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { keys } from "@/api/queries";
import * as fx from "@/test/fixtures";
import * as dx from "@/test/fixtures.dashboard";
import { renderApp } from "@/test/renderApp";

const api = {
  "GET /api/v1/admin/system": dx.systemInfo,
  "GET /metrics": () => new Response(dx.metricsText(), { headers: { "Content-Type": "text/plain" } }),
};

describe("SystemPage", () => {
  it("shows build, uptime, runtime, storage, event bus and configuration", async () => {
    renderApp({ route: "/system", api });

    const runtime = await screen.findByLabelText("Runtime");
    expect(within(runtime).getByText("42")).toBeInTheDocument();
    expect(within(runtime).getByText("256.0 MiB")).toBeInTheDocument();
    expect(within(runtime).getByRole("progressbar", { name: "Heap allocated: 25% of system memory" })).toBeInTheDocument();
    expect(within(runtime).getByRole("progressbar", { name: "Heap in use: 31% of system memory" })).toBeInTheDocument();

    expect(within(screen.getByLabelText("Build")).getByText("linux/amd64")).toBeInTheDocument();
    expect(within(screen.getByLabelText("Uptime")).getByText(/^1h 30m \d+s$/)).toBeInTheDocument();
    const storage = screen.getByLabelText("Storage");
    expect(within(storage).getByText("98,765")).toBeInTheDocument();
    expect(within(storage).getByText("32.0 MiB")).toBeInTheDocument();
    const bus = screen.getByLabelText("Event bus");
    expect(within(bus).getByText("1,200")).toBeInTheDocument();
    expect(within(bus).getByText("3")).toBeInTheDocument();
    const config = screen.getByLabelText("Configuration");
    expect(within(config).getByText("Open")).toBeInTheDocument();
    expect(within(config).getByText("/var/lib/worktree")).toBeInTheDocument();
  });

  it("charts goroutines and heap from successive samples", async () => {
    const { fetchMock, queryClient } = renderApp({ route: "/system", api });
    await screen.findByLabelText("Runtime");
    expect(screen.getAllByText(/a new point is added every 10 seconds/)).toHaveLength(2);

    fetchMock.on("GET /api/v1/admin/system", { ...dx.systemInfo, num_goroutine: 50 });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 5));
      await queryClient.refetchQueries({ queryKey: keys.system });
    });

    expect(await screen.findByRole("img", { name: "Goroutines over time" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Heap memory over time" })).toBeInTheDocument();
    expect(screen.getByText("Last 2 of up to 60 samples")).toBeInTheDocument();
    expect(within(screen.getByLabelText("Runtime")).getByText("50")).toBeInTheDocument();
  });

  it("filters the raw metrics output", async () => {
    const { user } = renderApp({ route: "/system", api });
    await user.click(await screen.findByRole("tab", { name: "Raw metrics" }));

    const output = await screen.findByRole("region", { name: "Metrics output" });
    expect(within(output).getByText("go_goroutines 42")).toBeInTheDocument();
    expect(within(output).getByText(/worktree_http_requests_total\{route="GET \/api\/v1\/tenants",code="200"\} 98/)).toBeInTheDocument();

    await user.type(screen.getByRole("searchbox", { name: "Filter metrics" }), "GOROUTINES");
    expect(await screen.findByText(/^GET \/metrics · 3 of \d+ lines$/)).toBeInTheDocument();
    expect(within(output).queryByText(/worktree_http_requests_total/)).not.toBeInTheDocument();
    expect(within(output).getByText("go_goroutines 42")).toBeInTheDocument();

    await user.clear(screen.getByRole("searchbox", { name: "Filter metrics" }));
    await user.type(screen.getByRole("searchbox", { name: "Filter metrics" }), "nothing-matches");
    expect(await within(output).findByText(/No lines match/)).toBeInTheDocument();
  });

  it("shows a forbidden state to non-admins without calling admin endpoints", async () => {
    const { fetchMock } = renderApp({
      route: "/system",
      api: { ...api, "GET /api/v1/auth/me": { ...fx.me, account: { ...fx.account, is_admin: false } } },
    });
    expect(await screen.findByText("Server administrators only")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Raw metrics" })).not.toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/admin/system")).toHaveLength(0);
    expect(fetchMock.find("GET /metrics")).toHaveLength(0);
  });
});
