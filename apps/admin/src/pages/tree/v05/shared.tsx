/** Components shared by the v0.5 tree screens (§13.12 admin-tree). */
import {
  Ban,
  CircleCheck,
  CircleDashed,
  CircleDot,
  CircleSlash,
  CircleX,
  LoaderCircle,
  PlugZap,
  TriangleAlert,
  type LucideIcon,
} from "lucide-react";
import type { CIState, TodoPriority, TodoStatus } from "@/api/v05tree";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { ciStateLabel, TODO_STATUS_LABEL } from "./util";

/** Shown instead of a feature the server answers `501 not_implemented` (or lacks) for. */
export function FeatureUnavailable({ feature, className }: { feature: string; className?: string }) {
  return (
    <div role="note" className={cn("flex flex-col items-center gap-2 px-6 py-10 text-center", className)}>
      <span className="flex size-10 items-center justify-center rounded-full border bg-muted">
        <PlugZap aria-hidden="true" className="size-5 text-muted-foreground" />
      </span>
      <p className="text-sm font-semibold">Not available on this server</p>
      <p className="max-w-md text-sm text-muted-foreground">{feature} are not enabled on this W0rkTree server.</p>
    </div>
  );
}

/** Compact variant for side panels. */
export function InlineUnavailable({ feature }: { feature: string }) {
  return (
    <p role="note" className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <PlugZap aria-hidden="true" className="size-3.5 shrink-0" />
      {feature}: not available on this server.
    </p>
  );
}

const TODO_ICON: Record<TodoStatus, LucideIcon> = {
  open: CircleDot,
  in_progress: LoaderCircle,
  done: CircleCheck,
  cancelled: CircleSlash,
};

/** Open = outline, in progress = secondary, done = filled, cancelled = struck through. */
export function TodoStatusBadge({ status }: { status: TodoStatus }) {
  const Icon = TODO_ICON[status] ?? CircleDot;
  return (
    <Badge
      variant={status === "done" ? "default" : status === "in_progress" ? "secondary" : "outline"}
      data-status={status}
      className={cn(status === "cancelled" && "text-muted-foreground line-through")}
    >
      <Icon aria-hidden="true" />
      {TODO_STATUS_LABEL[status] ?? status}
    </Badge>
  );
}

const PRIORITY_WEIGHT: Record<TodoPriority, string> = {
  low: "font-normal text-muted-foreground",
  medium: "font-normal",
  high: "font-semibold",
  critical: "font-bold uppercase",
};

export function PriorityBadge({ priority }: { priority: TodoPriority | "" | undefined }) {
  const p = priority || "medium";
  return (
    <Badge variant={p === "critical" ? "default" : "outline"} data-priority={p} className={PRIORITY_WEIGHT[p]}>
      {p}
    </Badge>
  );
}

export function BlockingBadge() {
  return (
    <Badge variant="default" title="Blocks merging into the target branch when its protection requires it">
      <Ban aria-hidden="true" />
      blocking
    </Badge>
  );
}

const CI_ICON: Record<CIState, LucideIcon> = {
  success: CircleCheck,
  failure: CircleX,
  error: TriangleAlert,
  pending: CircleDashed,
};

export function CIStateIcon({ state, className }: { state: CIState | ""; className?: string }) {
  const Icon = state ? CI_ICON[state] : CircleDashed;
  return <Icon aria-hidden="true" className={cn("size-4 shrink-0", state !== "success" && "text-muted-foreground", className)} />;
}

/** Combined CI state: passed = filled, failed/errored = bold outline, pending/none = dimmed outline. */
export function CIStateBadge({ state }: { state: CIState | "" }) {
  return (
    <Badge
      variant={state === "success" ? "default" : "outline"}
      data-ci-state={state || "none"}
      className={cn(
        (state === "failure" || state === "error") && "font-bold",
        (state === "pending" || state === "") && "text-muted-foreground",
      )}
    >
      <CIStateIcon state={state} className="size-3 text-current" />
      {ciStateLabel(state)}
    </Badge>
  );
}
