import { CircleAlert, RotateCw, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { errorMessage } from "@/api/client";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { absoluteTime, relativeTime } from "@/lib/format";
import { cn } from "@/lib/utils";

export function EmptyState({
  icon: Icon,
  title,
  children,
  action,
  className,
}: {
  icon?: LucideIcon;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col items-center justify-center gap-2 px-6 py-10 text-center", className)}>
      {Icon && (
        <span className="mb-1 flex size-10 items-center justify-center rounded-full border bg-muted">
          <Icon aria-hidden="true" className="size-5 text-muted-foreground" />
        </span>
      )}
      <p className="text-sm font-semibold">{title}</p>
      {children && <div className="max-w-md text-sm text-muted-foreground">{children}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function ErrorState({ error, onRetry, className }: { error: unknown; onRetry?: () => void; className?: string }) {
  return (
    <div role="alert" className={cn("flex flex-col items-center gap-2 px-6 py-10 text-center", className)}>
      <CircleAlert aria-hidden="true" className="size-5" />
      <p className="text-sm font-semibold">Could not load</p>
      <p className="max-w-md text-sm text-muted-foreground">{errorMessage(error)}</p>
      {onRetry && (
        <Button variant="outline" size="sm" onClick={onRetry} className="mt-1">
          <RotateCw aria-hidden="true" />
          Retry
        </Button>
      )}
    </div>
  );
}

export function LoadingRows({ rows = 5, label, className }: { rows?: number; label: string; className?: string }) {
  return (
    <div role="status" aria-label={label} className={cn("flex flex-col gap-2 p-4", className)}>
      {Array.from({ length: rows }, (_, i) => (
        <Skeleton key={i} className={cn("h-5", i % 3 === 2 ? "w-2/3" : "w-full")} />
      ))}
    </div>
  );
}

/** "3 minutes ago" with the absolute time as a tooltip. */
export function RelativeTime({ value, className }: { value: string | undefined; className?: string }) {
  return (
    <time dateTime={value} title={absoluteTime(value)} className={className}>
      {relativeTime(value)}
    </time>
  );
}
