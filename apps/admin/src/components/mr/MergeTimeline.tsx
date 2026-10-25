import { CircleDot, GitMerge, GitPullRequestClosed, ThumbsUp, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router";
import type { MergeRequest } from "@/api/types";
import { shortId } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Timestamp } from "./Timestamp";

interface TimelineEvent {
  key: string;
  icon: LucideIcon;
  at: string;
  text: ReactNode;
  /** Terminal events are drawn inverted (filled). */
  emphasis?: boolean;
}

function events(mr: MergeRequest, snapshotHref: (id: string) => string): TimelineEvent[] {
  const list: TimelineEvent[] = [
    {
      key: "opened",
      icon: CircleDot,
      at: mr.created_at,
      text: (
        <>
          <strong className="font-semibold">{mr.author}</strong> opened this merge request
        </>
      ),
    },
    ...(mr.approvals ?? []).map((a) => ({
      key: `approval-${a.account_id}`,
      icon: ThumbsUp,
      at: a.at,
      text: (
        <>
          <strong className="font-semibold">{a.username}</strong> approved
        </>
      ),
    })),
  ];
  if (mr.state === "merged") {
    list.push({
      key: "merged",
      icon: GitMerge,
      at: mr.merged_at ?? mr.updated_at,
      emphasis: true,
      text: (
        <>
          Merged into <code className="font-mono">{mr.target_branch}</code>
          {mr.merged_snapshot && (
            <>
              {" at "}
              <Link to={snapshotHref(mr.merged_snapshot)} className="font-mono underline-offset-4 hover:underline">
                {shortId(mr.merged_snapshot)}
              </Link>
            </>
          )}
        </>
      ),
    });
  } else if (mr.state === "closed") {
    list.push({ key: "closed", icon: GitPullRequestClosed, at: mr.updated_at, emphasis: true, text: "Closed without merging" });
  }
  return list.sort((a, b) => a.at.localeCompare(b.at));
}

/** Conversation-style history: opened, approvals, merged / closed. */
export function MergeTimeline({ mr, snapshotHref }: { mr: MergeRequest; snapshotHref: (id: string) => string }) {
  return (
    <ol aria-label="Timeline" className="relative flex flex-col gap-4 before:absolute before:top-2 before:bottom-2 before:left-3 before:w-px before:bg-border">
      {events(mr, snapshotHref).map((e) => (
        <li key={e.key} className="relative flex items-start gap-3 text-sm">
          <span
            className={cn(
              "z-0 flex size-6 shrink-0 items-center justify-center rounded-full border bg-background",
              e.emphasis && "border-foreground bg-foreground text-background",
            )}
          >
            <e.icon aria-hidden className="size-3.5" />
          </span>
          <p className="min-w-0 pt-0.5">
            {e.text} <Timestamp value={e.at} className="text-muted-foreground" />
          </p>
        </li>
      ))}
    </ol>
  );
}
