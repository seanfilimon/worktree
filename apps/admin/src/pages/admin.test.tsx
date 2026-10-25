import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import { json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

describe("accounts page", () => {
  it("toggles admin and disabled flags with PATCH after confirmation", async () => {
    const { user, fetchMock } = renderApp({
      route: "/accounts",
      api: {
        "GET /api/v1/admin/accounts": [fx.account, fx.bob],
        "PATCH /api/v1/admin/accounts/bob": (req) => json({ ...fx.bob, ...(req.body as object) }),
      },
    });
    const disable = await screen.findByRole("switch", { name: "Disabled: bob" });
    expect(disable).toHaveAttribute("aria-checked", "false");
    await user.click(disable);
    await user.click(await screen.findByRole("button", { name: "Disable account" }));
    await waitFor(() => expect(screen.getByRole("switch", { name: "Disabled: bob" })).toHaveAttribute("aria-checked", "true"));
    expect(fetchMock.find("PATCH /api/v1/admin/accounts/bob")[0]?.body).toEqual({ disabled: true });

    await user.click(screen.getByRole("switch", { name: "Server admin: bob" }));
    await user.click(await screen.findByRole("button", { name: "Grant admin" }));
    await waitFor(() => expect(fetchMock.find("PATCH /api/v1/admin/accounts/bob")).toHaveLength(2));
    expect(fetchMock.find("PATCH /api/v1/admin/accounts/bob")[1]?.body).toEqual({ is_admin: true });
  });

  it("prevents changing your own account", async () => {
    renderApp({ route: "/accounts", api: { "GET /api/v1/admin/accounts": [fx.account, fx.bob] } });
    expect(await screen.findByRole("switch", { name: "Server admin: alice" })).toBeDisabled();
    expect(screen.getByRole("switch", { name: "Server admin: bob" })).toBeEnabled();
  });

  it("is restricted to server admins", async () => {
    renderApp({ route: "/accounts", api: { "GET /api/v1/auth/me": { ...fx.me, account: { ...fx.account, is_admin: false } } } });
    expect(await screen.findByText("Server admin access required")).toBeInTheDocument();
  });
});

describe("settings page", () => {
  it("creates a token and shows it exactly once", async () => {
    let tokens: unknown[] = [];
    const created = { id: "tk1", account_id: "acc-1", name: "ci", created_at: new Date().toISOString() };
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: {
        "GET /api/v1/auth/tokens": () => json(tokens),
        "POST /api/v1/auth/tokens": () => {
          tokens = [created];
          return json({ token: "wt_secret_value", api_token: created }, 201);
        },
      },
    });
    expect(await screen.findByText("No API tokens")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New token" }));
    const dialog = await screen.findByRole("dialog", { name: "New API token" });
    await user.type(within(dialog).getByLabelText("Token name"), "ci");
    await user.click(within(dialog).getByRole("combobox", { name: "Expires" }));
    await user.click(await screen.findByRole("option", { name: "7 days" }));
    await user.click(within(dialog).getByRole("button", { name: "Create token" }));

    expect(await screen.findByLabelText("New API token")).toHaveValue("wt_secret_value");
    expect(fetchMock.find("POST /api/v1/auth/tokens")[0]?.body).toEqual({ name: "ci", ttl_hours: 168 });

    await user.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => expect(screen.queryByLabelText("New API token")).not.toBeInTheDocument());
    expect(screen.queryByDisplayValue("wt_secret_value")).not.toBeInTheDocument();
    expect(await screen.findByRole("table", { name: "API tokens" })).toHaveTextContent("ci");

    // Reopening starts a fresh form: the secret is gone for good.
    await user.click(screen.getByRole("button", { name: "New token" }));
    expect(await screen.findByRole("dialog", { name: "New API token" })).toBeInTheDocument();
    expect(screen.queryByDisplayValue("wt_secret_value")).not.toBeInTheDocument();
  });

  it("revokes a token after confirmation", async () => {
    const token = { id: "tk9", account_id: "acc-1", name: "laptop", created_at: new Date().toISOString() };
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: { "GET /api/v1/auth/tokens": [token], "DELETE /api/v1/auth/tokens/tk9": noContent() },
    });
    await user.click(await screen.findByRole("button", { name: "Revoke laptop" }));
    await user.click(await screen.findByRole("button", { name: "Revoke" }));
    expect(await screen.findByText("Token revoked")).toBeInTheDocument();
    expect(fetchMock.find("DELETE /api/v1/auth/tokens/tk9")).toHaveLength(1);
  });

  it("switches the theme", async () => {
    const { user } = renderApp({ route: "/settings", api: { "GET /api/v1/auth/tokens": [] } });
    await user.click(await screen.findByRole("radio", { name: "dark" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("wt.admin.theme")).toBe("dark");
    await user.click(screen.getByRole("radio", { name: "light" }));
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

describe("tenant detail", () => {
  it("edits member roles inline", async () => {
    const { user, fetchMock } = renderApp({
      route: "/tenants/acme",
      api: {
        "GET /api/v1/tenants/acme": fx.tenant,
        "GET /api/v1/tenants/acme/trees": [fx.summary],
        "GET /api/v1/tenants/acme/members": [
          { account: fx.account, role: "owner" },
          { account: fx.bob, role: "developer" },
        ],
        "PUT /api/v1/tenants/acme/members/bob": (req) => json({ account: fx.bob, role: (req.body as { role: string }).role }),
      },
    });
    expect(await screen.findByRole("table", { name: "Trees" })).toHaveTextContent("web");
    await user.click(screen.getByRole("tab", { name: "Members" }));
    const members = await screen.findByRole("table", { name: "Members" });
    await user.click(within(members).getByRole("combobox", { name: "Role of bob" }));
    await user.click(await screen.findByRole("option", { name: "maintainer" }));
    expect(await screen.findByText("Role updated")).toBeInTheDocument();
    expect(fetchMock.find("PUT /api/v1/tenants/acme/members/bob")[0]?.body).toEqual({ role: "maintainer" });
  });
});
