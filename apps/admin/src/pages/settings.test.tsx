import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as px from "@/test/fixtures.people";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const base = { "GET /api/v1/auth/me": px.meMulti, "GET /api/v1/auth/tokens": [] };

describe("settings page (rebuilt)", () => {
  it("shows the profile with the personal tenant and organisations", async () => {
    renderApp({ route: "/settings", api: base });
    const orgs = await screen.findByRole("list", { name: "Your organisations" });
    expect(orgs).toHaveTextContent("acme");
    expect(orgs).toHaveTextContent("globex");
    expect(orgs).not.toHaveTextContent("alice");
    expect(screen.getByRole("link", { name: "alice" })).toHaveAttribute("href", "/tenants/alice");
  });

  it("validates and changes the password", async () => {
    const { user, fetchMock } = renderApp({ route: "/settings", api: { ...base, "POST /api/v1/auth/password": noContent() } });
    const form = await screen.findByRole("form", { name: "Change password" });
    await user.click(within(form).getByRole("button", { name: "Change password" }));
    expect(await within(form).findByText("Enter your current password.")).toBeInTheDocument();

    await user.type(within(form).getByLabelText("Current password"), "old-password");
    await user.type(within(form).getByLabelText("New password"), "old-password");
    await user.type(within(form).getByLabelText("Confirm new password"), "something-else");
    await user.click(within(form).getByRole("button", { name: "Change password" }));
    expect(await within(form).findByText("Choose a password different from the current one.")).toBeInTheDocument();
    expect(within(form).getByText("Passwords do not match.")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/auth/password")).toHaveLength(0);

    const next = within(form).getByLabelText("New password");
    await user.clear(next);
    await user.type(next, "Correct-Horse-9-Battery");
    expect(within(form).getByRole("meter", { name: "Password strength" })).toHaveAttribute("aria-valuetext", "Strong");
    const confirm = within(form).getByLabelText("Confirm new password");
    await user.clear(confirm);
    await user.type(confirm, "Correct-Horse-9-Battery");
    await user.click(within(form).getByRole("button", { name: "Change password" }));

    expect(await screen.findByText("Password changed")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/auth/password")[0]?.body).toEqual({
      current_password: "old-password",
      new_password: "Correct-Horse-9-Battery",
    });
    await waitFor(() => expect(within(form).getByLabelText("Current password")).toHaveValue(""));
  });

  it("shows a wrong current password inline", async () => {
    const { user } = renderApp({
      route: "/settings",
      api: { ...base, "POST /api/v1/auth/password": apiError(403, "forbidden", "current password is incorrect") },
    });
    const form = await screen.findByRole("form", { name: "Change password" });
    await user.type(within(form).getByLabelText("Current password"), "nope-nope");
    await user.type(within(form).getByLabelText("New password"), "brand-new-pass");
    await user.type(within(form).getByLabelText("Confirm new password"), "brand-new-pass");
    await user.click(within(form).getByRole("button", { name: "Change password" }));
    expect(await within(form).findByText("current password is incorrect")).toBeInTheDocument();
  });

  it("toggles password visibility", async () => {
    const { user } = renderApp({ route: "/settings", api: base });
    const form = await screen.findByRole("form", { name: "Change password" });
    const current = within(form).getByLabelText("Current password");
    expect(current).toHaveAttribute("type", "password");
    await user.click(within(form).getAllByRole("button", { name: "Show password" })[0]!);
    expect(current).toHaveAttribute("type", "text");
  });

  it("requires a token name and copies the one-time token", async () => {
    const created = { id: "tk1", account_id: "acc-1", name: "deploy", created_at: new Date().toISOString() };
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: { ...base, "POST /api/v1/auth/tokens": json({ token: "wt_once", api_token: created }, 201) },
    });
    await user.click(await screen.findByRole("button", { name: "New token" }));
    const dialog = await screen.findByRole("dialog", { name: "New API token" });
    await user.click(within(dialog).getByRole("button", { name: "Create token" }));
    expect(within(dialog).getByText("Give the token a name.")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/auth/tokens")).toHaveLength(0);

    await user.type(within(dialog).getByLabelText("Token name"), "deploy");
    await user.click(within(dialog).getByRole("combobox", { name: "Expires" }));
    await user.click(await screen.findByRole("option", { name: "Never" }));
    await user.click(within(dialog).getByRole("button", { name: "Create token" }));
    const reveal = await screen.findByRole("dialog", { name: "Token “deploy” created" });
    expect(fetchMock.find("POST /api/v1/auth/tokens")[0]?.body).toEqual({ name: "deploy", ttl_hours: 0 });
    await user.click(within(reveal).getByRole("button", { name: "Copy token" }));
    expect(await navigator.clipboard.readText()).toBe("wt_once");
    expect(await screen.findByText("Token copied to clipboard")).toBeInTheDocument();
  });

  it("marks expired tokens", async () => {
    const expired = { id: "tk2", account_id: "acc-1", name: "old", created_at: "2025-01-01T00:00:00Z", expires_at: "2025-02-01T00:00:00Z" };
    renderApp({ route: "/settings", api: { ...base, "GET /api/v1/auth/tokens": [expired] } });
    const table = await screen.findByRole("table", { name: "API tokens" });
    expect(within(table).getByText("expired")).toBeInTheDocument();
  });

  it("signs out of the current session", async () => {
    const { user, fetchMock, router } = renderApp({ route: "/settings", api: { ...base, "POST /api/v1/auth/logout": noContent() } });
    await user.click(await screen.findByRole("button", { name: "Sign out of this session" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
    expect(fetchMock.find("POST /api/v1/auth/logout")).toHaveLength(1);
  });
});
