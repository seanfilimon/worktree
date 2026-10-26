import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, CircleX, GitMerge, GitPullRequestClosed, LoaderCircle, Shield, ThumbsUp } from "lucide-react";
import { useId, useMemo, useState, type ReactNode } from "react";
import { Link, useParams, useSearchParams } from "react-router";
import { errorMessage } from "@/api/client";
import { useApi } from "@/api/context";
import { keys, useBranches, useCompare, useMergeRequest, useProtections } from "@/api/queries";
import type { CompareResult, MergeRequest } from "@/api/types";
import { useAuth } from "@/auth/context";
import { DiffViewer } from "@/components/diff/DiffViewer";
import { BranchFlow } from "@/components/mr/BranchFlow";
import { DiffStat } from "@/components/mr/ChangeSummary";
import { MergeErrorAlert } from "@/components/mr/MergeErrorAlert";
import { MergeStateBadge } from "@/components/mr/MergeStateBadge";
import { MergeTimeline } from "@/components/mr/MergeTimeline";
import { ReadinessChecklist } from "@/components/mr/ReadinessChecklist";
import { Timestamp } from "@/components/mr/Timestamp";
import { UserAvatar, UserChip } from "@/components/mr/UserAvatar";
import { branchRelation, findBranch, matchingProtections, readiness, requiredApprovals, type BranchRelation } from "@/components/mr/model";
import {
  asV05,
  latestReviews,
  reviewState,
  useMrChecks,
  type BranchProtectionV05,
  type ReviewApproval,
  type ReviewState,
} from "@/api/v05tree";
import { Badge } from "@/components/ui/badge";
import { CommentThreads, ReviewForm } from "./v05/MrDiscussion";
import { ChecksPanel, LinkedGroupCard, ServerReadiness } from "./v05/MrPanels";
import { useMergeability, useV06Api, mergeConflictsOf, MERGE_METHOD_LABEL, isMergeMethod, type MergeMethod } from "@/api/v06";
import { ConflictList, MergeOptionsFields } from "./v06/merge";
import { applyMergeability, defaultMethod, mergeMethodSummary } from "./v06/mergeUtil";
import { useHotkeys } from "@/components/mr/useHotkeys";
// v0.7 review (§15.7)
import { isDraft, type BranchProtectionV07 } from "@/api/v07review";
import { newSideLines } from "./v07/lines";
import { describeV07MergeError } from "./v07/mergeErrors";
import { ReviewDiff } from "./v07/ReviewDiff";
import { DraftBadge, OwnersPanel, ReadyForReviewButton, ReviewersPanel } from "./v07/ReviewPanels";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
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
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { firstLine, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";

type Action = "approve" | "merge" | "close";
const DONE: Record<Action, string> = { approve: "Approved", merge: "Merged", close: "Closed" };

const TABS = ["overview", "changes", "snapshots"] as const;
type Tab = (typeof TABS)[number];

export function MergeRequestDetail() {
  const { tenant, tree } = useTreeContext();
  const { number = "" } = useParams();
  const n = Number(number);
  const mr = useMergeRequest(tenant, tree, n);

  if (!Number.isInteger(n) || n <= 0) {
    return <Notice title="Invalid merge request number" message={`“${number}” is not a merge request number.`} />;
  }
  if (mr.isPending) {
    return (
      <div aria-label="Loading merge request" className="flex flex-col gap-3">
        <Skeleton className="h-6 w-2/3" />
        <Skeleton className="h-4 w-1/2" />
        <Skeleton className="h-48 w-full" />
      </div>
    );
  }
  if (mr.isError) {
    return (
      <Notice title="Could not load merge request" message={errorMessage(mr.error)}>
        <Button size="sm" variant="outline" onClick={() => void mr.refetch()}>
          Retry
        </Button>
      </Notice>
    );
  }
  return <Detail mr={mr.data} tenant={tenant} tree={tree} />;
}

function Notice({ title, message, children }: { title: string; message: string; children?: ReactNode }) {
  return (
    <Alert>
      <CircleX aria-hidden />
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>
        <p>{message}</p>
        {children}
      </AlertDescription>
    </Alert>
  );
}

/**
 * What the Changes tab compares: open/closed MRs compare the target tip with
 * the source tip; merged ones the merged snapshot with its parent.
 */
function useMergeComparison(tenant: string, tree: string, m: MergeRequest) {
  const branches = useBranches(tenant, tree);
  let to = "";
  let from: string | undefined;
  let problem: string | undefined;
  let targetEmpty = false;

  if (m.state === "merged") {
    to = m.merged_snapshot ?? "";
  } else if (branches.data) {
    const source = findBranch(branches.data, m.source_branch);
    const target = findBranch(branches.data, m.target_branch);
    if (!source) problem = `Source branch “${m.source_branch}” no longer exists.`;
    else if (!source.tip) problem = `Source branch “${m.source_branch}” has no snapshots.`;
    else if (!target?.tip) targetEmpty = true;
    else {
      to = source.tip;
      from = target.tip;
    }
  } else if (branches.isError) {
    problem = `Could not load branches: ${errorMessage(branches.error)}`;
  }

  const compare = useCompare(tenant, tree, to, from);
  const current = compare.data && !compare.isPlaceholderData ? compare.data : undefined;
  let relation: BranchRelation | null = null;
  if (targetEmpty) relation = "fast_forward";
  else if (current) relation = branchRelation(current);
  if (!problem && compare.isError && m.state !== "merged") {
    problem = `Could not compare branches: ${errorMessage(compare.error)}`;
  }
  return { compare, current, relation, problem, targetEmpty, hasSnapshot: to !== "" };
}

function Detail({ mr: m, tenant, tree }: { mr: MergeRequest; tenant: string; tree: string }) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const { account } = useAuth();
  const { canManage: canModerate } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const tab: Tab = TABS.find((t) => t === params.get("tab")) ?? "overview";
  const [confirmMerge, setConfirmMerge] = useState(false);

  const protections = useProtections(tenant, tree);
  const rules = matchingProtections(protections.data ?? [], m.target_branch);
  const required = requiredApprovals(rules);
  const cmp = useMergeComparison(tenant, tree, m);
  const open = m.state === "open";

  // v0.5 (§13.4): only approvals of the current source tip count; a
  // changes-requested review blocks until that reviewer approves.
  const v = asV05(m);
  const branchList = useBranches(tenant, tree);
  const sourceTip = findBranch(branchList.data, m.source_branch)?.tip || undefined;
  const reviews = latestReviews(v.approvals);
  const stateOf = (r: ReviewApproval): ReviewState => reviewState(r, open ? sourceTip : undefined);
  const counted = reviews.filter((r) => stateOf(r) === "approved");
  const changesRequested = reviews.filter((r) => stateOf(r) === "changes_requested");
  const have = counted.length;
  const { checks: clientChecks } = readiness({
    mr: { ...m, approvals: counted },
    required,
    relation: cmp.relation,
    relationProblem: cmp.problem,
  });
  // v0.6 (§14.1): server-side merges. Without the mergeability endpoint
  // (older servers) the client-side fast-forward check stands.
  const v06 = useV06Api();
  const mergeability = useMergeability(tenant, tree, m.number, open);
  const mergeInfo = mergeability.data;
  const checks = applyMergeability(clientChecks, mergeInfo, m.target_branch);
  const failedCheck = checks.find((c) => c.status !== "pass");
  const clientBlocker = failedCheck ? (failedCheck.detail ?? failedCheck.label) : null;
  const [pickedMethod, setPickedMethod] = useState<MergeMethod | undefined>(undefined);
  const [mergeMessage, setMergeMessage] = useState("");
  const method = pickedMethod && mergeInfo?.methods.includes(pickedMethod) ? pickedMethod : defaultMethod(mergeInfo);
  const serverChecks = useMrChecks(tenant, tree, m.number, open);
  const draft = isDraft(m);
  // v0.7 (§15.7): drafts cannot be merged (`409 draft`).
  const blocker = draft
    ? "Draft: mark it ready for review before merging."
    : (clientBlocker ??
      (changesRequested.length > 0
        ? `Changes requested by ${changesRequested.map((r) => r.username).join(", ")}.`
        : v.linked_group
          ? "Part of a linked group: merge the group instead."
          : serverChecks.data && !serverChecks.data.mergeable
            ? (serverChecks.data.blockers[0]?.message ?? "The server reports this merge request is not mergeable.")
            : null));

  const act = useMutation({
    mutationFn: (action: Action) =>
      action === "merge"
        ? v06.mergeRequests.merge(
            tenant,
            tree,
            m.number,
            mergeInfo && method ? { method, message: method === "fast-forward" ? undefined : mergeMessage } : undefined,
          )
        : api.mergeRequests[action](tenant, tree, m.number),
    meta: { inlineError: true },
    onSuccess: async (updated: MergeRequest, action) => {
      queryClient.setQueryData(keys.mergeRequest(tenant, tree, m.number), updated);
      toast.success(`${DONE[action]} !${updated.number}`, updated.title);
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree] });
    },
  });

  const { mutate } = act;
  const blockerId = useId();
  const isAuthor = account?.id === m.author_id;
  const approvedByMe = counted.some((a) => a.account_id === account?.id);
  const canApprove = open && !isAuthor && !approvedByMe && !act.isPending;
  const canMerge = open && blocker === null && !protections.isPending && !act.isPending;
  const error = act.isError ? describeV07MergeError(act.error) : null;
  const errorConflicts = act.isError ? mergeConflictsOf(act.error) : null;
  const ciRef = m.state === "merged" ? { snapshot: m.merged_snapshot } : sourceTip ? { snapshot: sourceTip } : { branch: m.source_branch };
  const snapshotHref = (id: string) => treePath(tenant, tree, `snapshots/${encodeURIComponent(id)}`);
  // Source-tip lines visible in the diff: fallback for the lines a suggestion replaces.
  const diffFiles = cmp.current?.files;
  const knownLines = useMemo(() => (m.state === "merged" || !diffFiles ? null : newSideLines(diffFiles)), [m.state, diffFiles]);
  const knownLine = useMemo(
    () => (knownLines ? (path: string, line: number) => knownLines.get(path)?.get(line) : undefined),
    [knownLines],
  );

  const hotkeys = useMemo(
    () => ({
      a: canApprove ? () => mutate("approve") : undefined,
      m: canMerge ? () => setConfirmMerge(true) : undefined,
    }),
    [canApprove, canMerge, mutate],
  );
  useHotkeys(hotkeys);

  const setTab = (value: string) =>
    setParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        if (value === "overview") p.delete("tab");
        else p.set("tab", value);
        return p;
      },
      { replace: true },
    );

  return (
    <div className="flex flex-col gap-5">
      <header className="flex flex-col gap-2">
        <Link
          to={treePath(tenant, tree, "merge-requests")}
          className="inline-flex w-fit items-center gap-1 text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
        >
          <ArrowLeft aria-hidden className="size-3.5" />
          All merge requests
        </Link>
        <h2 className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xl font-semibold tracking-tight break-words">
          <span>{m.title}</span>
          <span className="font-mono font-normal text-muted-foreground">!{m.number}</span>
        </h2>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 text-sm text-muted-foreground">
          <MergeStateBadge state={m.state} />
          {draft && <DraftBadge />}
          <UserChip name={m.author} className="text-foreground" />
          <span className="inline-flex flex-wrap items-center gap-1">
            wants to merge <BranchFlow source={m.source_branch} target={m.target_branch} />
          </span>
          <span>
            opened <Timestamp value={m.created_at} />
          </span>
          <span>
            updated <Timestamp value={m.updated_at} />
          </span>
        </div>
      </header>

      {error && (
        <MergeErrorAlert error={error}>
          {error.kind === "review_required" && required > 0 && (
            <p>
              {have} of {required} required approvals.
            </p>
          )}
          {errorConflicts && <ConflictList conflicts={errorConflicts} />}
        </MergeErrorAlert>
      )}

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <Tabs value={tab} onValueChange={setTab} className="min-w-0">
          <TabsList variant="line" className="w-full justify-start overflow-x-auto overflow-y-hidden border-b">
            <TabsTrigger value="overview" className="flex-none">
              Overview
            </TabsTrigger>
            <TabsTrigger value="changes" className="flex-none">
              Changes
              {cmp.current && <span className="font-mono text-xs text-muted-foreground tabular-nums">{cmp.current.stats.files}</span>}
            </TabsTrigger>
            <TabsTrigger value="snapshots" className="flex-none">
              Snapshots
              {cmp.current && (
                <span className="font-mono text-xs text-muted-foreground tabular-nums">{cmp.current.snapshots.length}</span>
              )}
            </TabsTrigger>
          </TabsList>

          <TabsContent value="overview" className="flex flex-col gap-5 pt-2">
            <Card className="gap-3 py-4">
              <CardHeader className="flex items-center gap-2 px-4">
                <UserAvatar name={m.author} />
                <CardTitle className="text-sm">{m.author}</CardTitle>
                <Timestamp value={m.created_at} className="text-xs text-muted-foreground" />
              </CardHeader>
              <CardContent className="px-4">
                {m.description ? (
                  <p className="text-sm break-words whitespace-pre-wrap">{m.description}</p>
                ) : (
                  <p className="text-sm text-muted-foreground italic">No description provided.</p>
                )}
              </CardContent>
            </Card>
            <MergeTimeline mr={m} snapshotHref={snapshotHref} />
            {open && !isAuthor && <ReviewForm tenant={tenant} tree={tree} mr={m} />}
            <CommentThreads
              tenant={tenant}
              tree={tree}
              mr={m}
              sourceTip={open ? sourceTip : undefined}
              files={cmp.current?.files.map((f) => f.path) ?? []}
              canModerate={canModerate}
              knownLine={knownLine}
            />
          </TabsContent>

          <TabsContent value="changes" className="pt-2">
            <ChangesPanel
              cmp={cmp}
              mr={m}
              tenant={tenant}
              tree={tree}
              sourceTip={open ? sourceTip : undefined}
              canModerate={canModerate}
            />
          </TabsContent>

          <TabsContent value="snapshots" className="pt-2">
            <SnapshotsPanel cmp={cmp} snapshotHref={snapshotHref} />
          </TabsContent>
        </Tabs>

        <aside aria-label="Review" className="flex flex-col gap-4">
          <ReviewCard
            mr={m}
            reviews={reviews}
            stateOf={stateOf}
            have={have}
            rules={rules}
            required={required}
            protectionsLoading={protections.isPending}
          />

          <ReviewersPanel tenant={tenant} tree={tree} mr={m} reviews={reviews} stateOf={stateOf} />

          {v.linked_group && (
            <LinkedGroupCard tenant={tenant} groupId={v.linked_group} currentTreeId={m.tree_id} canMerge={open} />
          )}

          {open ? (
            <Card className="gap-4 py-4">
              <CardHeader className="px-4">
                <CardTitle className="text-sm">Merge readiness</CardTitle>
              </CardHeader>
              <CardContent className="flex flex-col gap-4 px-4">
                <ReadinessChecklist checks={checks} />
                {changesRequested.length > 0 && (
                  <p data-check="changes_requested" className="flex items-start gap-2 text-sm text-muted-foreground">
                    <CircleX aria-hidden className="mt-0.5 size-4 shrink-0" />
                    <span>
                      <span className="sr-only">Failed: </span>
                      Changes requested by {changesRequested.map((r) => r.username).join(", ")}
                    </span>
                  </p>
                )}
                <ServerReadiness checks={serverChecks.data} error={serverChecks.error} pending={serverChecks.isPending} />
                <Separator />
                <div className="flex flex-col gap-2">
                  {draft && (
                    <div data-draft-notice className="flex flex-col gap-2 rounded-md border border-dashed p-2.5 text-xs text-muted-foreground">
                      <p>
                        <span className="font-semibold text-foreground">Draft.</span> Reviewers are requested and merging is possible once it is
                        ready for review.
                      </p>
                      <ReadyForReviewButton tenant={tenant} tree={tree} mr={m} />
                    </div>
                  )}
                  {!isAuthor && (
                    <Button
                      variant="outline"
                      aria-keyshortcuts="a"
                      disabled={!canApprove}
                      onClick={() => act.mutate("approve")}
                    >
                      {act.isPending && act.variables === "approve" ? (
                        <LoaderCircle aria-hidden className="animate-spin" />
                      ) : (
                        <ThumbsUp aria-hidden />
                      )}
                      {approvedByMe ? "You approved" : "Approve"}
                      {!approvedByMe && <Kbd>A</Kbd>}
                    </Button>
                  )}
                  <AlertDialog open={confirmMerge} onOpenChange={setConfirmMerge}>
                    <AlertDialogTrigger asChild>
                      <Button aria-keyshortcuts="m" disabled={!canMerge} aria-describedby={blocker ? blockerId : undefined}>
                        {act.isPending && act.variables === "merge" ? (
                          <LoaderCircle aria-hidden className="animate-spin" />
                        ) : (
                          <GitMerge aria-hidden />
                        )}
                        Merge
                        <Kbd inverted>M</Kbd>
                      </Button>
                    </AlertDialogTrigger>
                    <AlertDialogContent>
                      <AlertDialogHeader>
                        <AlertDialogTitle>
                          Merge !{m.number} into {m.target_branch}?
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                          {cmp.relation === "up_to_date"
                            ? `${m.target_branch} already contains ${m.source_branch}; the merge request will be marked merged.`
                            : mergeInfo && method
                              ? mergeMethodSummary(method, m)
                              : `${m.target_branch} will be fast-forwarded to the tip of ${m.source_branch}.`}
                        </AlertDialogDescription>
                      </AlertDialogHeader>
                      {mergeInfo && method && cmp.relation !== "up_to_date" && (
                        <MergeOptionsFields
                          mr={m}
                          mergeability={mergeInfo}
                          method={method}
                          onMethod={setPickedMethod}
                          message={mergeMessage}
                          onMessage={setMergeMessage}
                        />
                      )}
                      <AlertDialogFooter>
                        <AlertDialogCancel>Cancel</AlertDialogCancel>
                        <AlertDialogAction onClick={() => act.mutate("merge")}>Merge</AlertDialogAction>
                      </AlertDialogFooter>
                    </AlertDialogContent>
                  </AlertDialog>
                  {blocker && (
                    <p id={blockerId}className="text-xs text-muted-foreground">
                      {blocker}
                    </p>
                  )}
                  <AlertDialog>
                    <AlertDialogTrigger asChild>
                      <Button variant="ghost" disabled={act.isPending}>
                        <GitPullRequestClosed aria-hidden />
                        Close
                      </Button>
                    </AlertDialogTrigger>
                    <AlertDialogContent>
                      <AlertDialogHeader>
                        <AlertDialogTitle>Close !{m.number}?</AlertDialogTitle>
                        <AlertDialogDescription>
                          The merge request is abandoned without merging. Its branches are kept.
                        </AlertDialogDescription>
                      </AlertDialogHeader>
                      <AlertDialogFooter>
                        <AlertDialogCancel>Cancel</AlertDialogCancel>
                        <AlertDialogAction onClick={() => act.mutate("close")}>Close merge request</AlertDialogAction>
                      </AlertDialogFooter>
                    </AlertDialogContent>
                  </AlertDialog>
                </div>
              </CardContent>
            </Card>
          ) : (
            <OutcomeCard mr={m} snapshotHref={snapshotHref} />
          )}

          {m.state !== "closed" && <OwnersPanel tenant={tenant} tree={tree} mr={m} />}
          <ChecksPanel tenant={tenant} tree={tree} {...ciRef} missing={serverChecks.data?.ci.missing} />
        </aside>
      </div>
    </div>
  );
}

