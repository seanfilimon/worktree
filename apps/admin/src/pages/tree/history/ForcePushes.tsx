/**
 * v0.8 history-v2 (§16.10): force-push markers in branch history, from the
 * server reflog (GET {tree}/reflog). Older servers without the endpoint
 * simply show no markers. Admins and owners can restore the tip a force
 * push (or restore) replaced (POST {tree}/reflog/restore).
 */
import { FastForward, History, ShieldAlert } from "lucide-react";
import { Link } from "react-router";
import { useRestoreBranch, type ReflogEntry } from "@/api/historyV2";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useToast } from "@/toast/store";
import { shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { RelativeTime } from "./shared";

/** Marker on a snapshot row that a force push made the branch tip. */
export function ForcePushBadge({ entry }: { entry: ReflogEntry }) {
  return (
    <Badge
      variant="outline"
      title={`${entry.kind === "restore" ? "Restored" : "Force-pushed"} by ${entry.actor}, replacing ${shortId(entry.old_tip)}`}
    >
      <FastForward aria-hidden="true" />
      {entry.kind === "restore" ? "restored" : "force-pushed"}
    </Badge>
  );
}

/** Restores `branch` to the tip `entry` replaced, after a confirmation. */
function RestoreButton({
  tenant,
  tree,
  branch,
  entry,
  currentTip,
}: {
  tenant: string;
  tree: string;
  branch: string;
  entry: ReflogEntry;
  currentTip: string;
}) {
  const toast = useToast();
  const restore = useRestoreBranch(tenant, tree);
  const target = shortId(entry.old_tip);
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="outline" size="sm" disabled={restore.isPending} aria-label={`Restore ${branch} to ${target}`}>
          <History aria-hidden="true" />
          Restore
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            Restore {branch} to {target}?
          </AlertDialogTitle>
          <AlertDialogDescription>
            {branch} moves from {shortId(currentTip)} back to {target}, the tip {entry.actor || "someone"} replaced. No
            snapshot is deleted: the current tip stays in the reflog and can be restored in turn. Branch protection
            applies, and the restore fails if someone pushes first.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={() =>
              restore.mutate(
                { branch, tip: entry.old_tip, expected_tip: currentTip },
                {
                  onSuccess: () => toast.success("Branch restored", `${branch} is at ${target}`),
                  onError: (err) => toast.error(err, `Could not restore ${branch}`),
                },
              )
            }
          >
            <History aria-hidden="true" />
            Restore {branch}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/**
 * The force pushes of a branch: who rewound it, from which tip to which.
 * With `currentTip` (only passed to callers who may restore) each entry
 * gets a "Restore" action back to the tip it replaced.
 */
export function ForcePushHistory({
  tenant,
  tree,
  branch,
  entries,
  currentTip,
}: {
  tenant: string;
  tree: string;
  branch: string;
  entries: readonly ReflogEntry[];
  currentTip?: string;
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
            <span className="text-muted-foreground">{e.kind === "restore" ? "restored" : "force-pushed"}</span>
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
            {currentTip && e.old_tip && e.old_tip !== currentTip && (
              <RestoreButton tenant={tenant} tree={tree} branch={branch} entry={e} currentTip={currentTip} />
            )}
          </li>
        ))}
      </ol>
    </section>
  );
}
