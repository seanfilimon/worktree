import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Account } from "@/api/types";
import * as fx from "@/test/fixtures";
import * as px from "@/test/fixtures.people";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const routes = {
  "GET /api/v1/admin/accounts": [fx.account, fx.bob, px.carol],
};

describe("accounts page (rebuilt)", () => {
  it("filters accounts by search", async () => {
    const { user } = renderApp({ route: "/accounts", api: routes });
    const table = await screen.findByRole("table", { name: "Accounts" });
    await user.type(screen.getByRole("searchbox", { name: "Search accounts" }), "viewer");
    expect(within(table).queryByRole("button", { name: "bob" })).not.toBeInTheDocument();
    expect(within(table).getByRole("button", { name: "carol" })).toBeInTheDocument();
    await user.clear(screen.getByRole("searchbox", { name: "Search accounts" }));
    await user.type(screen.getByRole("searchbox", { name: "Search accounts" }), "zzz");
    expect(screen.getByText("No accounts match your search")).toBeInTheDocument();
  });

  it("creates an account", async () => {
    let accounts: Account[] = [fx.account];
    const dave: Account = { ...px.carol, id: "acc-4", username: "dave", email: "dave@example.com", display_name: "" };
    const { user, fetchMock } = renderApp({
      route: "/accounts",
      api: {
        "GET /api/v1/admin/accounts": () => json(accounts),
        "POST /api/v1/admin/accounts": (req) => {
          accounts = [...accounts, { ...dave, is_admin: (req.body as { is_admin: boolean }).is_admin }];
          return json(accounts.at(-1), 201);
        },
      },
    });
    await user.click(await screen.findByRole("button", { name: "New account" }));
    const dialog = await screen.findByRole("dialog", { name: "New account" });
    await user.click(within(dialog).getByRole("button", { name: "Create account" }));
    expect(await within(dialog).findByText(/2–39 lowercase/)).toBeInTheDocument();
    expect(within(dialog).getByText("Enter a valid email address.")).toBeInTheDocument();
    expect(within(dialog).getByText("At least 8 characters.")).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText("Username"), "Dave");
    await user.type(within(dialog).getByLabelText("Email"), "dave@example.com");
    await user.type(within(dialog).getByLabelText("Password"), "short");
    expect(within(dialog).getByRole("meter", { name: "Password strength" })).toHaveAttribute("aria-valuetext", "Too short");
    await user.click(within(dialog).getByRole("button", { name: "Generate" }));
    const password = within(dialog).getByLabelText<HTMLInputElement>("Password");
    const generated = password.value;
    expect(generated).toHaveLength(20);
    expect(password).toHaveAttribute("type", "text");
    expect(within(dialog).getByRole("meter", { name: "Password strength" })).toHaveAttribute("aria-valuetext", "Strong");
    await user.click(within(dialog).getByRole("checkbox", { name: "Server admin" }));
    await user.click(within(dialog).getByRole("button", { name: "Create account" }));

    expect(await screen.findByText("Account created")).toBeInTheDocument();
    const body = fetchMock.find("POST /api/v1/admin/accounts")[0]?.body as Record<string, unknown>;
    expect(body).toMatchObject({ username: "dave", email: "dave@example.com", is_admin: true });
    expect(body.password).toBe(generated);
    expect(await screen.findByRole("button", { name: "dave" })).toBeInTheDocument();
  });

  it("resets a password with a generated value", async () => {
    const { user, fetchMock } = renderApp({
      route: "/accounts",
      api: { ...routes, "POST /api/v1/admin/accounts/bob/password": noContent() },
    });
    const table = await screen.findByRole("table", { name: "Accounts" });
    await user.click(within(table).getByRole("button", { name: "Actions for bob" }));
    await user.click(await screen.findByRole("menuitem", { name: "Reset password" }));
    const dialog = await screen.findByRole("dialog", { name: "Reset password" });

    await user.click(within(dialog).getByRole("button", { name: "Reset password" }));
    expect(within(dialog).getByText("At least 8 characters.")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/admin/accounts/bob/password")).toHaveLength(0);

    await user.click(within(dialog).getByRole("button", { name: "Generate" }));
    const value = within(dialog).getByLabelText<HTMLInputElement>("New password").value;
    await user.click(within(dialog).getByRole("button", { name: "Copy password" }));
    // user-event installs a clipboard stub on navigator.
    expect(await navigator.clipboard.readText()).toBe(value);
    await user.click(within(dialog).getByRole("button", { name: "Reset password" }));

    expect(await screen.findByText("Password reset")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/admin/accounts/bob/password")[0]?.body).toEqual({ password: value });
  });

  it("re-enables an account without confirmation and cancels a demotion", async () => {
    const disabledBob = { ...fx.bob, disabled: true, is_admin: true };
    const { user, fetchMock } = renderApp({
      route: "/accounts",
      api: {
        "GET /api/v1/admin/accounts": [fx.account, disabledBob],
        "PATCH /api/v1/admin/accounts/bob": (req) => json({ ...disabledBob, ...(req.body as object) }),
      },
    });
    await user.click(await screen.findByRole("switch", { name: "Disabled: bob" }));
    await waitFor(() => expect(fetchMock.find("PATCH /api/v1/admin/accounts/bob")).toHaveLength(1));
    expect(fetchMock.find("PATCH /api/v1/admin/accounts/bob")[0]?.body).toEqual({ disabled: false });

    await user.click(screen.getByRole("switch", { name: "Server admin: bob" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Remove admin rights from bob?" });
    await user.click(within(confirm).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(fetchMock.find("PATCH /api/v1/admin/accounts/bob")).toHaveLength(1);
    expect(screen.getByRole("switch", { name: "Server admin: bob" })).toHaveAttribute("aria-checked", "true");
  });

  it("shows account details with memberships in a sheet", async () => {
    const { user } = renderApp({
      route: "/accounts",
      api: {
        ...routes,
        "GET /api/v1/tenants": [fx.tenant, px.globex],
        "GET /api/v1/tenants/acme/members": px.acmeMembers,
        "GET /api/v1/tenants/globex/members": [{ account: fx.bob, role: "admin" }],
      },
    });
    await user.click(await screen.findByRole("button", { name: "bob" }));
    const sheet = await screen.findByRole("dialog", { name: "bob" });
    const list = await within(sheet).findByRole("list", { name: "Memberships" });
    expect(within(list).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      expect.stringContaining("acme"),
      expect.stringContaining("globex"),
    ]);
    expect(list).toHaveTextContent("developer");
    expect(list).toHaveTextContent("admin");
  });

  it("reports membership load failures in the sheet", async () => {
    const { user } = renderApp({
      route: "/accounts",
      api: { ...routes, "GET /api/v1/tenants/acme/members": apiError(500, "internal", "boom") },
    });
    await user.click(await screen.findByRole("button", { name: "carol" }));
    const sheet = await screen.findByRole("dialog", { name: "carol" });
    expect(await within(sheet).findByText("boom")).toBeInTheDocument();
  });
});