function Kbd({ children, inverted = false }: { children: string; inverted?: boolean }) {
  return (
    <kbd
      aria-hidden
      className={
        inverted
          ? "ml-auto rounded border border-primary-foreground/40 px-1 font-mono text-[10px] leading-4"
          : "ml-auto rounded border px-1 font-mono text-[10px] leading-4 text-muted-foreground"
      }
    >
      {children}
    </kbd>
  );
}

const REVIEW_BADGE: Record<Exclude<ReviewState, "approved">, { label: string; variant: "outline" | "secondary" | "default"; className?: string }> = {
  stale: { label: "stale", variant: "outline", className: "text-muted-foreground line-through" },
  changes_requested: { label: "changes requested", variant: "default" },
  commented: { label: "commented", variant: "secondary" },
};

function ReviewEntry({ r, state }: { r: ReviewApproval; state: ReviewState }) {
  const badge = state === "approved" ? null : REVIEW_BADGE[state];
  return (
    <li data-review={state} className="flex flex-col gap-0.5">
      <span className="flex items-center justify-between gap-2">
        <span className="flex min-w-0 items-center gap-1.5">
          <UserChip name={r.username} className={state === "stale" ? "text-muted-foreground" : undefined} />
          {badge && (
            <Badge
              variant={badge.variant}
              className={badge.className}
              title={state === "stale" && r.snapshot_id ? `Approved ${shortId(r.snapshot_id)}, an older source tip: no longer counts` : undefined}
            >
              {badge.label}
            </Badge>
          )}
        </span>
        <Timestamp value={r.at} className="shrink-0 text-xs text-muted-foreground" />
      </span>
      {r.body && <p className="pl-7 text-xs break-words whitespace-pre-wrap text-muted-foreground">{r.body}</p>}
    </li>
  );
}

