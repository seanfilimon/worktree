/** Merge request side panels for v0.5 (§13.4): CI checks, server merge readiness, linked group. */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, CircleX, ExternalLink, FilePen, GitMerge, Link2, LoaderCircle, UserCheck, type LucideIcon } from "lucide-react";
import { useId, useState } from "react";
import { Link } from "react-router";
import { errorMessage } from "@/api/client";
import { useTenantTrees } from "@/api/queries";
import { isUnavailable, useCIStatus, useLinkedGroup, v05Keys, type MergeChecks } from "@/api/v05tree";
import { MERGE_METHOD_LABEL, MERGE_METHODS, isMergeMethod, type MergeMethod } from "@/api/v06";
import { useV07ReviewApi } from "@/api/v07review";
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
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
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

/** v0.7 (§15.7) blocker kinds with a readable label and icon; others show their kind. */
const BLOCKER_LABEL: Record<string, { label: string; icon: LucideIcon }> = {
  draft: { label: "Draft", icon: FilePen },
  owners: { label: "Owner approval", icon: UserCheck },
};

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
          {checks.blockers.map((b, i) => {
            const known = BLOCKER_LABEL[b.kind];
            const Icon = known?.icon;
            return (
              <li key={`${b.kind}:${i}`} data-blocker={b.kind} className="text-xs">
                {known && Icon ? (
                  <span className="inline-flex items-center gap-1 font-semibold">
                    <Icon aria-hidden="true" className="size-3" />
                    {known.label}
                  </span>
                ) : (
                  <span className="font-mono font-semibold">{b.kind}</span>
                )}{" "}
                <span className="text-muted-foreground">{b.message}</span>
              </li>
            );
          })}
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
  const api = useV07ReviewApi();
  const group = useLinkedGroup(tenant, groupId);
  const trees = useTenantTrees(tenant);
  const queryClient = useQueryClient();
  const toast = useToast();
  const [open, setOpen] = useState(false);
  // v0.7 (§15.7): merge method for every member; "default" = each member's own default.
  const [method, setMethod] = useState<MergeMethod | "default">("default");
  const methodId = useId();
  const names = new Map((trees.data ?? []).map((t) => [t.tree.id, t.tree.name]));

  const merge = useMutation({
    mutationFn: () => api.linkedGroups.merge(tenant, groupId, method === "default" ? undefined : method),
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
                  <div className="grid gap-2">
                    <Label htmlFor={methodId}>Merge method</Label>
                    <Select value={method} onValueChange={(v) => setMethod(isMergeMethod(v) ? v : "default")}>
                      <SelectTrigger id={methodId} className="w-full sm:w-64" aria-describedby={`${methodId}-hint`}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="default">Default</SelectItem>
                        {MERGE_METHODS.map((mm) => (
                          <SelectItem key={mm} value={mm}>
                            {MERGE_METHOD_LABEL[mm]}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <p id={`${methodId}-hint`} className="text-xs text-muted-foreground">
                      {method === "default"
                        ? "Each member merges with its own default method."
                        : `Every member merges with ${MERGE_METHOD_LABEL[method].toLowerCase()}; members whose protection forbids it block the group.`}
                    </p>
                  </div>
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
