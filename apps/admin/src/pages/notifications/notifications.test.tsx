import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FakeEventSource } from "@/test/fakeEventSource";
import { noContent, type MockRequest } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";
import { reasonLabel, treeOfUrl, type Notification, type NotificationList, type WatchView } from "./api";

const N = "/api/v1/notifications";

function notification(over: Partial<Notification> = {}): Notification {
  return {
    id: "n1",
    account_id: "acct-alice",
    reason: "review_requested",
    event: "mr.review_requested",
    tenant_id: "t-acme",
    tree_id: "tree-web",
    title: "bob requested a review on !3 Add login",
    body: "Reviewers: user:alice",
    url: "/trees/acme/web/merge-requests/3",
    actor: "bob",
    created_at: new Date(Date.now() - 60_000).toISOString(),
    ...over,
  };
}

/** A tiny in-memory inbox behind the notification routes. */
function inbox(initial: Notification[]) {
  let items = [...initial];
  const list = (req: MockRequest): NotificationList => {
    const unreadOnly = req.url.searchParams.get("unread") === "true";
    const limit = Number(req.url.searchParams.get("limit") ?? "50");
    const shown = items.filter((n) => !unreadOnly || !n.read_at).slice(0, limit);
    return { notifications: shown, unread_count: items.filter((n) => !n.read_at).length };
  };
  const routes = {
    [`GET ${N}`]: list,
    [`POST ${N}/read-all`]: () => {
      const updated = items.filter((n) => !n.read_at).length;
      items = items.map((n) => ({ ...n, read_at: n.read_at ?? new Date().toISOString() }));
      return { updated };
    },
    [`GET ${N}/settings`]: { email: true, email_configured: false },
    [`GET ${N}/watches`]: { watches: null },
  } as Record<string, unknown>;
  for (const n of initial) {
    routes[`POST ${N}/${n.id}/read`] = () => {
      items = items.map((x) => (x.id === n.id ? { ...x, read_at: new Date().toISOString() } : x));
      return items.find((x) => x.id === n.id);
    };
    routes[`DELETE ${N}/${n.id}`] = () => {
      items = items.filter((x) => x.id !== n.id);
      return noContent();
    };
  }
  return {
    routes: routes as Record<string, never>,
    add(n: Notification) {
      items = [n, ...items];
    },
  };
}

describe("notification helpers", () => {
  it("labels reasons and finds the tree of a url", () => {
    expect(reasonLabel("review_requested")).toBe("Review requested");
    expect(reasonLabel("something_new")).toBe("something new");
    expect(treeOfUrl("/trees/acme/web/merge-requests/1")).toBe("acme/web");
    expect(treeOfUrl("/notifications")).toBeUndefined();
    expect(treeOfUrl(undefined)).toBeUndefined();
  });
});

describe("notification bell", () => {
  it("shows the unread count, previews and marks all read", async () => {
    const box = inbox([notification(), notification({ id: "n2", reason: "mentioned", title: "carol commented on !3" })]);
    const { user, fetchMock } = renderApp({ route: "/nowhere", api: box.routes });
    const bell = await screen.findByRole("button", { name: "Notifications (2 unread)" });
    expect(screen.getByTestId("notification-count")).toHaveTextContent("2");
    await user.click(bell);
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByText("carol commented on !3")).toBeInTheDocument();
    expect(within(menu).getByText(/Review requested · acme\/web/)).toBeInTheDocument();
    await user.click(within(menu).getByRole("button", { name: "Mark all read" }));
    await waitFor(() => expect(fetchMock.find(`POST ${N}/read-all`)).toHaveLength(1));
    await waitFor(() => expect(screen.queryByTestId("notification-count")).toBeNull());
    await user.keyboard("{Escape}");
    expect(await screen.findByRole("button", { name: "Notifications" })).toBeInTheDocument();
  });

  it("refreshes on notification.created from the shell stream", async () => {
    const box = inbox([]);
    renderApp({ route: "/nowhere", api: box.routes });
    await screen.findByRole("button", { name: "Notifications" });
    box.add(notification({ id: "n9" }));
    const stream = FakeEventSource.instances[0]!;
    act(() => {
      stream.open();
      stream.emit("notification.created", { type: "notification.created", tenant_id: "", at: new Date().toISOString(), payload: notification({ id: "n9" }) });
    });
    expect(await screen.findByRole("button", { name: "Notifications (1 unread)" })).toBeInTheDocument();
  });

  it("is a plain bell when the server has no notifications", async () => {
    renderApp({ route: "/nowhere" });
    expect(await screen.findByRole("button", { name: "Notifications" })).toBeInTheDocument();
  });
});