function ruleSummary(r: BranchProtectionV05): string {
  const checks = r.required_checks ?? [];
  return [
    `${r.required_approvals} ${r.required_approvals === 1 ? "approval" : "approvals"}`,
    r.no_direct_push && "no direct push",
    r.no_delete && "no delete",
    r.require_ci_pass && (checks.length ? `CI: ${checks.join(", ")}` : "CI must pass"),
    r.require_signed && "signed only",
    r.require_linear && "linear",
    r.block_on_todos && "blocks on TODOs",
    (r as BranchProtectionV07).require_owner_approval && "owner approval",
  ]
    .filter(Boolean)
    .join(" · ");
}

function ReviewCard({
  mr: m,
  reviews,
  stateOf,
  have,
  rules,
  required,
  protectionsLoading,
}: {
  mr: MergeRequest;
  reviews: ReviewApproval[];
  stateOf: (r: ReviewApproval) => ReviewState;
  have: number;
  rules: BranchProtectionV05[];
  required: number;
  protectionsLoading: boolean;
}) {
  const approvals = reviews.filter((r) => {
    const s = stateOf(r);
    return s === "approved" || s === "stale";
  });
  const others = reviews.filter((r) => !approvals.includes(r));
  const stale = approvals.length - have;
  return (
    <Card className="gap-4 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">Reviewers</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 px-4">
        <div className="flex flex-col gap-2">
          <p className="text-sm">
            <span className="font-semibold tabular-nums">{have}</span>
            {required > 0 ? (
              <> of {required} required approvals</>
            ) : (
              <> {have === 1 ? "approval" : "approvals"}</>
            )}
          </p>
          {required > 0 && (
            <Progress
              value={Math.min(100, (have / required) * 100)}
              aria-label="Required approvals"
              aria-valuetext={`${have} of ${required}`}
            />
          )}
        </div>
        {approvals.length > 0 ? (
          <ul aria-label="Approvals" className="flex flex-col gap-2 text-sm">
            {approvals.map((a) => (
              <ReviewEntry key={a.account_id} r={a} state={stateOf(a)} />
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted-foreground">No approvals yet.</p>
        )}
        {stale > 0 && (
          <p className="text-xs text-muted-foreground">
            {stale} stale {stale === 1 ? "approval was" : "approvals were"} given for an older source tip and no longer{" "}
            {stale === 1 ? "counts" : "count"}.
          </p>
        )}
        {others.length > 0 && (
          <ul aria-label="Other reviews" className="flex flex-col gap-2 text-sm">
            {others.map((r) => (
              <ReviewEntry key={r.account_id} r={r} state={stateOf(r)} />
            ))}
          </ul>
        )}
        <Separator />
        <div className="flex flex-col gap-1.5 text-xs text-muted-foreground">
          <p className="inline-flex items-center gap-1.5 font-medium text-foreground">
            <Shield aria-hidden className="size-3.5" />
            Protection
          </p>
          {protectionsLoading ? (
            <Skeleton className="h-3 w-40" />
          ) : rules.length === 0 ? (
            <p>
              <span className="font-mono">{m.target_branch}</span> is not protected.
            </p>
          ) : (
            <ul aria-label="Protection rules" className="flex flex-col gap-1">
              {rules.map((r) => (
                <li key={r.pattern}>
                  <span className="font-mono text-foreground">{r.pattern}</span>
                  {" · "}
                  {ruleSummary(r)}
                </li>
              ))}
            </ul>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

/** v0.6: how a merged request was merged ("" on pre-v0.6 records = fast-forward). */
function mergedVia(m: MergeRequest): MergeMethod {
  const v = (m as MergeRequest & { merge_method?: string }).merge_method;
  return isMergeMethod(v) ? v : "fast-forward";
}

function OutcomeCard({ mr: m, snapshotHref }: { mr: MergeRequest; snapshotHref: (id: string) => string }) {
  const merged = m.state === "merged";
  return (
    <Card className="gap-2 py-4">
      <CardHeader className="px-4">
        <CardTitle className="inline-flex items-center gap-1.5 text-sm">
          {merged ? <GitMerge aria-hidden className="size-4" /> : <GitPullRequestClosed aria-hidden className="size-4" />}
          {merged ? "Merged" : "Closed"}
        </CardTitle>
      </CardHeader>
      <CardContent className="px-4 text-sm text-muted-foreground">
        {merged ? (
          <p>
            <span className="font-mono text-foreground">{m.target_branch}</span>{" "}
            {mergedVia(m) === "fast-forward" ? "was fast-forwarded" : mergedVia(m) === "squash" ? "got a squash snapshot" : "got a merge snapshot"}{" "}
            <Timestamp value={m.merged_at} />
            {mergedVia(m) !== "fast-forward" && (
              <Badge variant="outline" className="ml-1">
                {MERGE_METHOD_LABEL[mergedVia(m)]}
              </Badge>
            )}
            {m.merged_snapshot && (
              <>
                {" to "}
                <Link to={snapshotHref(m.merged_snapshot)} className="font-mono text-foreground underline-offset-4 hover:underline">
                  {shortId(m.merged_snapshot)}
                </Link>
              </>
            )}
            .
          </p>
        ) : (
          <p>
            Closed without merging <Timestamp value={m.updated_at} />.
          </p>
        )}
      </CardContent>
    </Card>
  );
}

type Comparison = ReturnType<typeof useMergeComparison>;

function ComparisonState({ cmp, children }: { cmp: Comparison; children: (c: CompareResult) => ReactNode }) {
  if (cmp.problem) return <p className="py-8 text-center text-sm text-muted-foreground">{cmp.problem}</p>;
  if (cmp.targetEmpty) {
    return <p className="py-8 text-center text-sm text-muted-foreground">The target branch is empty: merging sets it to the source tip.</p>;
  }
  if (!cmp.hasSnapshot) return <p className="py-8 text-center text-sm text-muted-foreground">No merged snapshot was recorded.</p>;
  if (cmp.compare.isError) {
    return <p className="py-8 text-center text-sm text-muted-foreground">Could not compare: {errorMessage(cmp.compare.error)}</p>;
  }
  if (!cmp.current) {
    return (
      <div aria-label="Loading changes" className="flex flex-col gap-2">
        <Skeleton className="h-4 w-48" />
        <Skeleton className="h-32 w-full" />
      </div>
    );
  }
  return children(cmp.current);
}

function ChangesPanel({
  cmp,
  mr: m,
  tenant,
  tree,
  sourceTip,
  canModerate,
}: {
  cmp: Comparison;
  mr: MergeRequest;
  tenant: string;
  tree: string;
  sourceTip?: string;
  canModerate: boolean;
}) {
  const merged = m.state === "merged";
  return (
    <ComparisonState cmp={cmp}>
      {(c) =>
        c.files.length === 0 ? (
          <p className="py-8 text-center text-sm text-muted-foreground">No file changes.</p>
        ) : (
          <div className="flex flex-col gap-3">
            <p className="text-sm">
              <DiffStat stats={c.stats} />
              {merged && <span className="text-muted-foreground"> in the merged snapshot</span>}
            </p>
            {/* v0.7: open and closed requests get inline line threads; a merged one keeps the plain viewer
                (its lines are the merged snapshot's, not the source tip's). */}
            {merged ? (
              <DiffViewer files={c.files} />
            ) : (
              <ReviewDiff
                tenant={tenant}
                tree={tree}
                mr={m}
                files={c.files}
                sourceTip={sourceTip}
                canComment={m.state === "open"}
                canModerate={canModerate}
              />
            )}
          </div>
        )
      }
    </ComparisonState>
  );
}

function SnapshotsPanel({ cmp, snapshotHref }: { cmp: Comparison; snapshotHref: (id: string) => string }) {
  return (
    <ComparisonState cmp={cmp}>
      {(c) =>
        c.snapshots.length === 0 ? (
          <p className="py-8 text-center text-sm text-muted-foreground">No snapshots to merge.</p>
        ) : (
          <ol aria-label="Snapshots" className="divide-y rounded-md border">
            {c.snapshots.map((s) => (
              <li key={s.id} className="flex items-center gap-3 px-3 py-2 text-sm">
                <UserAvatar name={s.author} />
                <div className="min-w-0 flex-1">
                  <Link to={snapshotHref(s.id)} className="block truncate font-medium underline-offset-4 hover:underline">
                    {firstLine(s.message) || "(no message)"}
                  </Link>
                  <p className="text-xs text-muted-foreground">
                    {s.author} · <Timestamp value={s.created_at} />
                  </p>
                </div>
                <code className="shrink-0 font-mono text-xs text-muted-foreground">{shortId(s.id)}</code>
              </li>
            ))}
          </ol>
        )
      }
    </ComparisonState>
  );
}
