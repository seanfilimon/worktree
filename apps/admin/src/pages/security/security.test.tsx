import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SsoConfigUpdate } from "@/api/security";
import * as fx from "@/test/fixtures";
import * as sx from "@/test/fixtures.security";
import { json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";
import { normalizeRecoveryCode } from "./codes";
import { normalizeScopes } from "./scopes";
import { describeUserAgent } from "./userAgent";

const settings = {
  "GET /api/v1/auth/tokens": [],
  "GET /api/v1/auth/sessions": sx.sessions,
  "GET /api/v1/auth/mfa": sx.mfaOff,
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe("two-factor authentication", () => {
  it("enrolls with a QR code and manual secret, confirms and shows recovery codes", async () => {
    let state = sx.mfaOff;
    const createObjectURL = vi.fn(() => "blob:codes");
    Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: {
        ...settings,
        "GET /api/v1/auth/mfa": () => json(state),
        "POST /api/v1/auth/mfa/totp/enroll": sx.enrollment,
        "POST /api/v1/auth/mfa/totp/confirm": () => {
          state = sx.mfaOn;
          return json({ recovery_codes: sx.recoveryCodes });
        },
      },
    });
    await user.click(await screen.findByRole("button", { name: "Set up authenticator" }));
    const dialog = await screen.findByRole("dialog", { name: "Set up an authenticator app" });
    expect(await within(dialog).findByRole("img", { name: "QR code for your authenticator app" })).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Secret key")).toHaveTextContent("JBSW Y3DP EHPK 3PXP");
    expect(fetchMock.find("POST /api/v1/auth/mfa/totp/enroll")).toHaveLength(1);

    await user.click(within(dialog).getByRole("button", { name: "Copy secret" }));
    expect(await navigator.clipboard.readText()).toBe(sx.enrollment.secret);

    await user.click(within(dialog).getByRole("button", { name: "Verify and enable" }));
    expect(within(dialog).getByText("Enter the 6-digit code from your authenticator app.")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Verification code"), "654 321");
    await user.click(within(dialog).getByRole("button", { name: "Verify and enable" }));

    const done = await screen.findByRole("dialog", { name: "Two-factor authentication enabled" });
    expect(fetchMock.find("POST /api/v1/auth/mfa/totp/confirm")[0]?.body).toEqual({ code: "654321" });
    const list = within(done).getByRole("list", { name: "Recovery codes" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(10);

    await user.click(within(done).getByRole("button", { name: "Copy recovery codes" }));
    expect(await navigator.clipboard.readText()).toBe(sx.recoveryCodes.join("\n"));
    await user.click(within(done).getByRole("button", { name: "Download" }));
    expect(createObjectURL).toHaveBeenCalledTimes(1);
    expect(click).toHaveBeenCalledTimes(1);

    await user.click(within(done).getByRole("button", { name: "Done" }));
    expect(await screen.findByRole("button", { name: "Disable two-factor" })).toBeInTheDocument();
    expect(screen.getByText("Recovery codes left")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Set up authenticator" })).not.toBeInTheDocument();
  });

  it("disables two-factor with the current password", async () => {
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: { ...settings, "GET /api/v1/auth/mfa": sx.mfaOn, "DELETE /api/v1/auth/mfa/totp": noContent() },
    });
    await user.click(await screen.findByRole("button", { name: "Disable two-factor" }));
    const dialog = await screen.findByRole("dialog", { name: "Disable two-factor authentication?" });
    await user.click(within(dialog).getByRole("button", { name: "Disable" }));
    expect(within(dialog).getByText("Enter your password.")).toBeInTheDocument();
    expect(fetchMock.find("DELETE /api/v1/auth/mfa/totp")).toHaveLength(0);

    await user.type(within(dialog).getByLabelText("Current password"), "s3cret-pass");
    await user.click(within(dialog).getByRole("button", { name: "Disable" }));
    expect(await screen.findByText("Two-factor authentication disabled")).toBeInTheDocument();
    expect(fetchMock.find("DELETE /api/v1/auth/mfa/totp")[0]?.body).toEqual({ password: "s3cret-pass" });
  });

  it("regenerates recovery codes with an authenticator code", async () => {
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: {
        ...settings,
        "GET /api/v1/auth/mfa": sx.mfaOn,
        "POST /api/v1/auth/mfa/recovery-codes": { recovery_codes: sx.recoveryCodes.slice(0, 4) },
      },
    });
    await user.click(await screen.findByRole("button", { name: "Regenerate recovery codes" }));
    const dialog = await screen.findByRole("dialog", { name: "Regenerate recovery codes" });
    await user.type(within(dialog).getByLabelText("Authentication code"), "111222");
    await user.click(within(dialog).getByRole("button", { name: "Generate new codes" }));
    const done = await screen.findByRole("dialog", { name: "New recovery codes" });
    expect(within(done).getAllByRole("listitem")).toHaveLength(4);
    expect(fetchMock.find("POST /api/v1/auth/mfa/recovery-codes")[0]?.body).toEqual({ code: "111222" });
  });
});

