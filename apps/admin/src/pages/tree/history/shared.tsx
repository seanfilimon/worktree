/** Small building blocks shared by the tree overview / history / compare pages. */
import { Bot, Check, CircleAlert, Copy, GitMerge, Inbox, RotateCw } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { errorMessage, isApiError } from "@/api/client";
import type { Snapshot } from "@/api/types";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { absoluteTime, relativeTime } from "@/lib/format";
import { cn } from "@/lib/utils";

/** `<time>` showing "3 minutes ago" with the absolute time on hover. */
export function RelativeTime({ value, className }: { value: string | undefined; className?: string }) {
  return (
    <time dateTime={value} title={absoluteTime(value)} className={className}>
      {relativeTime(value)}
    </time>
  );
}

/** Up to two initials: "alice" → "AL", "Bob Smith" → "BS". */
function initials(name: string): string {
  const parts = name.trim().split(/[\s._@-]+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0]!.slice(0, 2).toUpperCase();
  return (parts[0]![0]! + parts[1]![0]!).toUpperCase();
}

export function AuthorAvatar({ name, className }: { name: string; className?: string }) {
  return (
    <Avatar size="sm" className={className} aria-hidden="true">
      <AvatarFallback className="bg-secondary font-medium text-secondary-foreground">{initials(name)}</AvatarFallback>
    </Avatar>
  );
}

/** Inline error with an optional retry button. */
export function QueryError({ error, onRetry, className }: { error: unknown; onRetry?: () => void; className?: string }) {
  const title = isApiError(error) && error.status === 403 ? "Access denied" : isApiError(error) && error.status === 404 ? "Not found" : "Could not load data";
  return (
    <Alert className={className}>
      <CircleAlert aria-hidden="true" />
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>
        <p>{errorMessage(error)}</p>
        {onRetry && (
          <Button variant="outline" size="sm" onClick={onRetry} className="mt-1">
            <RotateCw aria-hidden="true" />
            Try again
          </Button>
        )}
      </AlertDescription>
    </Alert>
  );
}

export function EmptyState({ title, children, icon, className }: { title: string; children?: ReactNode; icon?: ReactNode; className?: string }) {
  return (
    <div className={cn("flex flex-col items-center justify-center gap-2 px-6 py-10 text-center", className)}>
      <span className="text-muted-foreground [&_svg]:size-6">{icon ?? <Inbox aria-hidden="true" />}</span>
      <p className="text-sm font-medium">{title}</p>
      {children && <div className="max-w-md text-[13px] text-muted-foreground">{children}</div>}
    </div>
  );
}

export function LoadingRows({ label, rows = 4 }: { label: string; rows?: number }) {
  return (
    <div role="status" aria-label={label} className="flex flex-col gap-2">
      {Array.from({ length: rows }, (_, i) => (
        <Skeleton key={i} className={cn("h-6", i % 3 === 2 ? "w-2/3" : "w-full")} />
      ))}
    </div>
  );
}

/** Icon button copying `value` to the clipboard. */
export function CopyButton({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);

  return (
    <Button
      variant="ghost"
      size="icon-xs"
      aria-label={copied ? "Copied" : label}
      title={label}
      onClick={() => {
        void navigator.clipboard?.writeText(value).then(() => setCopied(true));
      }}
    >
      {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
    </Button>
  );
}

/** "auto" (daemon-generated) and "merge" (2+ parents) markers. */
export function SnapshotBadges({ snapshot }: { snapshot: Snapshot }) {
  const merge = (snapshot.parents?.length ?? 0) > 1;
  return (
    <>
      {snapshot.auto_generated && (
        <Badge variant="outline" title="Created automatically by the background daemon">
          <Bot aria-hidden="true" />
          auto
        </Badge>
      )}
      {merge && (
        <Badge variant="secondary">
          <GitMerge aria-hidden="true" />
          merge
        </Badge>
      )}
    </>
  );
}
