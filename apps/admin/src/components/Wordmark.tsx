import { cn } from "@/lib/utils";

/** Monochrome, text-only W0rkTree wordmark in the mono font. */
export function Wordmark({ className, suffix }: { className?: string; suffix?: string }) {
  return (
    <span
      className={cn(
        "inline-flex items-baseline gap-1.5 font-mono font-semibold tracking-tight text-foreground",
        className,
      )}
    >
      W0rkTree
      {suffix && (
        <span className="font-sans text-[0.7em] font-medium tracking-normal text-muted-foreground">{suffix}</span>
      )}
    </span>
  );
}
