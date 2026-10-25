/** Merge request side panels for v0.5 (§13.4): CI checks, server merge readiness, linked group. */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, CircleX, ExternalLink, GitMerge, Link2, LoaderCircle } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { errorMessage } from "@/api/client";
import { useTenantTrees } from "@/api/queries";
import { isUnavailable, useCIStatus, useLinkedGroup, useTreeV05Api, v05Keys, type MergeChecks } from "@/api/v05tree";
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
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { shortId } from "@/lib/format";
import { mergeRequestPath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";
import { RelativeTime } from "../history/shared";
import { CIStateBadge, CIStateIcon, InlineUnavailable } from "./shared";

/** CI checks reported for the merge request's source tip (or merged snapshot). */
export function ChecksPanel({
  tenant,
  tree,
  snapshot,
  branch,
  missing = [],
}: {
  tenant: string;
  tree: string;
  /** Preferred: the exact snapshot. */
  snapshot?: string;
  /** Fallback when the snapshot is unknown. */
  branch?: string;
  /** Required contexts that have not reported (from the merge checks). */
  missing?: readonly string[];
}) {
  const ci = useCIStatus(tenant, tree, { snapshot, branch });
  const checks = ci.data?.checks ?? [];

  return (
    <Card role="region" className="gap-3 py-4" aria-label="Checks">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">Checks</CardTitle>
        {ci.data && (
          <CardAction>
            <CIStateBadge state={ci.data.state} />
          </CardAction>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-2 px-4">
        {ci.isPending ? (
          <Skeleton className="h-4 w-40" />
        ) : ci.isError ? (
          isUnavailable(ci.error) ? (
            <InlineUnavailable feature="CI checks" />
          ) : (
            <p className="text-xs text-muted-foreground">Could not load checks: {errorMessage(ci.error)}</p>
          )
        ) : checks.length === 0 && missing.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            No checks reported{ci.data.snapshot ? <> for <span className="font-mono">{shortId(ci.data.snapshot)}</span></> : null}.
          </p>
        ) : (
          <ul aria-label="CI checks" className="flex flex-col gap-2 text-sm">
            {[...checks]
              .sort((a, b) => a.context.localeCompare(b.context))
              .map((c) => (
                <li key={c.context} data-ci-context={c.context} data-state={c.state} className="flex items-start gap-2">
                  <CIStateIcon state={c.state} className="mt-0.5" />
                  <div className="min-w-0 flex-1">
                    <p className={cn("flex items-center gap-1 font-mono text-xs", c.state === "success" ? "font-semibold" : "")}>
                      <span className="sr-only">{c.state}: </span>
                      {c.target_url ? (
                        <a href={c.target_url} target="_blank" rel="noreferrer noopener" className="inline-flex items-center gap-1 hover:underline">
                          {c.context}
                          <ExternalLink aria-hidden="true" className="size-3" />
                        </a>
                      ) : (
                        c.context
                      )}
                    </p>
                    {c.description && <p className="text-xs text-muted-foreground">{c.description}</p>}
                    <p className="text-[11px] text-muted-foreground">
                      {c.reporter} · <RelativeTime value={c.updated_at} />
                    </p>
                  </div>
                </li>
              ))}
            {missing
              .filter((m) => !checks.some((c) => c.context === m))
              .map((m) => (
                <li key={`missing:${m}`} data-ci-context={m} data-state="missing" className="flex items-start gap-2 text-muted-foreground">
                  <CIStateIcon state="pending" className="mt-0.5" />
                  <div>
                    <p className="font-mono text-xs">{m}</p>
                    <p className="text-xs">Required · not reported</p>
                  </div>
                </li>
              ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

/** What the server says about mergeability (`GET .../checks`). */
export function ServerReadiness({ checks, error, pending }: { checks?: MergeChecks; error: unknown; pending: boolean }) {
  if (pending) return <Skeleton className="h-4 w-48" />;
  if (error) {
    return isUnavailable(error) ? (
      <InlineUnavailable feature="Server merge checks" />
    ) : (
      <p className="text-xs text-muted-foreground">Could not load server checks: {errorMessage(error)}</p>
    );
  }
  if (!checks) return null;
  const Icon = checks.mergeable ? CircleCheck : CircleX;
  return (
    <section aria-label="Server merge checks" className="flex flex-col gap-2 text-sm">
      <p className={cn("flex items-center gap-2", checks.mergeable ? "font-medium" : "text-muted-foreground")} data-mergeable={checks.mergeable}>
        <Icon aria-hidden="true" className="size-4 shrink-0" />
        {checks.mergeable ? "Server: ready to merge" : "Server: merging is blocked"}
      </p>
      {checks.blockers.length > 0 && (
        <ul aria-label="Merge blockers" className="flex flex-col gap-1.5 pl-6">
          {checks.blockers.map((b, i) => (
            <li key={`${b.kind}:${i}`} data-blocker={b.kind} className="text-xs">
              <span className="font-mono font-semibold">{b.kind}</span> <span className="text-muted-foreground">{b.message}</span>
            </li>
          ))}
        </ul>
      )}
      <dl className="grid grid-cols-2 gap-x-3 gap-y-1 pl-6 text-xs text-muted-foreground">
        <dt>Approvals</dt>
        <dd className="text-foreground tabular-nums">
          {checks.approvals.current} / {checks.approvals.required}
          {checks.approvals.stale > 0 && <span className="text-muted-foreground"> · {checks.approvals.stale} stale</span>}
        </dd>
        {(checks.ci.state || checks.ci.required.length > 0) && (
          <>
            <dt>CI</dt>
            <dd className="text-foreground">
              {checks.ci.state || "none"}
              {checks.ci.missing.length > 0 && <span className="text-muted-foreground"> · {checks.ci.missing.length} missing</span>}
            </dd>
          </>
        )}
        {checks.todos.blocking_open > 0 && (
          <>
            <dt>Blocking TODOs</dt>
            <dd className="text-foreground tabular-nums">{checks.todos.blocking_open} open</dd>
          </>
        )}
      </dl>
    </section>
  );
}

/** The linked group this merge request belongs to; members merge atomically. */
export function LinkedGroupCard({ tenant, groupId, currentTreeId, canMerge }: { tenant: string; groupId: string; currentTreeId: string; canMerge: boolean }) {
  const api = useTreeV05Api();
  const group = useLinkedGroup(tenant, groupId);
  const trees = useTenantTrees(tenant);
  const queryClient = useQueryClient();
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const names = new Map((trees.data ?? []).map((t) => [t.tree.id, t.tree.name]));

  const merge = useMutation({
    mutationFn: () => api.linkedGroups.merge(tenant, groupId),
    meta: { inlineError: true },
    onSuccess: async () => {
      toast.success("Linked group merged", group.data?.name ?? groupId);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: v05Keys.linkedGroups(tenant) }),
        queryClient.invalidateQueries({ queryKey: ["tree"] }),
      ]);
    },
  });

  const g = group.data;
  return (
    <Card role="region" className="gap-3 py-4" aria-label="Linked group">
      <CardHeader className="px-4">
        <CardTitle className="flex items-center gap-1.5 text-sm">
          <Link2 aria-hidden="true" className="size-4" />
          Linked group
        </CardTitle>
        {g && (
          <CardAction>
            <Badge variant={g.state === "merged" ? "default" : "outline"} className={cn(g.state === "closed" && "line-through")}>
              {g.state}
            </Badge>
          </CardAction>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-4 text-sm">
        {group.isPending ? (
          <Skeleton className="h-4 w-40" />
        ) : group.isError || !g ? (
          isUnavailable(group.error) ? (
            <InlineUnavailable feature="Linked groups" />
          ) : (
            <p className="text-xs text-muted-foreground">Could not load the group: {errorMessage(group.error)}</p>
          )
        ) : (
          <>
            <p>
              <span className="font-medium">{g.name}</span>
              <span className="block text-xs text-muted-foreground">All members merge together, or none do.</span>
            </p>
            <ul aria-label="Group members" className="flex flex-col gap-1 text-xs">
              {g.members.map((m) => {
                const name = names.get(m.tree_id);
                return (
                  <li key={`${m.tree_id}#${m.mr_number}`} className="flex items-center gap-1.5 font-mono">
                    {name ? (
                      <Link to={mergeRequestPath(tenant, name, m.mr_number)} className="hover:underline">
                        {tenant}/{name}!{m.mr_number}
                      </Link>
                    ) : (
                      <span title={m.tree_id}>
                        {shortId(m.tree_id)}!{m.mr_number}
                      </span>
                    )}
                    {m.tree_id === currentTreeId && <span className="font-sans text-muted-foreground">(this tree)</span>}
                  </li>
                );
              })}
            </ul>
            {merge.isError && <p className="text-xs font-medium">Group merge failed: {errorMessage(merge.error)}</p>}
            {g.state === "open" && canMerge && (
              <AlertDialog open={open} onOpenChange={setOpen}>
                <AlertDialogTrigger asChild>
                  <Button variant="outline" size="sm" disabled={merge.isPending}>
                    {merge.isPending ? <LoaderCircle aria-hidden="true" className="animate-spin" /> : <GitMerge aria-hidden="true" />}
                    Merge group
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Merge linked group “{g.name}”?</AlertDialogTitle>
                    <AlertDialogDescription>
                      All {g.members.length} merge requests merge together. If any of them is not mergeable, none is merged.
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction onClick={() => merge.mutate()}>Merge all</AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}
