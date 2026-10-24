import type { ReactNode } from "react";
import { errorMessage, isApiError } from "@/api/client";
import { cn } from "@/lib/cn";
import { Button } from "./Button";

export function Skeleton({ className }: { className?: string }) {
  return <div aria-hidden="true" className={cn("animate-pulse bg-muted rounded", className)} />;
}

/** Placeholder rows while a list loads. */
export function SkeletonRows({ rows = 5, label = "Loading" }: { rows?: number; label?: string }) {
  return (
    <div role="status" aria-label={label} className="flex flex-col gap-2 p-4">
      {Array.from({ length: rows }, (_, i) => (
        <Skeleton key={i} className={cn("h-5", i % 3 === 2 ? "w-2/3" : "w-full")} />
      ))}
    </div>
  );
}

export function EmptyState({ title, children, action, className }: { title: string; children?: ReactNode; action?: ReactNode; className?: string }) {
  return (
    <div className={cn("flex flex-col items-center justify-center gap-2 px-6 py-10 text-center", className)}>
      <svg aria-hidden="true" viewBox="0 0 24 24" className="size-8 text-subtle" fill="none" stroke="currentColor" strokeWidth="1.5">
        <path d="M12 3v6m0 0l-4 4m4-4l4 4M5 21h14M8 17h8" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <p className="font-medium">{title}</p>
      {children && <div className="max-w-md text-[13px] text-muted-foreground">{children}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function ErrorState({ error, onRetry, className }: { error: unknown; onRetry?: () => void; className?: string }) {
  const title = isApiError(error) && error.status === 403 ? "Access denied" : isApiError(error) && error.status === 404 ? "Not found" : "Could not load data";
  return (
    <div role="alert" className={cn("flex flex-col items-center gap-2 px-6 py-10 text-center", className)}>
      <p className="font-medium text-danger">{title}</p>
      <p className="max-w-md text-[13px] text-muted-foreground">{errorMessage(error)}</p>
      {onRetry && (
        <Button size="sm" onClick={onRetry} className="mt-1">
          Try again
        </Button>
      )}
    </div>
  );
}

type AlertTone = "danger" | "warning" | "info" | "success";
const ALERT_TONES: Record<AlertTone, string> = {
  danger: "border-danger/40 bg-danger-soft",
  warning: "border-warning/40 bg-warning-soft",
  info: "border-info/40 bg-info-soft",
  success: "border-foreground/40 bg-accent-soft",
};

export function Alert({ tone = "danger", title, children, className }: { tone?: AlertTone; title: string; children?: ReactNode; className?: string }) {
  return (
    <div role={tone === "danger" ? "alert" : "status"} className={cn("rounded-md border px-3 py-2.5", ALERT_TONES[tone], className)}>
      <p className="font-medium">{title}</p>
      {children && <div className="mt-0.5 text-[13px] text-muted-foreground">{children}</div>}
    </div>
  );
}
