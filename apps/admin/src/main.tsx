import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router";
import { ApiClient } from "@/api/client";
import { createApi } from "@/api/endpoints";
import { createPersistentValue, REFRESH_STORAGE_KEY, TOKEN_STORAGE_KEY } from "@/api/storage";
import { AppProviders } from "@/app/AppProviders";
import { createQueryClient } from "@/app/queryClient";
import { preloadRoutes, routes } from "@/app/routes";
import { createToastStore } from "@/toast/store";
import "./index.css";

const toasts = createToastStore();
const api = createApi(
  new ApiClient({
    baseUrl: import.meta.env.VITE_WT_SERVER_URL ?? "",
    tokenStore: createPersistentValue(TOKEN_STORAGE_KEY),
    refreshStore: createPersistentValue(REFRESH_STORAGE_KEY), // v0.8 device-auth (§16.6)
  }),
);
const queryClient = createQueryClient({ onMutationError: (err) => toasts.error(err) });
// BASE_URL is "/admin/" in production builds (served by the Go server).
const router = createBrowserRouter(routes, { basename: import.meta.env.BASE_URL.replace(/\/$/, "") || "/" });

const root = document.getElementById("root");
if (!root) throw new Error("#root element missing");

createRoot(root).render(
  <StrictMode>
    <AppProviders api={api} queryClient={queryClient} toasts={toasts}>
      <RouterProvider router={router} />
    </AppProviders>
  </StrictMode>,
);

// Fetch the lazily split pages in the background once the first screen is
// up, so navigating never waits for a chunk (the initial load stays small).
const prefetch = () => {
  void preloadRoutes().catch(() => {
    // A failed prefetch is retried by the router when the page is visited.
  });
};
if ("requestIdleCallback" in window) window.requestIdleCallback(prefetch, { timeout: 5000 });
else setTimeout(prefetch, 1500);
