import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router";
import { ApiClient } from "@/api/client";
import { createApi } from "@/api/endpoints";
import { createPersistentValue, TOKEN_STORAGE_KEY } from "@/api/storage";
import { AppProviders } from "@/app/AppProviders";
import { createQueryClient } from "@/app/queryClient";
import { routes } from "@/app/routes";
import { createToastStore } from "@/toast/store";
import "./index.css";

const toasts = createToastStore();
const api = createApi(
  new ApiClient({
    baseUrl: import.meta.env.VITE_WT_SERVER_URL ?? "",
    tokenStore: createPersistentValue(TOKEN_STORAGE_KEY),
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
