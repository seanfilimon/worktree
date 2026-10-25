import { ArrowRight, GitBranch } from "lucide-react";
import { cn } from "@/lib/utils";

function BranchName({ name }: { name: string }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1 rounded-sm bg-muted px-1.5 py-0.5 font-mono text-xs text-foreground">
      <GitBranch aria-hidden className="size-3 shrink-0 text-muted-foreground" />
      <span className="truncate">{name}</span>
    </span>
  );
}

/** `source → target`, readable as "source into target" by screen readers. */
export function BranchFlow({ source, target, className }: { source: string; target: string; className?: string }) {
  return (
    <span className={cn("inline-flex min-w-0 flex-wrap items-center gap-1", className)}>
      <BranchName name={source} />
      <ArrowRight aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="sr-only">into</span>
      <BranchName name={target} />
    </span>
  );
}
