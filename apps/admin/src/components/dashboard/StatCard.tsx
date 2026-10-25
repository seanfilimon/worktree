import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

interface StatCardProps {
  label: string;
  value: ReactNode;
  icon?: LucideIcon;
  /** Secondary line under the value. */
  hint?: ReactNode;
  loading?: boolean;
  className?: string;
}

/** Compact KPI tile: label, large tabular value, optional icon and hint. */
export function StatCard({ label, value, icon: Icon, hint, loading = false, className }: StatCardProps) {
  return (
    <Card className={cn("gap-1 px-4 py-3", className)}>
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{label}</p>
        {Icon && <Icon aria-hidden="true" className="size-4 text-muted-foreground" />}
      </div>
      {loading ? (
        <Skeleton className="h-7 w-16" />
      ) : (
        <p className="font-mono text-2xl font-semibold tracking-tight tabular-nums">{value}</p>
      )}
      {hint && <p className="truncate text-xs text-muted-foreground">{hint}</p>}
    </Card>
  );
}
