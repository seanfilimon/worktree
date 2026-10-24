import { useSyncExternalStore } from "react";
import { cn } from "@/lib/cn";
import { useToast, type ToastKind } from "./store";

const KIND_STYLES: Record<ToastKind, string> = {
  error: "border-danger/40 before:bg-danger",
  success: "border-accent/40 before:bg-accent",
  info: "border-info/40 before:bg-info",
};

export function Toaster() {
  const store = useToast();
  const toasts = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);

  return (
    <div
      aria-live="polite"
      aria-label="Notifications"
      role="region"
      className="pointer-events-none fixed inset-x-4 bottom-4 z-50 flex flex-col items-end gap-2 sm:left-auto sm:w-96"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          role={t.kind === "error" ? "alert" : "status"}
          className={cn(
            "pointer-events-auto relative w-full overflow-hidden rounded-md border bg-surface py-3 pr-10 pl-4 shadow-card",
            "before:absolute before:inset-y-0 before:left-0 before:w-1",
            KIND_STYLES[t.kind],
          )}
        >
          <p className="font-medium">{t.title}</p>
          {t.message && <p className="mt-0.5 text-[13px] break-words text-muted">{t.message}</p>}
          <button
            type="button"
            onClick={() => store.dismiss(t.id)}
            className="absolute top-2 right-2 rounded p-1 text-subtle hover:bg-surface-2 hover:text-fg"
            aria-label="Dismiss notification"
          >
            <svg aria-hidden="true" viewBox="0 0 16 16" className="size-3.5" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M4 4l8 8M12 4l-8 8" />
            </svg>
          </button>
        </div>
      ))}
    </div>
  );
}
