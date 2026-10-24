import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, RouterProvider } from "react-router";
import { ApiClient } from "@/api/client";
import { createApi } from "@/api/endpoints";
import { createPersistentValue, TOKEN_STORAGE_KEY } from "@/api/storage";
import { AppProviders } from "@/app/AppProviders";
import { createQueryClient } from "@/app/queryClient";
import { routes } from "@/app/routes";
import { createToastStore } from "@/toast/store";
import * as fx from "./fixtures";
import { mockFetch, type MockRoute } from "./mockFetch";

interface RenderAppOptions {
  /** Initial URL. */
  route?: string;
  /** Extra / overriding API routes ("METHOD /path"). */
  api?: Record<string, MockRoute>;
  /** Start signed in (default true). */
  signedIn?: boolean;
}

/** Routes every authenticated page needs. */
export const baseRoutes: Record<string, MockRoute> = {
  "GET /healthz": { status: "ok", version: "2.0.0-test", uptime_seconds: 3700 },
  "GET /api/v1/auth/me": fx.me,
  "GET /api/v1/tenants": [fx.tenant],
};

/** Mounts the full app (real routes and providers) over a mocked fetch. */
export function renderApp({ route = "/", api = {}, signedIn = true }: RenderAppOptions = {}) {
  const fetchMock = mockFetch({ ...baseRoutes, ...api });
  const tokenStore = createPersistentValue(TOKEN_STORAGE_KEY);
  if (signedIn) tokenStore.set("test-token");
  const client = new ApiClient({ tokenStore, fetch: fetchMock.fetch });
  const queryClient = createQueryClient({ retry: false });
  const toasts = createToastStore(0);
  const router = createMemoryRouter(routes, { initialEntries: [route] });
  const user = userEvent.setup();

  const utils = render(
    <AppProviders api={createApi(client)} queryClient={queryClient} toasts={toasts}>
      <RouterProvider router={router} />
    </AppProviders>,
  );
  return { ...utils, user, router, fetchMock, client, queryClient, toasts };
}

/** Mocks for a tree page of acme/web. */
export function treeRoutes(extra: Record<string, MockRoute> = {}): Record<string, MockRoute> {
  const base = "/api/v1/trees/acme/web";
  return {
    [`GET ${base}`]: fx.summary,
    [`GET ${base}/branches`]: fx.branches,
    [`GET ${base}/protections`]: [fx.protection],
    ...extra,
  };
}
