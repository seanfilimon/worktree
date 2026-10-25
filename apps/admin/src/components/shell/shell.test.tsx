import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TOKEN_STORAGE_KEY } from "@/api/storage";
import { FakeEventSource } from "@/test/fakeEventSource";
import * as fx from "@/test/fixtures";
import { noContent } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";
import { filterCommand } from "./commandFilter";

const auditApi = { "GET /api/v1/audit": [] };

function mainNav() {
  return screen.getByRole("navigation", { name: "Main" });
}

describe("app shell sidebar", () => {
  it("shows administration items to server admins and marks the active page", async () => {
    renderApp({ route: "/audit", api: auditApi });
    await screen.findByRole("heading", { name: "Audit log", level: 1 });
    const nav = mainNav();
    for (const name of ["Dashboard", "Trees", "Tenants", "Accounts", "Audit log", "System", "Settings"]) {
      expect(within(nav).getByRole("link", { name })).toBeInTheDocument();
    }
    expect(within(nav).getByRole("link", { name: "Audit log" })).toHaveAttribute("aria-current", "page");
    expect(within(nav).getByRole("link", { name: "Dashboard" })).not.toHaveAttribute("aria-current");
  });

  it("hides admin-only items from regular members", async () => {
    renderApp({ route: "/audit", api: { ...auditApi, "GET /api/v1/auth/me": fx.memberMe } });
    await screen.findByRole("heading", { name: "Audit log", level: 1 });
    const nav = mainNav();
    expect(within(nav).getByRole("link", { name: "Trees" })).toBeInTheDocument();
    expect(within(nav).getByRole("link", { name: "Audit log" })).toBeInTheDocument();
    expect(within(nav).queryByRole("link", { name: "Accounts" })).not.toBeInTheDocument();
    expect(within(nav).queryByRole("link", { name: "System" })).not.toBeInTheDocument();
  });

  it("collapses to icons with the sidebar trigger", async () => {
    const { user, container } = renderApp({ route: "/audit", api: auditApi });
    await screen.findByRole("heading", { name: "Audit log", level: 1 });
    const sidebar = container.querySelector('[data-slot="sidebar"]');
    expect(sidebar).toHaveAttribute("data-state", "expanded");
    // The header trigger (the rail with the same name is pointer-only).
    const trigger = screen
      .getAllByRole("button", { name: "Toggle Sidebar" })
      .find((b) => b.dataset.sidebar === "trigger");
    await user.click(trigger!);
    expect(sidebar).toHaveAttribute("data-state", "collapsed");
    expect(sidebar).toHaveAttribute("data-collapsible", "icon");
  });
});

describe("app shell header", () => {
  it("shows the page's breadcrumbs in the header bar", async () => {
    renderApp({ route: "/trees/acme/web/overview", api: treeRoutes() });
    const trail = await screen.findByRole("navigation", { name: "Breadcrumb" });
    await waitFor(() => expect(within(trail).getByText("web")).toHaveAttribute("aria-current", "page"));
    expect(within(trail).getByRole("link", { name: "Trees" })).toHaveAttribute("href", "/trees");
    expect(within(trail).getByRole("link", { name: "acme" })).toHaveAttribute("href", "/tenants/acme");
  });

  it("reflects the live event stream status", async () => {
    renderApp({ route: "/nowhere" });
    expect(await screen.findByRole("status", { name: "Event stream: Connecting" })).toBeInTheDocument();
    // The shell's unscoped stream is opened before any page subscribes.
    const stream = FakeEventSource.instances[0]!;
    expect(new URL(stream.url, "http://x").searchParams.get("tenant")).toBeNull();
    act(() => stream.open());
    expect(screen.getByRole("status", { name: "Event stream: Live" })).toBeInTheDocument();
    act(() => stream.fail());
    expect(screen.getByRole("status", { name: "Event stream: Reconnecting" })).toBeInTheDocument();
  });

  it("renders store toasts with Sonner", async () => {
    const { toasts } = renderApp({ route: "/nowhere" });
    await screen.findByRole("heading", { name: "Page not found" });
    act(() => {
      toasts.success("Token created", "Copy it now.");
    });
    expect(await screen.findByText("Token created")).toBeInTheDocument();
    expect(screen.getByText("Copy it now.")).toBeInTheDocument();
    act(() => toasts.dismiss(toasts.getSnapshot()[0]!.id));
    await waitFor(() => expect(screen.queryByText("Token created")).not.toBeInTheDocument());
  });
});

