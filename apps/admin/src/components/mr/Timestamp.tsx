import { absoluteTime, relativeTime } from "@/lib/format";
import { cn } from "@/lib/utils";

/** Relative time ("5 minutes ago") with the absolute time as a tooltip. */
export function Timestamp({ value, className }: { value: string | undefined; className?: string }) {
  if (!value) return <span className={className}>—</span>;
  return (
    <time dateTime={value} title={absoluteTime(value)} className={cn("whitespace-nowrap", className)}>
      {relativeTime(value)}
    </time>
  );
}
