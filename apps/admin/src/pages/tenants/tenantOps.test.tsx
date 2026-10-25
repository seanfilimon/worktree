import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { TenantV05 } from "@/api/v05ops";
import * as fx from "@/test/fixtures";
import * as ox from "@/test/fixtures.ops";
import { apiError, json } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const route = "/tenants/acme?tab=settings";

function routes(t: TenantV05 = fx.tenant) {
  return {
    "GET /api/v1/tenants/acme": t,
    "GET /api/v1/tenants/acme/usage": ox.usage,
    "GET /api/v1/admin/tenants/acme/limits": ox.usage.limits,
  };
}

describe("tenant settings: status", () => {
  it("suspends with a required reason and then offers resume", async () => {
    const { user, fetchMock } = renderApp({
      route,
      api: {
        ...routes(),
        "PATCH /api/v1/admin/tenants/acme": (req) => ({ ...fx.tenant, ...(req.body as object) }),
      },
    });
    const status = await screen.findByRole("group", { name: "Tenant status" });
    expect(status).toHaveTextContent("Active");

    await user.click(screen.getByRole("button", { name: "Suspend" }));
    const dialog = await screen.findByRole("dialog", { name: "Suspend tenant" });
    await user.click(within(dialog).getByRole("button", { name: "Suspend" }));
    expect(await within(dialog).findByText("Give a reason; members and admins see it.")).toBeInTheDocument();
    expect(fetchMock.find("PATCH /api/v1/admin/tenants/acme")).toHaveLength(0);

    await user.type(within(dialog).getByLabelText("Reason"), "Billing overdue");
    await user.click(within(dialog).getByRole("button", { name: "Suspend" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(fetchMock.find("PATCH /api/v1/admin/tenants/acme")[0]!.body).toEqual({
      status: "suspended",
      status_reason: "Billing overdue",
    });
    expect(screen.getByRole("group", { name: "Tenant status" })).toHaveTextContent("Suspended");
    expect(screen.getByRole("group", { name: "Tenant status" })).toHaveTextContent("Reason: Billing overdue");
    expect(screen.getByRole("button", { name: "Resume" })).toBeInTheDocument();
  });

  it("reactivates a deactivated tenant without a reason", async () => {
    const { user, fetchMock } = renderApp({
      route,
      api: {
        ...routes({ ...fx.tenant, status: "deactivated", status_reason: "Closed" }),
        "PATCH /api/v1/admin/tenants/acme": { ...fx.tenant, status: "active" },
      },
    });
    expect(await screen.findByRole("group", { name: "Tenant status" })).toHaveTextContent("Deactivated");
    expect(screen.queryByRole("button", { name: "Suspend" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Reactivate" }));
    const dialog = await screen.findByRole("dialog", { name: "Reactivate tenant" });
    expect(within(dialog).queryByLabelText("Reason")).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Reactivate" }));
    await waitFor(() => expect(fetchMock.find("PATCH /api/v1/admin/tenants/acme")).toHaveLength(1));
    expect(fetchMock.find("PATCH /api/v1/admin/tenants/acme")[0]!.body).toEqual({ status: "active", status_reason: "" });
    expect(await screen.findByRole("button", { name: "Suspend" })).toBeInTheDocument();
  });

  it("is read-only for non-admins", async () => {
    renderApp({ route, api: { ...routes({ ...fx.tenant, status: "suspended" }), "GET /api/v1/auth/me": fx.memberMe } });
    expect(await screen.findByRole("group", { name: "Tenant status" })).toHaveTextContent("Suspended");
    expect(screen.getByText("Only server admins can suspend or deactivate a tenant.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Resume" })).not.toBeInTheDocument();
    expect(await screen.findByRole("table", { name: "Usage" })).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Tenant limits" })).not.toBeInTheDocument();
  });
});

describe("tenant settings: limits", () => {
  it("edits limits and saves the overrides", async () => {
    const { user, fetchMock } = renderApp({
      route,
      api: {
        ...routes(),
        "PUT /api/v1/admin/tenants/acme/limits": (req) => json(req.body),
      },
    });
    const form = await screen.findByRole("form", { name: "Tenant limits" });
    const trees = within(form).getByLabelText("Max trees");
    expect(trees).toHaveValue("10");
    expect(within(form).getByRole("button", { name: "Save limits" })).toBeDisabled();

    await user.clear(trees);
    await user.type(trees, "-1");
    expect(within(form).getByText("Whole number, 0 or more.")).toBeInTheDocument();
    expect(within(form).getByRole("button", { name: "Save limits" })).toBeDisabled();

    await user.clear(trees);
    await user.type(trees, "25");
    await user.type(within(form).getByLabelText("Max storage"), "5");
    await user.type(within(form).getByLabelText("Requests per minute"), "600");
    await user.click(within(form).getByRole("button", { name: "Save limits" }));

    expect(await screen.findByText("Limits saved")).toBeInTheDocument();
    expect(fetchMock.find("PUT /api/v1/admin/tenants/acme/limits")[0]!.body).toEqual({
      max_trees: 25,
      max_storage_bytes: 5 * 1024 ** 3,
      requests_per_minute: 600,
    });
  });

  it("clears the overrides", async () => {
    const { user, fetchMock } = renderApp({
      route,
      api: { ...routes(), "PUT /api/v1/admin/tenants/acme/limits": {} },
    });
    const form = await screen.findByRole("form", { name: "Tenant limits" });
    await user.click(within(form).getByRole("button", { name: "Clear overrides" }));
    await waitFor(() => expect(fetchMock.find("PUT /api/v1/admin/tenants/acme/limits")).toHaveLength(1));
    expect(fetchMock.find("PUT /api/v1/admin/tenants/acme/limits")[0]!.body).toEqual({});
  });

  it("explains 501 not_implemented", async () => {
    renderApp({
      route,
      api: {
        ...routes(),
        "GET /api/v1/tenants/acme/usage": apiError(501, "not_implemented", "limits are not configured"),
      },
    });
    expect(await screen.findByText("Tenant usage and limits is not available on this server")).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Tenant limits" })).not.toBeInTheDocument();
  });
});
