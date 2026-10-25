import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { absoluteTime, relativeTime } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useNow } from "./useNow";

/**
 * Relative timestamp ("5 minutes ago") that stays current, with the absolute
 * time in a hover tooltip, in `dateTime` and as screen-reader text.
 */
export function TimeAgo({ value, className }: { value: string | undefined; className?: string }) {
  const now = useNow(30_000);
  if (!value || Number.isNaN(Date.parse(value))) return <span className={className}>—</span>;
  const absolute = absoluteTime(value);
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <time dateTime={value} className={cn("cursor-default whitespace-nowrap", className)}>
            {relativeTime(value, now)}
            <span className="sr-only"> ({absolute})</span>
          </time>
        </TooltipTrigger>
        <TooltipContent>{absolute}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
