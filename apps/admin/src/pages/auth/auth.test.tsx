import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TOKEN_STORAGE_KEY } from "@/api/storage";
import * as fx from "@/test/fixtures";
import { apiError, json } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";

const dashboardApi = {
  "GET /api/v1/admin/stats": fx.stats,
  "GET /api/v1/audit": [],
};

describe("login flow", () => {
  it("redirects unauthenticated visitors to the sign-in page", async () => {
    const { router } = renderApp({ route: "/trees", signedIn: false });
    expect(await screen.findByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/login");
    expect(screen.getByRole("link", { name: "Create one" })).toHaveAttribute("href", "/register");
  });

  it("shows an error for invalid credentials", async () => {
    const { user } = renderApp({
      route: "/login",
      signedIn: false,
      api: { "POST /api/v1/auth/login": apiError(401, "unauthenticated", "invalid credentials") },
    });
    await user.type(await screen.findByLabelText("Username"), "alice");
    await user.type(screen.getByLabelText("Password"), "wrong");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Invalid username or password");
    expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBeNull();
  });

  it("signs in, stores the token and returns to the requested page", async () => {
    const { user, router, fetchMock } = renderApp({
      route: "/settings",
      signedIn: false,
      api: {
        "POST /api/v1/auth/login": (req) => {
          expect(req.body).toEqual({ username: "alice", password: "s3cret-pass" });
          return json({ token: "new-token", account: fx.account });
        },
        "GET /api/v1/auth/tokens": [],
      },
    });
    await user.type(await screen.findByLabelText("Username"), "alice");
    await user.type(screen.getByLabelText("Password"), "s3cret-pass");
    await user.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("heading", { name: "Settings", level: 1 })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/settings");
    expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBe("new-token");
    expect(fetchMock.find("GET /api/v1/auth/me").at(-1)?.headers.get("Authorization")).toBe("Bearer new-token");
  });

  it("hides the register link when the server disables registration", async () => {
    renderApp({
      route: "/login",
      signedIn: false,
      api: { "GET /healthz": { status: "ok", version: "2", uptime_seconds: 1, allow_registration: false } },
    });
    await screen.findByRole("heading", { name: "Sign in" });
    await waitFor(() => expect(screen.queryByRole("link", { name: "Create one" })).not.toBeInTheDocument());
  });

  it("sends the user back to sign-in when the session expires", async () => {
    const { router } = renderApp({
      route: "/",
      api: { ...dashboardApi, "GET /api/v1/auth/me": apiError(401, "unauthenticated", "token revoked") },
    });
    expect(await screen.findByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/login");
    expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBeNull();
  });

  it("shows the server version and keeps sign-in disabled until both fields are filled", async () => {
    const { user } = renderApp({ route: "/login", signedIn: false });
    expect(await screen.findByText("Server 2.0.0-test")).toBeInTheDocument();
    const submit = screen.getByRole("button", { name: "Sign in" });
    expect(submit).toBeDisabled();
    await user.type(screen.getByLabelText("Username"), "alice");
    expect(submit).toBeDisabled();
    await user.type(screen.getByLabelText("Password"), "x");
    expect(submit).toBeEnabled();
  });

  it("reports an unreachable server", async () => {
    renderApp({ route: "/login", signedIn: false, api: { "GET /healthz": apiError(500, "internal", "down") } });
    expect(await screen.findByText("Server unreachable")).toBeInTheDocument();
  });

  it("validates the registration form before submitting", async () => {
    const { user, fetchMock } = renderApp({ route: "/register", signedIn: false });
    await user.click(await screen.findByRole("button", { name: "Create account" }));
    expect(screen.getByLabelText("Username")).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByLabelText("Username")).toHaveAccessibleDescription(/Lowercase letters/);
    expect(screen.getByLabelText("Email")).toHaveAccessibleDescription("Enter a valid email address.");
    expect(screen.getByLabelText("Password")).toHaveAccessibleDescription("At least 8 characters.");
    expect(fetchMock.find("POST /api/v1/auth/register")).toHaveLength(0);
  });

  it("registers, signs in and opens the dashboard", async () => {
    const { user, router } = renderApp({
      route: "/register",
      signedIn: false,
      api: {
        ...dashboardApi,
        "POST /api/v1/auth/register": (req) => {
          expect(req.body).toEqual({ username: "carol", email: "carol@example.com", password: "long-enough" });
          return json({ token: "carol-token", account: { ...fx.bob, username: "carol" } }, 201);
        },
      },
    });
    await user.type(await screen.findByLabelText("Username"), "Carol");
    await user.type(screen.getByLabelText("Email"), "carol@example.com");
    await user.type(screen.getByLabelText("Password"), "long-enough");
    await user.click(screen.getByRole("button", { name: "Create account" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/"));
    expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBe("carol-token");
  });

  it("explains when registration is disabled", async () => {
    const { user } = renderApp({
      route: "/register",
      signedIn: false,
      api: { "POST /api/v1/auth/register": apiError(403, "forbidden", "registration disabled") },
    });
    await user.type(await screen.findByLabelText("Username"), "carol");
    await user.type(screen.getByLabelText("Email"), "carol@example.com");
    await user.type(screen.getByLabelText("Password"), "long-enough");
    await user.click(screen.getByRole("button", { name: "Create account" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Registration is disabled on this server");
  });
});
