import { CircleAlert, RefreshCw, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { errorMessage } from "@/api/client";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  className?: string;
}

export function EmptyState({ icon: Icon, title, children, action, className }: EmptyStateProps) {
  return (
    <div className={cn("flex flex-col items-center gap-2 px-6 py-12 text-center", className)}>
      <span className="flex size-10 items-center justify-center rounded-full border bg-muted">
        <Icon aria-hidden="true" className="size-5 text-muted-foreground" />
      </span>
      <p className="font-medium">{title}</p>
      {children && <div className="max-w-sm text-sm text-muted-foreground">{children}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function QueryError({ error, onRetry, title = "Could not load data" }: { error: unknown; onRetry?: () => void; title?: string }) {
  return (
    <div className="p-4">
      <Alert>
        <CircleAlert aria-hidden="true" />
        <AlertTitle>{title}</AlertTitle>
        <AlertDescription>
          <p>{errorMessage(error)}</p>
          {onRetry && (
            <Button variant="outline" size="sm" className="mt-1" onClick={onRetry}>
              <RefreshCw aria-hidden="true" />
              Retry
            </Button>
          )}
        </AlertDescription>
      </Alert>
    </div>
  );
}

/** Placeholder rows while a table loads. */
export function TableSkeleton({ label, rows = 4, columns = 4 }: { label: string; rows?: number; columns?: number }) {
  return (
    <div role="status" aria-label={label} className="flex flex-col gap-3 p-4">
      {Array.from({ length: rows }, (_, r) => (
        <div key={r} className="flex items-center gap-4">
          <Skeleton className="size-8 shrink-0 rounded-full" />
          {Array.from({ length: columns - 1 }, (_, c) => (
            <Skeleton key={c} className={cn("h-4", c === 0 ? "w-1/4" : "flex-1")} />
          ))}
        </div>
      ))}
    </div>
  );
}

/** Inline form-level error inside dialogs (mutations with `meta.inlineError`). */
export function FormError({ error }: { error: unknown }) {
  if (!error) return null;
  return (
    <Alert>
      <CircleAlert aria-hidden="true" />
      <AlertTitle>Request failed</AlertTitle>
      <AlertDescription>{errorMessage(error)}</AlertDescription>
    </Alert>
  );
}
