import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export type BadgeTone = "neutral" | "accent" | "danger" | "warning" | "info";

const TONES: Record<BadgeTone, string> = {
  neutral: "bg-surface-2 text-muted border-border",
  accent: "bg-accent-soft text-accent border-accent/30",
  danger: "bg-danger-soft text-danger border-danger/30",
  warning: "bg-warning-soft text-warning border-warning/30",
  info: "bg-info-soft text-info border-info/30",
};

export function Badge({ tone = "neutral", children, className, title }: { tone?: BadgeTone; children: ReactNode; className?: string; title?: string }) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex items-center gap-1 rounded-full border px-1.5 py-px text-[11px] leading-4 font-medium whitespace-nowrap",
        TONES[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

const STATE_TONES: Record<string, BadgeTone> = {
  open: "accent",
  merged: "info",
  closed: "neutral",
  private: "neutral",
  internal: "warning",
  public: "info",
};

/** Badge for merge-request state or tree visibility values. */
export function StateBadge({ value }: { value: string }) {
  return <Badge tone={STATE_TONES[value] ?? "neutral"}>{value}</Badge>;
}
