import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export function Card({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("rounded-lg border border-border bg-surface shadow-card", className)}>{children}</div>;
}

interface SectionProps {
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  /** Removes body padding (for edge-to-edge tables). */
  flush?: boolean;
}

/** A titled card; the title is a real heading for document outline. */
export function Section({ title, description, actions, children, className, flush = false }: SectionProps) {
  return (
    <Card className={className}>
      <div className="flex flex-wrap items-start justify-between gap-2 border-b border-border px-4 py-3">
        <div className="min-w-0">
          <h2 className="text-sm font-semibold">{title}</h2>
          {description && <p className="mt-0.5 text-[13px] text-muted-foreground">{description}</p>}
        </div>
        {actions && <div className="flex items-center gap-2">{actions}</div>}
      </div>
      <div className={flush ? undefined : "p-4"}>{children}</div>
    </Card>
  );
}
