import { CircleAlert, Inbox, RefreshCw, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { errorMessage } from "@/api/client";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

interface EmptyStateProps {
  title: string;
  children?: ReactNode;
  icon?: LucideIcon;
  action?: ReactNode;
  className?: string;
}

/** Centered placeholder for lists and panels with nothing to show. */
export function EmptyState({ title, children, icon: Icon = Inbox, action, className }: EmptyStateProps) {
  return (
    <div className={cn("flex flex-col items-center gap-2 px-6 py-10 text-center", className)}>
      <span className="flex size-10 items-center justify-center rounded-full border bg-muted text-muted-foreground">
        <Icon aria-hidden="true" className="size-5" />
      </span>
      <p className="text-sm font-medium">{title}</p>
      {children && <div className="max-w-sm text-[13px] text-muted-foreground">{children}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

/** Inline error with an optional retry. */
export function ErrorState({ error, onRetry, className }: { error: unknown; onRetry?: () => void; className?: string }) {
  return (
    <Alert className={cn("m-4 w-auto", className)}>
      <CircleAlert aria-hidden="true" />
      <AlertTitle>Could not load data</AlertTitle>
      <AlertDescription className="flex flex-wrap items-center justify-between gap-2">
        <span>{errorMessage(error)}</span>
        {onRetry && (
          <Button size="xs" variant="outline" onClick={onRetry}>
            <RefreshCw aria-hidden="true" />
            Retry
          </Button>
        )}
      </AlertDescription>
    </Alert>
  );
}

/** Skeleton list rows with an accessible loading label. */
export function SkeletonRows({ rows = 4, label }: { rows?: number; label: string }) {
  return (
    <div role="status" aria-label={label} className="flex flex-col gap-3 p-4">
      {Array.from({ length: rows }, (_, i) => (
        <Skeleton key={i} className="h-4" style={{ width: `${90 - ((i * 17) % 40)}%` }} />
      ))}
    </div>
  );
}
