import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { countdown, movedTo, type TenantDeletion, type TenantV08 } from "@/api/tenantLifecycle";
import { ApiError } from "@/api/client";
import * as fx from "@/test/fixtures";
import * as ox from "@/test/fixtures.ops";
import { apiError, json } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const route = "/tenants/acme?tab=settings";

function routes(t: TenantV08 = fx.tenant) {
  return {
    "GET /api/v1/tenants/acme": t,
    "GET /api/v1/tenants/acme/usage": ox.usage,
    "GET /api/v1/admin/tenants/acme/limits": ox.usage.limits,
  };
}

const deletion: TenantDeletion = {
  tenant_id: fx.tenant.id,
  slug: "acme",
  requested_by: "alice",
  requested_at: "2026-10-20T12:00:00Z",
  purge_at: new Date(Date.now() + (3 * 86400 + 5 * 3600) * 1000).toISOString(),
  revoked_keys: 2,
};

describe("tenant lifecycle helpers", () => {
  it("formats countdowns", () => {
    const now = Date.parse("2026-10-20T00:00:00Z");
    expect(countdown(now + (29 * 86400 + 23 * 3600 + 59) * 1000, now)).toBe("29d 23h");
    expect(countdown(now + (4 * 3600 + 5 * 60) * 1000, now)).toBe("4h 05m");
    expect(countdown(now + 200_000, now)).toBe("3m 20s");
    expect(countdown(now - 1000, now)).toBe("now");
  });

  it("reads the new slug of tenant_moved errors", () => {
    expect(movedTo(new ApiError(301, "tenant_moved", "renamed", { details: { old_slug: "a", new_slug: "b" } }))).toBe("b");
    expect(movedTo(new ApiError(404, "not_found", "x"))).toBeUndefined();
    expect(movedTo(new Error("x"))).toBeUndefined();
  });
});

describe("tenant settings: contact email", () => {
  it("sets the email, then verifies it with the code", async () => {
    let current: TenantV08 = { ...fx.tenant };
    const { user, fetchMock } = renderApp({
      route,
      api: {
        ...routes(),
        "GET /api/v1/tenants/acme": () => current,
        "PATCH /api/v1/tenants/acme": (req) => {
          current = { ...current, email: (req.body as { email: string }).email, email_verified_at: null };
          return current;
        },
        "POST /api/v1/tenants/acme/email/verify": (req) => {
          if ((req.body as { token: string }).token !== "wte_good") return apiError(400, "invalid_email_token", "invalid, expired or already used verification code");
          current = { ...current, email_verified_at: "2026-10-21T09:00:00Z" };
          return current;
        },
        "POST /api/v1/tenants/acme/email/resend": { email: "ops@acme.io", delivery: "log", expires_at: "2026-10-22T09:00:00Z" },
      },
    });
    const status = await screen.findByRole("group", { name: "Email status" });
    expect(status).toHaveTextContent("No contact email.");

    const form = screen.getByRole("form", { name: "Contact email" });
    await user.type(within(form).getByLabelText("Email address"), "not-an-email");
    expect(within(form).getByText("Enter an address like ops@example.com.")).toBeInTheDocument();
    expect(within(form).getByRole("button", { name: "Save email" })).toBeDisabled();
    await user.clear(within(form).getByLabelText("Email address"));
    await user.type(within(form).getByLabelText("Email address"), "ops@acme.io");
    await user.click(within(form).getByRole("button", { name: "Save email" }));

    await waitFor(() => expect(screen.getByRole("group", { name: "Email status" })).toHaveTextContent("Unverified"));
    expect(fetchMock.find("PATCH /api/v1/tenants/acme")[0]!.body).toEqual({ email: "ops@acme.io" });

    const verify = screen.getByRole("form", { name: "Verify email" });
    await user.click(within(verify).getByRole("button", { name: "Resend code" }));
    expect(await within(verify).findByRole("status")).toHaveTextContent("written to the server log");

    await user.type(within(verify).getByLabelText("Verification code"), "wte_bad");
    await user.click(within(verify).getByRole("button", { name: "Verify" }));
    expect(await within(verify).findByText("invalid, expired or already used verification code")).toBeInTheDocument();

    await user.clear(within(verify).getByLabelText("Verification code"));
    await user.type(within(verify).getByLabelText("Verification code"), "wte_good");
    await user.click(within(verify).getByRole("button", { name: "Verify" }));
    await waitFor(() => expect(screen.getByRole("group", { name: "Email status" })).toHaveTextContent("Verified"));
    expect(screen.queryByRole("form", { name: "Verify email" })).not.toBeInTheDocument();
  });

  it("pre-fills the code from the verification link", async () => {
    renderApp({
      route: "/tenants/acme?tab=settings&email_token=wte_fromlink",
      api: routes({ ...fx.tenant, email: "ops@acme.io", email_verified_at: null }),
    });
    const verify = await screen.findByRole("form", { name: "Verify email" });
    expect(within(verify).getByLabelText("Verification code")).toHaveValue("wte_fromlink");
  });
});

