import { createContext, useContext } from "react";
import { errorMessage } from "@/api/client";

export type ToastKind = "error" | "success" | "info";

export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  message?: string;
}

export interface ToastStore {
  subscribe: (listener: () => void) => () => void;
  getSnapshot: () => readonly Toast[];
  push: (t: Omit<Toast, "id">) => number;
  dismiss: (id: number) => void;
  error: (err: unknown, title?: string) => number;
  success: (title: string, message?: string) => number;
}

const MAX_TOASTS = 5;

/**
 * Framework-independent toast queue, so non-React code (e.g. the query
 * client's global mutation error handler) can raise toasts too.
 */
export function createToastStore(autoDismissMs = 6000): ToastStore {
  let toasts: readonly Toast[] = [];
  let nextId = 1;
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach((l) => l());

  const store: ToastStore = {
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => toasts,
    push(t) {
      const id = nextId++;
      toasts = [...toasts, { ...t, id }].slice(-MAX_TOASTS);
      emit();
      if (autoDismissMs > 0) setTimeout(() => store.dismiss(id), autoDismissMs);
      return id;
    },
    dismiss(id) {
      const next = toasts.filter((t) => t.id !== id);
      if (next.length !== toasts.length) {
        toasts = next;
        emit();
      }
    },
    error: (err, title = "Request failed") => store.push({ kind: "error", title, message: errorMessage(err) }),
    success: (title, message) => store.push({ kind: "success", title, message }),
  };
  return store;
}

export const ToastContext = createContext<ToastStore | null>(null);

export function useToast(): ToastStore {
  const store = useContext(ToastContext);
  if (!store) throw new Error("useToast must be used inside <AppProviders>");
  return store;
}
