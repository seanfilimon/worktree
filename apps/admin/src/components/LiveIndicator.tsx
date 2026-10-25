import type { StreamStatus } from "@/api/events";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

const LABEL: Record<StreamStatus, string> = {
  connecting: "Connecting",
  open: "Live",
  reconnecting: "Reconnecting",
  closed: "Offline",
};

const DESCRIPTION: Record<StreamStatus, string> = {
  connecting: "Connecting to the live event stream…",
  open: "Receiving live server events.",
  reconnecting: "Connection lost — reconnecting…",
  closed: "Not connected to the event stream.",
};

/** Monochrome dot: filled = live, hollow pulsing = (re)connecting, hollow faint = offline. */
function Dot({ status }: { status: StreamStatus }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "inline-block size-2 shrink-0 rounded-full border border-foreground",
        status === "open" && "bg-foreground",
        (status === "connecting" || status === "reconnecting") && "animate-pulse motion-reduce:animate-none",
        status === "closed" && "border-muted-foreground",
      )}
    />
  );
}

interface LiveIndicatorProps {
  status: StreamStatus;
  /** Dot only; the label moves to a tooltip and the accessible name. */
  compact?: boolean;
  className?: string;
}

/** Connection state of an SSE subscription. */
export function LiveIndicator({ status, compact = false, className }: LiveIndicatorProps) {
  if (!compact) {
    return (
      <span
        role="status"
        className={cn("inline-flex items-center gap-1.5 text-xs font-medium text-muted-foreground", className)}
      >
        <Dot status={status} />
        {LABEL[status]}
      </span>
    );
  }
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          role="status"
          tabIndex={0}
          aria-label={`Event stream: ${LABEL[status]}`}
          className={cn(
            "inline-flex size-8 items-center justify-center rounded-md outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
            className,
          )}
        >
          <Dot status={status} />
        </span>
      </TooltipTrigger>
      <TooltipContent>
        <span className="font-medium">{LABEL[status]}</span> · {DESCRIPTION[status]}
      </TooltipContent>
    </Tooltip>
  );
}
