import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as ox from "@/test/fixtures.ops";
import { apiError } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const opsRoutes = {
  "GET /api/v1/admin/gc": ox.gcStatus,
  "GET /api/v1/tenants/acme/usage": ox.usage,
  "GET /api/v1/admin/tenants/acme/limits": ox.usage.limits,
};

describe("OperationsPage", () => {
  it("is restricted to server admins", async () => {
    const { fetchMock } = renderApp({ route: "/operations", api: { "GET /api/v1/auth/me": fx.memberMe, ...opsRoutes } });
    expect(await screen.findByText("Server administrators only")).toBeInTheDocument();
    expect(fetchMock.find("GET /api/v1/admin/gc")).toHaveLength(0);
  });

  it("shows the server configuration, the latest run and the run history", async () => {
    renderApp({ route: "/operations", api: opsRoutes });
    const dl = await screen.findByLabelText("Server configuration");
    expect(dl).toHaveTextContent("GC interval");
    expect(dl).toHaveTextContent("1d 0h");
    expect(dl).toHaveTextContent("720h0m0s");
    expect(dl).toHaveTextContent("Audit retention");
    expect(dl).toHaveTextContent("keep forever");

    const report = screen.getByRole("region", { name: "GC report" });
    expect(report).toHaveTextContent("Collection");
    expect(report).toHaveTextContent("took 2.5 s");
    expect(report).toHaveTextContent("3.0 MiB");

    const runs = screen.getByRole("table", { name: "GC runs" });
    expect(within(runs).getAllByRole("row")).toHaveLength(3);
    expect(within(runs).getByText("dry run")).toBeInTheDocument();
    expect(within(runs).getByText("collect")).toBeInTheDocument();
  });

  it("runs a dry run, and a real collection only after confirmation", async () => {
    const { user, fetchMock } = renderApp({
      route: "/operations",
      api: {
        ...opsRoutes,
        "POST /api/v1/admin/gc": (req) =>
          req.url.searchParams.get("dry_run") === "true"
            ? { ...ox.gcDryRun, unreachable: 77 }
            : { ...ox.gcRun, deleted: 41 },
      },
    });
    await user.click(await screen.findByRole("button", { name: "Dry run" }));
    expect(await screen.findByText("Dry run finished")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/admin/gc")[0]!.url.searchParams.get("dry_run")).toBe("true");
    const report = screen.getByRole("region", { name: "GC report" });
    expect(report).toHaveTextContent("Dry run");
    expect(report).toHaveTextContent("77");

    await user.click(screen.getByRole("button", { name: "Run GC" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Run garbage collection now?" });
    expect(fetchMock.find("POST /api/v1/admin/gc")).toHaveLength(1);
    await user.click(within(confirm).getByRole("button", { name: "Run GC" }));
    expect(await screen.findByText("Garbage collection finished")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/admin/gc")[1]!.url.searchParams.get("dry_run")).toBe("false");
    await waitFor(() => expect(screen.getByRole("region", { name: "GC report" })).toHaveTextContent("41"));
  });

  it("explains 501 not_implemented and still shows tenant quotas", async () => {
    renderApp({
      route: "/operations",
      api: { ...opsRoutes, "GET /api/v1/admin/gc": apiError(501, "not_implemented", "gc is not configured") },
    });
    expect(await screen.findByText("Garbage collection is not available on this server")).toBeInTheDocument();
    const usage = await screen.findByRole("table", { name: "Usage" });
    expect(usage).toHaveTextContent("Trees");
    expect(usage).toHaveTextContent("512.0 MiB");
    expect(usage).toHaveTextContent("1.0 GiB");
    expect(screen.getByRole("progressbar", { name: "Storage used" })).toBeInTheDocument();
    expect(await screen.findByRole("form", { name: "Tenant limits" })).toBeInTheDocument();
  });
});