describe("tenant settings: rename", () => {
  it("validates the slug, renames and moves to the new URL", async () => {
    const renamed = { ...fx.tenant, slug: "acme-inc" };
    const { user, fetchMock, router } = renderApp({
      route,
      api: {
        ...routes(),
        "GET /api/v1/tenants/acme-inc": renamed,
        "GET /api/v1/tenants/acme-inc/usage": ox.usage,
        "GET /api/v1/admin/tenants/acme-inc/limits": ox.usage.limits,
        "POST /api/v1/tenants/acme/rename": (req) =>
          json({
            tenant: { ...renamed, slug: (req.body as { slug: string }).slug },
            reservation: { slug: "acme", tenant_id: fx.tenant.id, renamed_to: "acme-inc", reserved_at: "2026-10-20T12:00:00Z", expires_at: "2027-01-18T12:00:00Z" },
          }),
      },
    });
    await user.click(await screen.findByRole("button", { name: "Change slug" }));
    const dialog = await screen.findByRole("dialog", { name: "Change slug" });
    await user.type(within(dialog).getByLabelText("New slug"), "Acme Inc");
    await user.click(within(dialog).getByRole("button", { name: "Change slug" }));
    expect(within(dialog).getByText(/1–63 lowercase letters/)).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/tenants/acme/rename")).toHaveLength(0);

    await user.clear(within(dialog).getByLabelText("New slug"));
    await user.type(within(dialog).getByLabelText("New slug"), "acme-inc");
    await user.click(within(dialog).getByRole("button", { name: "Change slug" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/tenants/acme-inc"));
    expect(fetchMock.find("POST /api/v1/tenants/acme/rename")[0]!.body).toEqual({ slug: "acme-inc" });
    expect(await screen.findByText("Tenant renamed")).toBeInTheDocument();
  });

  it("explains a renamed tenant's old slug", async () => {
    renderApp({
      route: "/tenants/acme",
      api: {
        "GET /api/v1/tenants/acme": apiError(301, "tenant_moved", "renamed", { details: { old_slug: "acme", new_slug: "acme-inc" } }),
      },
    });
    expect(await screen.findByText("This tenant was renamed")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Go to acme-inc/ })).toHaveAttribute("href", "/tenants/acme-inc");
  });

  it("hides owner actions from non-owners", async () => {
    renderApp({ route, api: { ...routes(), "GET /api/v1/auth/me": fx.memberMe } });
    expect(await screen.findByText("Only owners and server admins can delete or restore this tenant.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Change slug" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete tenant" })).not.toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Contact email" })).not.toBeInTheDocument();
  });
});

describe("tenant settings: danger zone", () => {
  it("deletes after typing the slug, then counts down and restores", async () => {
    let scheduled: TenantDeletion | null = null;
    const { user, fetchMock } = renderApp({
      route,
      api: {
        ...routes(),
        "GET /api/v1/tenants/acme/deletion": () => (scheduled ? json(scheduled) : apiError(404, "deletion_not_scheduled", "no deletion")),
        "DELETE /api/v1/tenants/acme": () => {
          scheduled = deletion;
          return json(deletion, 202);
        },
        "POST /api/v1/tenants/acme/restore": () => {
          scheduled = null;
          return fx.tenant;
        },
      },
    });
    await user.click(await screen.findByRole("button", { name: "Delete tenant" }));
    const dialog = await screen.findByRole("alertdialog", { name: "Delete tenant" });
    const confirm = within(dialog).getByRole("button", { name: "Delete tenant" });
    expect(confirm).toBeDisabled();
    await user.type(within(dialog).getByLabelText(/Type acme to confirm/), "acme");
    await user.click(confirm);
    await waitFor(() => expect(fetchMock.find("DELETE /api/v1/tenants/acme")).toHaveLength(1));

    const group = await screen.findByRole("group", { name: "Scheduled deletion" });
    expect(within(group).getByRole("timer", { name: "Time until purge" }).textContent).toMatch(/^3d [45]h$/);
    expect(group).toHaveTextContent("2 service-account key(s) were revoked");

    await user.click(within(group).getByRole("button", { name: "Restore tenant" }));
    await waitFor(() => expect(fetchMock.find("POST /api/v1/tenants/acme/restore")).toHaveLength(1));
    expect(await screen.findByRole("button", { name: "Delete tenant" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Scheduled deletion" })).not.toBeInTheDocument();
  });

  it("shows a scheduled deletion on load", async () => {
    renderApp({
      route,
      api: { ...routes({ ...fx.tenant, status: "suspended", status_reason: "scheduled for deletion" }), "GET /api/v1/tenants/acme/deletion": deletion },
    });
    expect(await screen.findByRole("group", { name: "Scheduled deletion" })).toHaveTextContent("Purged in");
    // A tenant scheduled for deletion is read-only: no slug change, no email edits.
    expect(screen.queryByRole("button", { name: "Change slug" })).not.toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "Contact email" })).not.toBeInTheDocument();
  });

  it("explains that personal tenants cannot be deleted", async () => {
    renderApp({ route, api: routes({ ...fx.tenant, kind: "user" }) });
    expect(await screen.findByText(/Personal tenants belong to their account/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete tenant" })).not.toBeInTheDocument();
  });
});
