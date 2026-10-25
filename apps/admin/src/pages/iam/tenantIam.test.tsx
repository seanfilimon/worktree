import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as ix from "@/test/fixtures.iam";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const T = "/api/v1/tenants/acme";

function open(tab: string, api: Record<string, unknown> = {}) {
  return renderApp({ route: `/tenants/acme?tab=${tab}`, api: { ...ix.tenantIamRoutes(), ...(api as object) } });
}

describe("tenant teams tab", () => {
  it("creates a team with a derived slug and opens it", async () => {
    const { user, fetchMock } = open("teams", {
      [`POST ${T}/teams`]: (req: { body: unknown }) => json({ ...ix.team, id: "team-2", ...(req.body as object) }, 201),
      [`GET ${T}/teams/web-team`]: { team: { ...ix.team, slug: "web-team", name: "Web Team" }, members: null, grants: null },
    });
    expect(await screen.findByRole("table", { name: "Teams" })).toHaveTextContent("Platform");
    await user.click(screen.getByRole("button", { name: "New team" }));
    const dialog = await screen.findByRole("dialog", { name: "New team" });
    await user.type(within(dialog).getByLabelText("Name"), "Web Team");
    expect(within(dialog).getByLabelText("Slug")).toHaveValue("web-team");
    await user.click(within(dialog).getByRole("button", { name: "Create team" }));

    await waitFor(() => expect(fetchMock.find(`POST ${T}/teams`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/teams`)[0]?.body).toEqual({ slug: "web-team", name: "Web Team", description: "" });
    const sheet = await screen.findByRole("dialog", { name: "Web Team" });
    expect(await within(sheet).findByText("No members yet.")).toBeInTheDocument();
  });

  it("shows create errors from the envelope inline", async () => {
    const { user } = open("teams", { [`POST ${T}/teams`]: apiError(409, "conflict", "team platform already exists") });
    await user.click(await screen.findByRole("button", { name: "New team" }));
    const dialog = await screen.findByRole("dialog", { name: "New team" });
    await user.type(within(dialog).getByLabelText("Name"), "Platform");
    await user.click(within(dialog).getByRole("button", { name: "Create team" }));
    expect(await within(dialog).findByText("team platform already exists")).toBeInTheDocument();
  });

  it("manages members and shows grants in the team sheet", async () => {
    const { user, fetchMock } = open("teams", {
      [`PUT ${T}/teams/platform/members/carol`]: (req: { body: unknown }) => ({ account: fx.bob, ...(req.body as object) }),
      [`PUT ${T}/teams/platform/members/bob`]: (req: { body: unknown }) => ({ account: fx.bob, ...(req.body as object) }),
      [`DELETE ${T}/teams/platform/members/bob`]: noContent(),
      [`DELETE ${T}/teams/platform`]: noContent(),
    });
    await user.click(await screen.findByRole("button", { name: "Open team platform" }));
    const sheet = await screen.findByRole("dialog", { name: "Platform" });
    const members = await within(sheet).findByRole("table", { name: "Team members" });
    expect(members).toHaveTextContent("bob");
    expect(within(sheet).getByRole("table", { name: "Team grants" })).toHaveTextContent("acme/web");
    expect(within(sheet).getByRole("link", { name: "acme/web" })).toHaveAttribute("href", "/trees/acme/web/access");

    // Add a member.
    await user.type(within(sheet).getByLabelText("Username"), "Carol");
    await user.click(within(sheet).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/teams/platform/members/carol`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/teams/platform/members/carol`)[0]?.body).toEqual({ role: "member" });

    // Change a member's role.
    await user.click(within(sheet).getByRole("combobox", { name: "Team role of bob" }));
    await user.click(await screen.findByRole("option", { name: "member" }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/teams/platform/members/bob`)[0]?.body).toEqual({ role: "member" }));

    // Remove a member.
    await user.click(within(sheet).getByRole("button", { name: "Remove bob from team" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Remove bob from platform?" });
    await user.click(within(confirm).getByRole("button", { name: "Remove member" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${T}/teams/platform/members/bob`)).toHaveLength(1));

    // Delete the team.
    await user.click(within(sheet).getByRole("button", { name: "Delete team" }));
    const del = await screen.findByRole("alertdialog", { name: "Delete team platform?" });
    await user.click(within(del).getByRole("button", { name: "Delete team" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${T}/teams/platform`)).toHaveLength(1));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Platform" })).not.toBeInTheDocument());
  });
});

describe("tenant roles tab", () => {
  it("renders the read-only built-in matrix", async () => {
    open("roles");
    const matrix = await screen.findByRole("table", { name: "Built-in role permissions" });
    expect(within(matrix).getByText("developer tree:write: allowed")).toBeInTheDocument();
    expect(within(matrix).getByText("viewer tree:write: not allowed")).toBeInTheDocument();
    // Area wildcards count: admin has tree:*.
    expect(within(matrix).getByText("admin tree:admin: allowed")).toBeInTheDocument();
    expect(within(matrix).getByText("developer mr:merge: not allowed")).toBeInTheDocument();
    expect(screen.getByRole("table", { name: "Custom roles" })).toHaveTextContent("release-manager");
  });

  it("creates a custom role from the action checklist", async () => {
    const { user, fetchMock } = open("roles", {
      [`POST ${T}/roles`]: (req: { body: unknown }) => json({ builtin: false, ...(req.body as object) }, 201),
    });
    await user.click(await screen.findByRole("button", { name: "New role" }));
    const dialog = await screen.findByRole("dialog", { name: "New custom role" });
    await user.click(within(dialog).getByRole("button", { name: "Create role" }));
    expect(within(dialog).getByText("Pick at least one action.")).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText("Name"), "owner");
    expect(within(dialog).getByText("That name is a built-in role.")).toBeInTheDocument();
    await user.clear(within(dialog).getByLabelText("Name"));
    await user.type(within(dialog).getByLabelText("Name"), "deployer");
    await user.click(within(dialog).getByRole("checkbox", { name: /^tree:write/ }));
    await user.click(within(dialog).getByRole("checkbox", { name: /^mr mr:\*/i }));
    await user.click(within(dialog).getByRole("button", { name: "Create role" }));

    await waitFor(() => expect(fetchMock.find(`POST ${T}/roles`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/roles`)[0]?.body).toEqual({ name: "deployer", description: "", permissions: ["tree:write", "mr:*"] });
    expect(await screen.findByText("Role created")).toBeInTheDocument();
  });

  it("explains a 409 when deleting an assigned role", async () => {
    const { user } = open("roles", {
      [`DELETE ${T}/roles/release-manager`]: apiError(409, "conflict", "role release-manager is assigned to 2 subjects"),
    });
    await user.click(await screen.findByRole("button", { name: "Delete release-manager" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Delete role release-manager?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete role" }));
    expect(await within(confirm).findByText("role release-manager is assigned to 2 subjects")).toBeInTheDocument();
    expect(within(confirm).getByText(/Reassign the members/)).toBeInTheDocument();
    expect(within(confirm).queryByRole("button", { name: "Delete role" })).not.toBeInTheDocument();
    await user.click(within(confirm).getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("edits a custom role's permissions", async () => {
    const { user, fetchMock } = open("roles", {
      [`PATCH ${T}/roles/release-manager`]: (req: { body: unknown }) => ({ ...ix.roles[5], ...(req.body as object) }),
    });
    await user.click(await screen.findByRole("button", { name: "Edit release-manager" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit role release-manager" });
    expect(within(dialog).getByLabelText("Name")).toBeDisabled();
    expect(within(dialog).getByRole("checkbox", { name: /^mr:merge/ })).toBeChecked();
    await user.click(within(dialog).getByRole("checkbox", { name: /^mr:merge/ }));
    await user.click(within(dialog).getByRole("button", { name: "Save role" }));
    await waitFor(() =>
      expect(fetchMock.find(`PATCH ${T}/roles/release-manager`)[0]?.body).toEqual({
        description: "Merges release branches",
        permissions: ["tree:read"],
      }),
    );
  });
});

describe("tenant policies tab", () => {
  it("lists policies and toggles enabled with a full replace", async () => {
    const { user, fetchMock } = open("policies", {
      [`PUT ${T}/policies/pol-1`]: (req: { body: unknown }) => ({ ...ix.policy, ...(req.body as object), version: 4 }),
    });
    const table = await screen.findByRole("table", { name: "Policies" });
    expect(table).toHaveTextContent("no-friday-deploys");
    expect(table).toHaveTextContent("v3");
    expect(table).toHaveTextContent("1 deny");
    const toggle = within(table).getByRole("switch", { name: "Enable no-friday-deploys" });
    expect(toggle).toBeChecked();
    await user.click(toggle);
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/policies/pol-1`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/policies/pol-1`)[0]?.body).toEqual({
      name: "no-friday-deploys",
      description: "Deny pushes to main on Fridays",
      enabled: false,
      statements: ix.policy.statements,
    });
    expect(await screen.findByText("Policy disabled")).toBeInTheDocument();
  });

  it("builds a statement visually and creates the policy", async () => {
    const { user, fetchMock } = open("policies", {
      [`POST ${T}/policies`]: (req: { body: unknown }) => json({ ...ix.policy, id: "pol-2", ...(req.body as object), version: 1 }, 201),
    });
    await user.click(await screen.findByRole("button", { name: "New policy" }));
    const dialog = await screen.findByRole("dialog", { name: "New policy" });
    expect(await within(dialog).findByText("Statements are valid")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Name"), "guard");

    const st = within(dialog).getByRole("group", { name: "Statement 1" });
    await user.click(within(st).getByRole("radio", { name: "Deny" }));
    await user.click(within(st).getByRole("combobox", { name: "Actions" }));
    await user.click(await screen.findByRole("option", { name: /^tree:write/ }));
    await user.keyboard("{Escape}");
    expect(within(st).getByRole("list", { name: "Selected actions" })).toHaveTextContent("tree:write");
    await user.click(within(st).getByRole("button", { name: "Add wt:tree/acme/*" }));
    await user.type(within(st).getByLabelText("Principals"), "team:platform{Enter}");
    await user.click(within(st).getByRole("button", { name: /Conditions/ }));
    await user.click(within(st).getByRole("switch", { name: "Require MFA" }));
    // Live validation (debounced) sees the edits.
    await waitFor(() =>
      expect(fetchMock.find(`POST ${T}/policies/validate`).at(-1)?.body).toMatchObject({
        statements: [{ effect: "deny", actions: ["tree:write"], conditions: { require_mfa: true } }],
      }),
    );
    await user.click(within(dialog).getByRole("button", { name: "Create policy" }));

    await waitFor(() => expect(fetchMock.find(`POST ${T}/policies`)).toHaveLength(1));
    expect(fetchMock.find(`POST ${T}/policies`)[0]?.body).toEqual({
      name: "guard",
      description: "",
      enabled: true,
      statements: [
        {
          effect: "deny",
          actions: ["tree:write"],
          resources: ["wt:tree/acme/*"],
          principals: ["team:platform"],
          conditions: { require_mfa: true },
        },
      ],
    });
  });

  it("validates JSON mode live and shows per-field errors", async () => {
    const { user, fetchMock } = open("policies", {
      [`POST ${T}/policies/validate`]: (req: { body: unknown }) => {
        const { statements } = req.body as { statements: { actions: string[] }[] };
        return statements[0]?.actions.includes("tree:fly")
          ? { valid: false, errors: [{ statement: 0, field: "actions", message: "unknown action tree:fly" }] }
          : { valid: true, errors: [] };
      },
    });
    await user.click(await screen.findByRole("button", { name: "Edit no-friday-deploys" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit policy no-friday-deploys" });
    await user.click(within(dialog).getByRole("tab", { name: "JSON" }));
    const editor = within(dialog).getByLabelText("Statements (JSON array)");
    expect((editor as HTMLTextAreaElement).value).toContain('"branch:push"');

    await user.clear(editor);
    await user.click(editor);
    await user.paste("[{");
    expect(await within(dialog).findByText("Cannot parse")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Save policy" })).toBeDisabled();

    await user.clear(editor);
    await user.paste(JSON.stringify([{ effect: "allow", actions: ["tree:fly"], resources: ["*"], principals: ["*"] }]));
    const errors = await within(dialog).findByRole("list", { name: "Validation errors" });
    expect(errors).toHaveTextContent("Statement 1 · actions: unknown action tree:fly");
    expect(within(dialog).getByText("1 problem found")).toBeInTheDocument();
    expect(fetchMock.find(`POST ${T}/policies/validate`).at(-1)?.body).toEqual({
      statements: [{ effect: "allow", actions: ["tree:fly"], resources: ["*"], principals: ["*"], conditions: {} }],
    });

    // The visual builder shows the same error next to the field.
    await user.click(within(dialog).getByRole("tab", { name: "Visual" }));
    const st = within(dialog).getByRole("group", { name: "Statement 1" });
    expect(within(st).getByText("unknown action tree:fly")).toBeInTheDocument();
  });
});

describe("tenant service accounts tab", () => {
  it("creates a service account and opens its sheet", async () => {
    const created = { ...ix.serviceAccount, id: "sa-2", name: "deployer", description: "" };
    const { user, fetchMock } = open("service-accounts", {
      [`POST ${T}/service-accounts`]: json(created, 201),
      [`GET ${T}/service-accounts/deployer`]: { service_account: created, keys: null },
    });
    expect(await screen.findByRole("table", { name: "Service accounts" })).toHaveTextContent("ci-bot");
    await user.click(screen.getByRole("button", { name: "New service account" }));
    const dialog = await screen.findByRole("dialog", { name: "New service account" });
    await user.type(within(dialog).getByLabelText("Name"), "Deployer");
    await user.click(within(dialog).getByRole("combobox", { name: "Role" }));
    expect(await screen.findByRole("option", { name: "release-manager" })).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "maintainer" }));
    await user.click(within(dialog).getByRole("button", { name: "Create service account" }));
    await waitFor(() =>
      expect(fetchMock.find(`POST ${T}/service-accounts`)[0]?.body).toEqual({ name: "deployer", description: "", role: "maintainer" }),
    );
    const sheet = await screen.findByRole("dialog", { name: "deployer" });
    expect(await within(sheet).findByText(/No keys/)).toBeInTheDocument();
  });

  it("creates a key and reveals the token once", async () => {
    const { user, fetchMock } = open("service-accounts", {
      [`POST ${T}/service-accounts/ci-bot/keys`]: (req: { body: unknown }) =>
        json({ token: "wts_secret_123", api_token: { ...ix.key, id: "key-2", ...(req.body as object) } }, 201),
    });
    await user.click(await screen.findByRole("button", { name: "Open service account ci-bot" }));
    const sheet = await screen.findByRole("dialog", { name: "ci-bot" });
    expect(await within(sheet).findByRole("table", { name: "Keys" })).toHaveTextContent("github-actions");

    await user.click(within(sheet).getByRole("button", { name: "Create key" }));
    const dialog = await screen.findByRole("dialog", { name: "Create key" });
    await user.type(within(dialog).getByLabelText("Key name"), "deploy");
    await user.click(within(dialog).getByRole("button", { name: "90 days" }));
    await user.click(within(dialog).getByRole("button", { name: "Add scope tree:write" }));
    await user.click(within(dialog).getByRole("button", { name: "Create key" }));

    const reveal = await screen.findByRole("dialog", { name: "Key created" });
    expect(within(reveal).getByDisplayValue("wts_secret_123")).toBeInTheDocument();
    expect(within(reveal).getByText(/only time the key is shown/)).toBeInTheDocument();
    expect(fetchMock.find(`POST ${T}/service-accounts/ci-bot/keys`)[0]?.body).toEqual({
      name: "deploy",
      ttl_hours: 2160,
      scopes: ["tree:write"],
    });
    await user.click(within(reveal).getByRole("button", { name: "Copy key" }));
    expect(await navigator.clipboard.readText()).toBe("wts_secret_123");
    await user.click(within(reveal).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(screen.queryByDisplayValue("wts_secret_123")).not.toBeInTheDocument());
  });

  it("disables the account and revokes a key", async () => {
    const { user, fetchMock } = open("service-accounts", {
      [`PATCH ${T}/service-accounts/ci-bot`]: (req: { body: unknown }) => ({ ...ix.serviceAccount, ...(req.body as object) }),
      [`DELETE ${T}/service-accounts/ci-bot/keys/key-1`]: noContent(),
    });
    await user.click(await screen.findByRole("button", { name: "Open service account ci-bot" }));
    const sheet = await screen.findByRole("dialog", { name: "ci-bot" });
    await user.click(await within(sheet).findByRole("switch", { name: "Disabled" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${T}/service-accounts/ci-bot`)[0]?.body).toEqual({ disabled: true }));

    await user.click(within(sheet).getByRole("button", { name: "Revoke key github-actions" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Revoke key github-actions?" });
    await user.click(within(confirm).getByRole("button", { name: "Revoke key" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${T}/service-accounts/ci-bot/keys/key-1`)).toHaveLength(1));
  });
});

describe("tenant invitations tab", () => {
  it("shows each invitation status", async () => {
    open("invitations");
    const table = await screen.findByRole("table", { name: "Invitations" });
    const row = (text: string) => within(table).getByText(text).closest("tr")!;
    expect(row("dana@example.com")).toHaveTextContent("Pending");
    await waitFor(() => expect(row("dana@example.com")).toHaveTextContent("joins team platform"));
    expect(row("erin")).toHaveTextContent("Accepted");
    expect(row("old@example.com")).toHaveTextContent("Expired");
    expect(row("nope@example.com")).toHaveTextContent("Revoked");
    // Only pending invitations can be revoked.
    expect(within(table).getAllByRole("button", { name: /Revoke invitation/ })).toHaveLength(1);
  });

  it("creates an invitation and shows a copyable link", async () => {
    const { user, fetchMock } = open("invitations", {
      [`POST ${T}/invitations`]: (req: { body: unknown }) =>
        json({ invitation: { ...ix.invitations[0], id: "inv-9", ...(req.body as object) }, code: "wti_abc123" }, 201),
    });
    await user.click(await screen.findByRole("button", { name: "Invite" }));
    const dialog = await screen.findByRole("dialog", { name: "Invite to acme" });
    await user.type(within(dialog).getByLabelText("Email"), "not-an-email");
    await user.click(within(dialog).getByRole("button", { name: "Create invitation" }));
    expect(within(dialog).getByText("Enter an email address.")).toBeInTheDocument();
    await user.clear(within(dialog).getByLabelText("Email"));
    await user.type(within(dialog).getByLabelText("Email"), "zoe@example.com");
    await user.click(within(dialog).getByRole("combobox", { name: "Team (optional)" }));
    await user.click(await screen.findByRole("option", { name: "Platform" }));
    await user.click(within(dialog).getByRole("button", { name: "Create invitation" }));

    const done = await screen.findByRole("dialog", { name: "Invitation created" });
    const link = `${window.location.origin}/admin/invite/wti_abc123`;
    expect(within(done).getByDisplayValue(link)).toBeInTheDocument();
    expect(fetchMock.find(`POST ${T}/invitations`)[0]?.body).toEqual({ email: "zoe@example.com", role: "developer", team: "platform" });
    await user.click(within(done).getByRole("button", { name: "Copy invitation link" }));
    expect(await navigator.clipboard.readText()).toBe(link);
  });

  it("invites by username and revokes a pending invitation", async () => {
    const { user, fetchMock } = open("invitations", {
      [`POST ${T}/invitations`]: json({ invitation: { ...ix.invitations[1], accepted_at: undefined }, code: "wti_x" }, 201),
      [`DELETE ${T}/invitations/inv-1`]: noContent(),
    });
    await user.click(await screen.findByRole("button", { name: "Invite" }));
    const dialog = await screen.findByRole("dialog", { name: "Invite to acme" });
    await user.click(within(dialog).getByRole("radio", { name: "Username" }));
    await user.type(within(dialog).getByLabelText("Username"), "Erin");
    await user.click(within(dialog).getByRole("button", { name: "Create invitation" }));
    await screen.findByRole("dialog", { name: "Invitation created" });
    expect(fetchMock.find(`POST ${T}/invitations`)[0]?.body).toEqual({ username: "erin", role: "developer" });
    await user.click(screen.getByRole("button", { name: "Done" }));

    await user.click(await screen.findByRole("button", { name: "Revoke invitation for dana@example.com" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Revoke invitation?" });
    await user.click(within(confirm).getByRole("button", { name: "Revoke invitation" }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${T}/invitations/inv-1`)).toHaveLength(1));
  });
});