describe("command palette", () => {
  it("opens with Ctrl+K and navigates to a search result", async () => {
    const { user, router, fetchMock } = renderApp({
      route: "/nowhere",
      api: treeRoutes({ "GET /api/v1/search": fx.searchResults }),
    });
    await screen.findByRole("heading", { name: "Page not found" });

    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    const input = within(dialog).getByRole("combobox", { name: "Search" });
    expect(input).toHaveFocus();
    await user.type(input, "web");

    const result = await within(dialog).findByRole("option", { name: /^acme\/web/ });
    expect(fetchMock.find("GET /api/v1/search").at(-1)?.url.searchParams.get("q")).toBe("web");
    expect(within(dialog).getByRole("option", { name: /feature\/login/ })).toBeInTheDocument();
    expect(within(dialog).getByRole("option", { name: /@bob/ })).toBeInTheDocument();

    await user.click(result);
    await waitFor(() => expect(router.state.location.pathname).toBe("/trees/acme/web/overview"));
    expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();
  });

  it("links branch results to the branch history", async () => {
    const { user, router } = renderApp({
      route: "/nowhere",
      api: treeRoutes({ "GET /api/v1/search": fx.searchResults }),
    });
    await screen.findByRole("heading", { name: "Page not found" });
    await user.click(screen.getByRole("button", { name: "Search" }));
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.type(within(dialog).getByRole("combobox"), "login");
    await user.click(await within(dialog).findByRole("option", { name: /feature\/login/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/trees/acme/web/history"));
    expect(router.state.location.search).toBe("?branch=feature%2Flogin");
  });

  it("opens with / and runs navigation commands from the keyboard", async () => {
    const { user, router } = renderApp({ route: "/nowhere", api: auditApi });
    await screen.findByRole("heading", { name: "Page not found" });

    await user.keyboard("/");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("audit");
    await waitFor(() =>
      expect(within(dialog).getByRole("option", { name: /Audit log/ })).toHaveAttribute("aria-selected", "true"),
    );
    expect(within(dialog).queryByRole("option", { name: "Dashboard" })).not.toBeInTheDocument();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/audit"));
  });

  it("hides account results and admin pages from regular members", async () => {
    const { user } = renderApp({
      route: "/nowhere",
      // Even if a server returned accounts, members never see them.
      api: { "GET /api/v1/auth/me": fx.memberMe, "GET /api/v1/search": fx.searchResults },
    });
    await screen.findByRole("heading", { name: "Page not found" });
    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    expect(within(dialog).getByRole("option", { name: "Audit log" })).toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: "Accounts" })).not.toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: "System" })).not.toBeInTheDocument();

    await user.type(within(dialog).getByRole("combobox"), "bob");
    expect(await within(dialog).findByRole("option", { name: /^acme\/web/ })).toBeInTheDocument();
    expect(within(dialog).queryByRole("option", { name: /@bob/ })).not.toBeInTheDocument();
  });

  it("shows search progress and an empty state", async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { user } = renderApp({
      route: "/nowhere",
      api: {
        "GET /api/v1/search": async () => {
          await gate;
          return { tenants: [], trees: [], branches: [], accounts: [] };
        },
      },
    });
    await screen.findByRole("heading", { name: "Page not found" });
    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.type(within(dialog).getByRole("combobox"), "zzz-nothing");
    expect(await within(dialog).findByRole("status")).toHaveTextContent("Searching…");

    act(() => release());
    expect(await within(dialog).findByText("No results for “zzz-nothing”.")).toBeInTheDocument();
    expect(within(dialog).queryByText("Searching…")).not.toBeInTheDocument();
  });
});

describe("filterCommand", () => {
  it("requires every term as a substring and ranks word starts first", () => {
    expect(filterCommand("nav:/accounts", "acc", ["Accounts", "users"])).toBe(1);
    expect(filterCommand("nav:/", "acc", ["Dashboard", "activity"])).toBe(0);
    expect(filterCommand("nav:/audit", "log", ["Audit log"])).toBe(1);
    expect(filterCommand("nav:/trees", "ree", ["Trees"])).toBe(0.5);
    expect(filterCommand("theme:dark", "dark theme", ["Dark theme"])).toBe(1);
    expect(filterCommand("theme:dark", "dark light", ["Dark theme"])).toBe(0);
  });
});

describe("user menu", () => {
  it("switches the theme, toggling the dark class", async () => {
    const { user } = renderApp({ route: "/nowhere" });
    await screen.findByRole("heading", { name: "Page not found" });
    expect(document.documentElement).not.toHaveClass("dark");

    await user.click(screen.getByRole("button", { name: "Account menu for alice" }));
    await user.click(await screen.findByRole("menuitemradio", { name: "Dark" }));
    expect(document.documentElement).toHaveClass("dark");

    await user.click(screen.getByRole("button", { name: "Account menu for alice" }));
    await user.click(await screen.findByRole("menuitemradio", { name: "Light" }));
    expect(document.documentElement).not.toHaveClass("dark");
  });

  it("signs out, revoking the token", async () => {
    const { user, router, fetchMock } = renderApp({
      route: "/nowhere",
      api: { "POST /api/v1/auth/logout": noContent() },
    });
    await screen.findByRole("heading", { name: "Page not found" });

    await user.click(screen.getByRole("button", { name: "Account menu for alice" }));
    await user.click(await screen.findByRole("menuitem", { name: "Sign out" }));

    expect(await screen.findByRole("heading", { name: "Sign in" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/login");
    expect(fetchMock.find("POST /api/v1/auth/logout")).toHaveLength(1);
    expect(localStorage.getItem(TOKEN_STORAGE_KEY)).toBeNull();
  });
});