describe("sessions", () => {
  it("lists sessions, marks the current one and revokes another", async () => {
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: { ...settings, "DELETE /api/v1/auth/sessions/ses-2": noContent() },
    });
    const table = await screen.findByRole("table", { name: "Sessions" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("Chrome on Windows");
    expect(rows[0]).toHaveTextContent("This browser");
    expect(rows[0]).toHaveTextContent("2FA");
    expect(within(rows[0]!).queryByRole("button")).not.toBeInTheDocument();
    expect(rows[2]).toHaveTextContent("wt CLI 0.4.0");

    await user.click(within(table).getByRole("button", { name: "Revoke session: Firefox on macOS" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Revoke this session?" });
    expect(confirm).toHaveTextContent("203.0.113.9");
    await user.click(within(confirm).getByRole("button", { name: "Revoke" }));
    expect(await screen.findByText("Session revoked")).toBeInTheDocument();
    expect(fetchMock.find("DELETE /api/v1/auth/sessions/ses-2")).toHaveLength(1);
  });

  it("signs out every other session", async () => {
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: { ...settings, "POST /api/v1/auth/sessions/revoke-others": { revoked: 2 } },
    });
    await screen.findByRole("table", { name: "Sessions" });
    await user.click(screen.getByRole("button", { name: "Sign out other sessions" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Sign out every other session?" });
    expect(confirm).toHaveTextContent("2 other sessions are signed out");
    await user.click(within(confirm).getByRole("button", { name: "Sign out others" }));
    expect(await screen.findByText("2 other sessions revoked")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/auth/sessions/revoke-others")).toHaveLength(1);
  });

  it("disables revoke-others when this is the only session", async () => {
    renderApp({ route: "/settings", api: { ...settings, "GET /api/v1/auth/sessions": [sx.sessions[0]] } });
    await screen.findByRole("table", { name: "Sessions" });
    expect(screen.getByRole("button", { name: "Sign out other sessions" })).toBeDisabled();
  });
});

describe("scoped API tokens", () => {
  it("creates a token limited to selected scopes", async () => {
    const created = { ...sx.scopedToken, id: "tk-new", name: "ci", scopes: ["mr:*", "tree:read"] };
    const { user, fetchMock } = renderApp({
      route: "/settings",
      api: {
        ...settings,
        "GET /api/v1/auth/tokens": [sx.scopedToken],
        "GET /api/v1/iam/actions": sx.iamActions,
        "POST /api/v1/auth/tokens": json({ token: "wt_scoped", api_token: created }, 201),
      },
    });
    const table = await screen.findByRole("table", { name: "API tokens" });
    expect(within(table).getByText("tree:read")).toBeInTheDocument();
    expect(within(table).getByText("mr:*")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "New token" }));
    const dialog = await screen.findByRole("dialog", { name: "New API token" });
    await user.type(within(dialog).getByLabelText("Token name"), "ci");
    await user.click(within(dialog).getByRole("radio", { name: /Selected scopes/ }));
    await user.click(within(dialog).getByRole("button", { name: "Create token" }));
    expect(within(dialog).getByText("Pick at least one scope.")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/auth/tokens")).toHaveLength(0);

    await user.click(await within(dialog).findByRole("checkbox", { name: "mr:create" }));
    await user.click(within(dialog).getByRole("checkbox", { name: "tree:read" }));
    await user.click(within(dialog).getByRole("checkbox", { name: "mr:*" }));
    expect(within(dialog).getByRole("checkbox", { name: "mr:merge" })).toBeDisabled();
    expect(within(dialog).getByText("Scopes: mr:*, tree:read")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Create token" }));

    await screen.findByRole("dialog", { name: "Token “ci” created" });
    expect(fetchMock.find("POST /api/v1/auth/tokens")[0]?.body).toEqual({
      name: "ci",
      ttl_hours: 720,
      scopes: ["mr:*", "tree:read"],
    });
  });

  it("falls back to the built-in action list on older servers", async () => {
    const { user } = renderApp({ route: "/settings", api: settings });
    await user.click(await screen.findByRole("button", { name: "New token" }));
    const dialog = await screen.findByRole("dialog", { name: "New API token" });
    await user.click(within(dialog).getByRole("radio", { name: /Selected scopes/ }));
    expect(await within(dialog).findByRole("checkbox", { name: "branch:push" })).toBeInTheDocument();
  });
});

describe("admin reset codes", () => {
  it("issues a one-time code and a reset link", async () => {
    const { user, fetchMock } = renderApp({
      route: "/accounts",
      api: {
        "GET /api/v1/admin/accounts": [fx.account, fx.bob],
        "POST /api/v1/admin/accounts/bob/reset-code": {
          code: "wtr_XyZ-1",
          expires_at: new Date(Date.now() + 3_600_000).toISOString(),
        },
      },
    });
    await user.click(await screen.findByRole("button", { name: "Actions for bob" }));
    await user.click(await screen.findByRole("menuitem", { name: "Issue reset code" }));
    const dialog = await screen.findByRole("dialog", { name: "Issue a reset code for bob?" });
    await user.click(within(dialog).getByRole("button", { name: "Issue code" }));

    const done = await screen.findByRole("dialog", { name: "Reset code for bob" });
    expect(fetchMock.find("POST /api/v1/admin/accounts/bob/reset-code")[0]?.body).toEqual({ ttl_hours: 1 });
    expect(within(done).getByLabelText("One-time code")).toHaveValue("wtr_XyZ-1");
    const link = `${window.location.origin}/reset-password?code=wtr_XyZ-1`;
    expect(within(done).getByLabelText("Reset link")).toHaveValue(link);
    await user.click(within(done).getByRole("button", { name: "Copy reset link" }));
    expect(await navigator.clipboard.readText()).toBe(link);
  });

  it("does not offer a reset code for your own account", async () => {
    const { user } = renderApp({ route: "/accounts", api: { "GET /api/v1/admin/accounts": [fx.account, fx.bob] } });
    await user.click(await screen.findByRole("button", { name: "Actions for alice" }));
    expect(await screen.findByRole("menuitem", { name: "Issue reset code" })).toHaveAttribute("aria-disabled", "true");
  });
});

describe("SSO settings", () => {
  it("is listed under Administration for server admins only", async () => {
    renderApp({ route: "/sso", api: { "GET /api/v1/admin/sso": sx.ssoConfig } });
    const nav = await screen.findByRole("navigation", { name: "Main" });
    expect(within(nav).getByRole("link", { name: "Single sign-on" })).toHaveAttribute("aria-current", "page");
  });

  it("keeps the client secret write-only", async () => {
    const bodies: SsoConfigUpdate[] = [];
    const { user } = renderApp({
      route: "/sso",
      api: {
        "GET /api/v1/admin/sso": sx.ssoConfig,
        "PUT /api/v1/admin/sso": (req) => {
          const body = req.body as SsoConfigUpdate;
          bodies.push(body);
          return json({
            ...sx.ssoConfig,
            ...body,
            client_secret: undefined,
            has_client_secret: true,
            updated_at: new Date().toISOString(),
          });
        },
      },
    });
    const form = await screen.findByRole("form", { name: "Single sign-on settings" });
    const secret = within(form).getByLabelText("Client secret");
    expect(secret).toHaveValue("");
    expect(secret).toHaveAttribute("type", "password");
    expect(secret).toHaveAccessibleDescription(/Leave blank to keep it/);
    expect(within(form).getByLabelText("Issuer URL")).toHaveValue("https://acme.okta.com");

    const label = within(form).getByLabelText("Button label");
    await user.clear(label);
    await user.type(label, "Acme SSO");
    await user.click(within(form).getByRole("button", { name: "Save settings" }));
    expect(await screen.findByText("Single sign-on settings saved")).toBeInTheDocument();
    expect(bodies[0]).not.toHaveProperty("client_secret");
    expect(bodies[0]).toMatchObject({
      enabled: true,
      name: "Acme SSO",
      issuer: "https://acme.okta.com",
      client_id: "wt-admin",
      scopes: ["openid", "email", "profile"],
      allowed_domains: ["example.com"],
      auto_create: false,
      link_by_email: false,
      username_claim: "preferred_username",
    });

    const fresh = await screen.findByRole("form", { name: "Single sign-on settings" });
    await user.type(within(fresh).getByLabelText("Client secret"), "n3w-s3cret");
    await user.click(within(fresh).getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(bodies).toHaveLength(2));
    expect(bodies[1]?.client_secret).toBe("n3w-s3cret");
    await waitFor(() =>
      expect(
        within(screen.getByRole("form", { name: "Single sign-on settings" })).getByLabelText("Client secret"),
      ).toHaveValue(""),
    );
  });

  it("starts from defaults when SSO was never configured and validates required fields", async () => {
    const { user, fetchMock } = renderApp({ route: "/sso" });
    const form = await screen.findByRole("form", { name: "Single sign-on settings" });
    expect(within(form).getByLabelText("Redirect URL")).toHaveValue(
      `${window.location.origin}/api/v1/auth/sso/callback`,
    );
    expect(within(form).getByLabelText("Scopes")).toHaveValue("openid email profile");
    await user.click(within(form).getByRole("switch", { name: "Enable single sign-on" }));
    await user.click(within(form).getByRole("button", { name: "Save settings" }));
    expect(within(form).getByLabelText("Client ID")).toHaveAccessibleDescription("Required when SSO is enabled.");
    expect(within(form).getByLabelText("Issuer URL")).toHaveAttribute("aria-invalid", "true");
    expect(fetchMock.find("PUT /api/v1/admin/sso")).toHaveLength(0);
  });
});

describe("security helpers", () => {
  it("normalises scopes, recovery codes and user agents", () => {
    expect(normalizeScopes(["tree:read", "mr:create", "mr:*"])).toEqual(["mr:*", "tree:read"]);
    expect(normalizeScopes(["tree:read", "*"])).toEqual(["*"]);
    expect(normalizeRecoveryCode(" ABCD 1234 ")).toBe("abcd-1234");
    expect(normalizeRecoveryCode("abcd-1234")).toBe("abcd-1234");
    expect(describeUserAgent(undefined)).toBe("Unknown client");
    expect(describeUserAgent("curl/8.4.0")).toBe("curl");
    expect(
      describeUserAgent("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Version/17.0 Mobile Safari/604.1"),
    ).toBe("Safari on iOS");
  });
});
