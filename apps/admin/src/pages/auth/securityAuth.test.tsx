import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { TOKEN_STORAGE_KEY } from "@/api/storage";
import { readSsoHash } from "@/auth/ssoHash";
import * as fx from "@/test/fixtures";
import * as sx from "@/test/fixtures.security";
import { apiError, json, noContent } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const dashboardApi = {
  "GET /api/v1/admin/stats": fx.stats,
  "GET /api/v1/audit": [],
};

const challenge = { mfa_required: true, mfa_token: "mfa-tok", account: { username: "alice" } };

async function signInWithPassword(user: ReturnType<typeof renderApp>["user"]) {
  await user.type(await screen.findByLabelText("Username"), "alice");
  await user.type(screen.getByLabelText("Password"), "s3cret-pass");
  await user.click(screen.getByRole("button", { name: "Sign in" }));
}

afterEach(() => {
  window.history.replaceState(null, "", "/");
});

describe("login with MFA", () => {
  it("asks for an authenticator code and completes the sign-in", async () => {
    const { user, router, fetchMock } = renderApp({
      route: "/",
      signedIn: false,
      api: {
        ...dashboardApi,
        "POST /api/v1/auth/login": challenge,
        "POST /api/v1/auth/login/mfa": json({ token: "mfa-session", account: fx.account }),
      },
    });
    await signInWithPassword(user);

    expect(await screen.findByRole("heading", { name: "Two-factor authentication" })).toBeInTheDocument();
    expect(screen.getByText("alice")).toBeInTheDocument();
    expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBeNull();

    const field = screen.getByLabelText("Authentication code");
    await user.type(field, "12");
    await user.click(screen.getByRole("button", { name: "Verify" }));
    expect(field).toHaveAccessibleDescription("Enter the 6-digit code from your authenticator app.");
    expect(fetchMock.find("POST /api/v1/auth/login/mfa")).toHaveLength(0);

    await user.clear(field);
    await user.type(field, "123 456");
    await user.click(screen.getByRole("button", { name: "Verify" }));
    await waitFor(() => expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBe("mfa-session"));
    expect(fetchMock.find("POST /api/v1/auth/login/mfa")[0]?.body).toEqual({ mfa_token: "mfa-tok", code: "123456" });
    expect(router.state.location.pathname).toBe("/");
  });

  it("accepts a recovery code instead and reports rejected codes", async () => {
    const { user, fetchMock } = renderApp({
      route: "/login",
      signedIn: false,
      api: {
        ...dashboardApi,
        "POST /api/v1/auth/login": challenge,
        "POST /api/v1/auth/login/mfa": apiError(401, "unauthenticated", "invalid code"),
      },
    });
    await signInWithPassword(user);
    await user.click(await screen.findByRole("button", { name: "Use a recovery code" }));
    const field = screen.getByLabelText("Recovery code");
    await user.type(field, "ABCD1234");
    await user.click(screen.getByRole("button", { name: "Verify" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("That code did not work");
    expect(fetchMock.find("POST /api/v1/auth/login/mfa")[0]?.body).toEqual({ mfa_token: "mfa-tok", code: "abcd-1234" });

    fetchMock.on("POST /api/v1/auth/login/mfa", json({ token: "rec-session", account: fx.account }));
    await user.click(screen.getByRole("button", { name: "Verify" }));
    await waitFor(() => expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBe("rec-session"));
  });

  it("goes back to the password step", async () => {
    const { user } = renderApp({ route: "/login", signedIn: false, api: { "POST /api/v1/auth/login": challenge } });
    await signInWithPassword(user);
    await user.click(await screen.findByRole("button", { name: "Back" }));
    expect(await screen.findByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toHaveValue("");
  });
});

describe("single sign-on", () => {
  it("shows the SSO button only when /auth/providers enables it, keeping the return path", async () => {
    renderApp({ route: "/trees", signedIn: false, api: { "GET /api/v1/auth/providers": sx.providersSso } });
    const link = await screen.findByRole("link", { name: "Sign in with Okta" });
    expect(link).toHaveAttribute("href", "/api/v1/auth/sso/start?redirect=%2Ftrees");
  });

  it("hides the SSO button when disabled and follows the providers' registration flag", async () => {
    renderApp({
      route: "/login",
      signedIn: false,
      api: { "GET /api/v1/auth/providers": { ...sx.providers, registration: false } },
    });
    await screen.findByRole("heading", { name: "Sign in" });
    await waitFor(() => expect(screen.queryByRole("link", { name: "Create one" })).not.toBeInTheDocument());
    expect(screen.queryByRole("link", { name: /Sign in with/ })).not.toBeInTheDocument();
  });

  it("consumes #sso_token on load, stores it and strips it from the URL", async () => {
    window.history.replaceState(null, "", "/#sso_token=sso-tok");
    const { fetchMock } = renderApp({ route: "/", signedIn: false, api: dashboardApi });
    await waitFor(() => expect(fetchMock.find("GET /api/v1/auth/me").length).toBeGreaterThan(0));
    expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBe("sso-tok");
    expect(fetchMock.find("GET /api/v1/auth/me")[0]?.headers.get("Authorization")).toBe("Bearer sso-tok");
    await waitFor(() => expect(window.location.hash).toBe(""));
  });

  it("shows #sso_error on the sign-in page", async () => {
    window.history.replaceState(null, "", "/login#sso_error=Email%20domain%20not%20allowed");
    renderApp({ route: "/login", signedIn: false });
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Single sign-on failed");
    expect(alert).toHaveTextContent("Email domain not allowed");
    await waitFor(() => expect(window.location.hash).toBe(""));
  });

  it("parses the fragment defensively", () => {
    expect(readSsoHash("")).toEqual({ token: null, error: null });
    expect(readSsoHash("#section")).toEqual({ token: null, error: null });
    expect(readSsoHash("#sso_token=a%2Bb")).toEqual({ token: "a+b", error: null });
    expect(readSsoHash("#sso_error=")).toEqual({ token: null, error: "Single sign-on failed." });
  });
});

describe("invitation page", () => {
  const preview = { "GET /api/v1/invitations/wti_abc": sx.invitation };

  it("previews and accepts an invitation when signed in", async () => {
    const { user, router, fetchMock } = renderApp({
      route: "/invite/wti_abc",
      api: {
        ...preview,
        "POST /api/v1/invitations/wti_abc/accept": {
          ...sx.invitation.invitation,
          accepted_at: new Date().toISOString(),
        },
      },
    });
    expect(await screen.findByRole("heading", { name: "Join Acme Corp" })).toBeInTheDocument();
    const details = screen.getByLabelText("Invitation details");
    expect(details).toHaveTextContent("acme");
    expect(details).toHaveTextContent("developer");
    expect(details).toHaveTextContent("alice@example.com");
    expect(await screen.findByText(/Signed in as/)).toHaveTextContent("Signed in as alice.");

    await user.click(screen.getByRole("button", { name: "Accept invitation" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/tenants/acme"));
    expect(fetchMock.find("POST /api/v1/invitations/wti_abc/accept")).toHaveLength(1);
    expect(await screen.findByText("Invitation accepted")).toBeInTheDocument();
  });

  it("sends signed-out visitors to sign in and brings them back to accept", async () => {
    const { user, router, fetchMock } = renderApp({
      route: "/invite/wti_abc",
      signedIn: false,
      api: {
        ...preview,
        "POST /api/v1/auth/login": json({ token: "new-token", account: fx.account }),
        "POST /api/v1/invitations/wti_abc/accept": sx.invitation.invitation,
      },
    });
    expect(screen.queryByRole("button", { name: "Accept invitation" })).not.toBeInTheDocument();
    expect(await screen.findByRole("link", { name: "Create an account" })).toHaveAttribute("href", "/register");
    await user.click(screen.getByRole("link", { name: "Sign in to accept" }));
    expect(router.state.location.pathname).toBe("/login");

    await signInWithPassword(user);
    await waitFor(() => expect(router.state.location.pathname).toBe("/invite/wti_abc"));
    await user.click(await screen.findByRole("button", { name: "Accept invitation" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/tenants/acme"));
    expect(fetchMock.find("POST /api/v1/invitations/wti_abc/accept")[0]?.headers.get("Authorization")).toBe(
      "Bearer new-token",
    );
  });

  it("blocks accepting an invitation addressed to someone else", async () => {
    renderApp({
      route: "/invite/wti_abc",
      api: {
        "GET /api/v1/invitations/wti_abc": {
          ...sx.invitation,
          invitation: { ...sx.invitation.invitation, email: "carol@example.com" },
        },
      },
    });
    expect(await screen.findByText("This invitation is for carol@example.com")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Accept invitation" })).toBeDisabled();
  });

  it("explains revoked, expired and unknown invitations", async () => {
    const revoked = { ...sx.invitation, invitation: { ...sx.invitation.invitation, revoked: true } };
    const first = renderApp({
      route: "/invite/wti_abc",
      signedIn: false,
      api: { "GET /api/v1/invitations/wti_abc": revoked },
    });
    expect(await screen.findByText("Invitation revoked")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Sign in to accept" })).not.toBeInTheDocument();
    first.unmount();

    const expired = {
      ...sx.invitation,
      invitation: { ...sx.invitation.invitation, expires_at: "2025-01-01T00:00:00Z" },
    };
    const second = renderApp({
      route: "/invite/wti_abc",
      signedIn: false,
      api: { "GET /api/v1/invitations/wti_abc": expired },
    });
    expect(await screen.findByText("Invitation expired")).toBeInTheDocument();
    second.unmount();

    renderApp({ route: "/invite/wti_nope", signedIn: false });
    expect(await screen.findByRole("heading", { name: "Invitation not found" })).toBeInTheDocument();
  });
});

describe("reset password page", () => {
  it("prefills the code from the link and sets the new password", async () => {
    const { user, fetchMock } = renderApp({
      route: "/reset-password?code=wtr_abc123",
      signedIn: false,
      api: { "POST /api/v1/auth/password-reset": noContent() },
    });
    const form = await screen.findByRole("form", { name: "Reset password" });
    expect(within(form).getByLabelText("Reset code")).toHaveValue("wtr_abc123");

    await user.type(within(form).getByLabelText("New password"), "brand-new-pass");
    await user.type(within(form).getByLabelText("Confirm new password"), "brand-new-pas");
    await user.click(within(form).getByRole("button", { name: "Set new password" }));
    expect(within(form).getByText("Passwords do not match.")).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/auth/password-reset")).toHaveLength(0);

    await user.type(within(form).getByLabelText("Confirm new password"), "s");
    await user.click(within(form).getByRole("button", { name: "Set new password" }));
    expect(await screen.findByRole("heading", { name: "Password updated" })).toBeInTheDocument();
    expect(fetchMock.find("POST /api/v1/auth/password-reset")[0]?.body).toEqual({
      code: "wtr_abc123",
      new_password: "brand-new-pass",
    });
    expect(screen.getByRole("link", { name: "Go to sign in" })).toHaveAttribute("href", "/login");
  });

  it("validates the code and reports a rejected one", async () => {
    const { user } = renderApp({
      route: "/reset-password",
      signedIn: false,
      api: { "POST /api/v1/auth/password-reset": apiError(400, "bad_request", "invalid or expired code") },
    });
    const form = await screen.findByRole("form", { name: "Reset password" });
    await user.click(within(form).getByRole("button", { name: "Set new password" }));
    expect(within(form).getByLabelText("Reset code")).toHaveAccessibleDescription(/starts with wtr_/);

    await user.type(within(form).getByLabelText("Reset code"), "wtr_old");
    await user.type(within(form).getByLabelText("New password"), "brand-new-pass");
    await user.type(within(form).getByLabelText("Confirm new password"), "brand-new-pass");
    await user.click(within(form).getByRole("button", { name: "Set new password" }));
    expect(await within(form).findByRole("alert")).toHaveTextContent("This reset code is invalid or has expired");
  });
});