describe("notifications page", () => {
  it("lists, filters, marks read and removes notifications", async () => {
    const box = inbox([
      notification(),
      notification({ id: "n2", reason: "ci_failed", title: "CI failure: ci/build on !3", url: "/trees/acme/web/merge-requests/3" }),
      notification({ id: "n3", reason: "watching", title: "old news", read_at: new Date().toISOString() }),
    ]);
    const { user, fetchMock } = renderApp({ route: "/notifications", api: box.routes });
    expect(await screen.findByRole("heading", { name: "Notifications", level: 1 })).toBeInTheDocument();
    expect(await screen.findByText("bob requested a review on !3 Add login")).toBeInTheDocument();
    expect(screen.getByText("2 unread")).toBeInTheDocument();
    expect(screen.queryByText("old news")).toBeNull();

    await user.click(screen.getByRole("radio", { name: "All" }));
    expect(await screen.findByText("old news")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: 'Mark "CI failure: ci/build on !3" read' }));
    await waitFor(() => expect(fetchMock.find(`POST ${N}/n2/read`)).toHaveLength(1));
    await user.click(screen.getByRole("button", { name: 'Done with "old news"' }));
    await waitFor(() => expect(fetchMock.find(`DELETE ${N}/n3`)).toHaveLength(1));
    await waitFor(() => expect(screen.queryByText("old news")).toBeNull());

    // Settings: SMTP is not configured on this server.
    expect(screen.getByText(/no SMTP server configured/)).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Email me about things that involve me" })).toBeChecked();
    expect(screen.getByText(/You participate in every tree/)).toBeInTheDocument();
  });

  it("changes a watch level from the watched list", async () => {
    const watches: WatchView[] = [
      { account_id: "a", tenant_id: "t-acme", tree_id: "tree-web", tenant: "acme", tree: "web", level: "watching", email: false, updated_at: new Date().toISOString() },
      { account_id: "a", tenant_id: "t-acme", tenant: "acme", level: "ignoring", email: false, updated_at: new Date().toISOString() },
    ];
    const box = inbox([]);
    const { user, fetchMock } = renderApp({
      route: "/notifications",
      api: {
        ...box.routes,
        [`GET ${N}/watches`]: { watches },
        "PUT /api/v1/trees/acme/web/watch": (req: MockRequest) => ({ ...watches[0], ...(req.body as object) }),
      },
    });
    expect(await screen.findByRole("link", { name: "acme/web" })).toHaveAttribute("href", "/trees/acme/web");
    expect(screen.getByText("every tree")).toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "Email watching notifications for acme/web" }));
    await waitFor(() => expect(fetchMock.find("PUT /api/v1/trees/acme/web/watch")).toHaveLength(1));
    expect(fetchMock.find("PUT /api/v1/trees/acme/web/watch")[0]!.body).toEqual({ level: "watching", email: true });
  });
});

describe("tree watch control", () => {
  it("shows the level and switches to watching", async () => {
    const T = "/api/v1/trees/acme/web";
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/overview",
      api: treeRoutes({
        [`GET ${T}/watch`]: { account_id: "a", tenant_id: "t", tree_id: "tree-web", level: "participating", email: false, updated_at: "0001-01-01T00:00:00Z" },
        [`PUT ${T}/watch`]: (req: MockRequest) => ({ account_id: "a", tenant_id: "t", tree_id: "tree-web", email: false, updated_at: new Date().toISOString(), ...(req.body as object) }),
      }),
    });
    const button = await screen.findByRole("button", { name: "Watch settings: Participating" });
    await user.click(button);
    await user.click(await screen.findByRole("menuitemradio", { name: /Watching/ }));
    await waitFor(() => expect(fetchMock.find(`PUT ${T}/watch`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/watch`)[0]!.body).toEqual({ level: "watching", email: false });
  });

  it("is hidden when the server has no notifications", async () => {
    renderApp({ route: "/trees/acme/web/overview", api: treeRoutes() });
    await screen.findByRole("navigation", { name: "Tree sections" });
    expect(screen.queryByRole("button", { name: /Watch settings/ })).toBeNull();
  });
});
