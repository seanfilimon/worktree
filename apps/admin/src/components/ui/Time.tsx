import { absoluteTime, relativeTime } from "@/lib/format";

/** Relative timestamp with the absolute time as a tooltip. */
export function RelativeTime({ value, className }: { value: string | undefined; className?: string }) {
  if (!value) return <span className={className}>—</span>;
  return (
    <time dateTime={value} title={absoluteTime(value)} className={className}>
      {relativeTime(value)}
    </time>
  );
}
