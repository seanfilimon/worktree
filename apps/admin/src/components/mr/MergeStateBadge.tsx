import { GitMerge, GitPullRequest, GitPullRequestClosed } from "lucide-react";
import type { MergeRequestState } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

const LABEL: Record<MergeRequestState, string> = { open: "Open", merged: "Merged", closed: "Closed" };

/** Monochrome state: open = outline, merged = filled, closed = struck and dimmed. */
export function MergeStateBadge({ state, className }: { state: MergeRequestState; className?: string }) {
  const Icon = state === "merged" ? GitMerge : state === "closed" ? GitPullRequestClosed : GitPullRequest;
  return (
    <Badge
      variant={state === "merged" ? "default" : "outline"}
      data-state={state}
      className={cn(state === "closed" && "text-muted-foreground line-through decoration-1", className)}
    >
      <Icon aria-hidden />
      {LABEL[state]}
    </Badge>
  );
}
