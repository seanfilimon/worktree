import type { StreamStatus } from "@/api/events";
import { cn } from "@/lib/cn";

const LABEL: Record<StreamStatus, string> = {
  connecting: "Connecting",
  open: "Live",
  reconnecting: "Reconnecting",
  closed: "Offline",
};

/** Connection state of an SSE subscription. */
export function LiveIndicator({ status }: { status: StreamStatus }) {
  return (
    <span role="status" className="inline-flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
      <span
        aria-hidden="true"
        className={cn(
          "size-2 rounded-full",
          status === "open" ? "bg-primary" : status === "closed" ? "bg-subtle" : "animate-pulse bg-warning motion-reduce:animate-none",
        )}
      />
      {LABEL[status]}
    </span>
  );
}
