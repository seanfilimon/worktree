import { cn } from "@/lib/cn";

/** Text-only W0rkTree wordmark; the zero carries the accent colour. */
export function Wordmark({ className, suffix }: { className?: string; suffix?: string }) {
  return (
    <span className={cn("inline-flex items-baseline gap-1.5 font-mono font-semibold tracking-tight", className)}>
      <span className="sr-only">W0rkTree</span>
      <span aria-hidden="true">
        W<span className="text-foreground">0</span>rkTree
      </span>
      {suffix && <span className="font-sans text-[0.7em] font-medium tracking-normal text-muted-foreground">{suffix}</span>}
    </span>
  );
}
