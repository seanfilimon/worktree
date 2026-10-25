import { FileMinus, FilePen, FilePlus, History } from "lucide-react";
import { Link } from "react-router";
import { useFileHistory } from "@/api/queries";
import type { FileChangeStatus } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { firstLine, shortId } from "@/lib/format";
import { filesPath, treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { EmptyState, ErrorState, LoadingRows, RelativeTime } from "./states";

const CHANGE = {
  added: { icon: FilePlus, variant: "default", label: "added" },
  modified: { icon: FilePen, variant: "outline", label: "modified" },
  deleted: { icon: FileMinus, variant: "outline", label: "deleted" },
} as const;

/** Monochrome change badge: filled = added, outline = modified, struck = deleted. */
export function ChangeBadge({ change }: { change: FileChangeStatus }) {
  const c = CHANGE[change];
  const Icon = c.icon;
  return (
    <Badge variant={c.variant} className={cn("font-mono", change === "deleted" && "line-through")}>
      <Icon aria-hidden="true" />
      {c.label}
    </Badge>
  );
}

interface FileHistoryProps {
  tenant: string;
  tree: string;
  path: string;
  /** Branch whose history is searched ("" = default branch). */
  branch: string;
  /** Snapshot currently shown, marked in the list. */
  current: string;
}

/** Snapshots on `branch` that touched `path`, newest first. */
export function FileHistory({ tenant, tree, path, branch, current }: FileHistoryProps) {
  const history = useFileHistory(tenant, tree, path, branch);

  if (history.isPending) return <LoadingRows rows={4} label="Loading file history" />;
  if (history.isError) return <ErrorState error={history.error} onRetry={() => void history.refetch()} />;
  if (history.data.length === 0)
    return (
      <EmptyState icon={History} title="No history found">
        No snapshot on <span className="font-mono">{branch || "the default branch"}</span> changed this file.
      </EmptyState>
    );

  return (
    <ol aria-label={`History of ${path}`} className="divide-y">
      {history.data.map(({ snapshot: s, change }) => {
        const isCurrent = s.id === current;
        return (
          <li key={s.id} className={cn("flex flex-wrap items-start gap-x-3 gap-y-1 px-4 py-3", isCurrent && "bg-accent")}>
            <ChangeBadge change={change} />
            <div className="min-w-0 flex-1">
              <Link
                to={treePath(tenant, tree, `snapshots/${encodeURIComponent(s.id)}`)}
                className="block truncate text-sm font-medium hover:underline"
              >
                {firstLine(s.message) || <span className="text-muted-foreground italic">No message</span>}
              </Link>
              <p className="text-xs text-muted-foreground">
                <span className="font-medium text-foreground">{s.author}</span> · <RelativeTime value={s.created_at} />
                {s.auto_generated && " · auto"}
                {isCurrent && <span className="font-semibold text-foreground"> · viewing</span>}
              </p>
            </div>
            <div className="flex items-center gap-3 text-xs">
              <Link
                to={treePath(tenant, tree, `snapshots/${encodeURIComponent(s.id)}`)}
                className="font-mono hover:underline"
                title={s.id}
                aria-label={`Snapshot ${shortId(s.id)}`}
              >
                {shortId(s.id)}
              </Link>
              {change !== "deleted" && !isCurrent && (
                <Link
                  to={filesPath(tenant, tree, { snapshot: s.id, path })}
                  className="text-muted-foreground hover:text-foreground hover:underline"
                >
                  View file
                </Link>
              )}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
