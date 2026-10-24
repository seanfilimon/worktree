import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { ApiContext } from "@/api/context";
import type { Api } from "@/api/endpoints";
import { AuthProvider } from "@/auth/AuthProvider";
import { ThemeProvider } from "@/theme/ThemeProvider";
import { ToastContext, type ToastStore } from "@/toast/store";
import { Toaster } from "@/toast/Toaster";

interface AppProvidersProps {
  api: Api;
  queryClient: QueryClient;
  toasts: ToastStore;
  children: ReactNode;
}

export function AppProviders({ api, queryClient, toasts, children }: AppProvidersProps) {
  return (
    <ThemeProvider>
      <ToastContext.Provider value={toasts}>
        <ApiContext.Provider value={api}>
          <QueryClientProvider client={queryClient}>
            <AuthProvider>
              {children}
              <Toaster />
            </AuthProvider>
          </QueryClientProvider>
        </ApiContext.Provider>
      </ToastContext.Provider>
    </ThemeProvider>
  );
}
