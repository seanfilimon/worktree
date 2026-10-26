/**
 * v0.8 history-v2 (§16.10): force-push markers in branch history, from the
 * server reflog (GET {tree}/reflog). Older servers without the endpoint
 * simply show no markers.
 */
import { FastForward, ShieldAlert } from "lucide-react";
import { Link } from "react-router";
import type { ReflogEntry } from "@/api/historyV2";
import { Badge } from "@/components/ui/badge";
import { shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { RelativeTime } from "./shared";

/** Marker on a snapshot row that a force push made the branch tip. */
export function ForcePushBadge({ entry }: { entry: ReflogEntry }) {
  return (
    <Badge
      variant="outline"
      title={`Force-pushed by ${entry.actor}, replacing ${shortId(entry.old_tip)}`}
    >
      <FastForward aria-hidden="true" />
      force-pushed
    </Badge>
  );
}

/** The force pushes of a branch: who rewound it, from which tip to which. */
export function ForcePushHistory({
  tenant,
  tree,
  branch,
  entries,
}: {
  tenant: string;
  tree: string;
  branch: string;
  entries: readonly ReflogEntry[];
}) {
  if (entries.length === 0) return null;
  const snap = (id: string) =>
    treePath(tenant, tree, `snapshots/${encodeURIComponent(id)}`);
  return (
    <section
      aria-label={`Force pushes on ${branch}`}
      className="mb-5 rounded-lg border"
    >
      <h3 className="flex items-center gap-2 border-b px-3 py-2 text-xs font-medium">
        <FastForward aria-hidden="true" className="size-3.5" />
        {entries.length === 1
          ? "1 force push"
          : `${entries.length} force pushes`}{" "}
        rewrote {branch}
      </h3>
      <ol className="divide-y text-sm">
        {entries.map((e) => (
          <li
            key={`${e.at}-${e.new_tip}`}
            className="flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-2"
          >
            <span className="font-medium">{e.actor || "unknown"}</span>
            <span className="text-muted-foreground">force-pushed</span>
            {e.old_tip ? (
              <span
                className="rounded border px-1.5 py-0.5 font-mono text-xs line-through"
                title={`Replaced tip ${e.old_tip}`}
              >
                {shortId(e.old_tip)}
              </span>
            ) : (
              <span className="text-muted-foreground">(new)</span>
            )}
            <span aria-hidden="true">→</span>
            <Link
              to={snap(e.new_tip)}
              className="rounded border px-1.5 py-0.5 font-mono text-xs hover:underline"
              title={e.new_tip}
            >
              {shortId(e.new_tip)}
            </Link>
            {e.bypass && (
              <Badge
                variant="outline"
                title={
                  e.bypassed.join("; ") || "Branch protection was bypassed"
                }
              >
                <ShieldAlert aria-hidden="true" />
                bypassed protection
              </Badge>
            )}
            <RelativeTime
              value={e.at}
              className="ml-auto text-xs text-muted-foreground"
            />
          </li>
        ))}
      </ol>
    </section>
  );
}
