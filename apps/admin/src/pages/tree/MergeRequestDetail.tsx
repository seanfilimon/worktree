import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link, useParams } from "react-router";
import { useApi } from "@/api/context";
import { keys, useMergeRequest, useProtections } from "@/api/queries";
import type { MergeRequest } from "@/api/types";
import { useAuth } from "@/auth/context";
import { StateBadge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card, Section } from "@/components/ui/Card";
import { ConfirmButton } from "@/components/ui/ConfirmButton";
import { Alert, EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { RelativeTime } from "@/components/ui/Time";
import { pathMatch } from "@/lib/glob";
import { shortId } from "@/lib/format";
import { describeMergeError } from "@/lib/mergeErrors";
import { filesPath, treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";

type Action = "approve" | "merge" | "close";

const DONE: Record<Action, string> = { approve: "Approved", merge: "Merged", close: "Closed" };

export function MergeRequestDetail() {
  const api = useApi();
  const { tenant, tree } = useTreeContext();
  const { number = "" } = useParams();
  const n = Number(number);
  const mr = useMergeRequest(tenant, tree, n);
  const protections = useProtections(tenant, tree);
  const { account } = useAuth();
  const queryClient = useQueryClient();
  const toast = useToast();

  const act = useMutation({
    mutationFn: (action: Action) => api.mergeRequests[action](tenant, tree, n),
    meta: { inlineError: true },
    onSuccess: async (updated: MergeRequest, action) => {
      queryClient.setQueryData(keys.mergeRequest(tenant, tree, n), updated);
      toast.success(`${DONE[action]} !${updated.number}`, updated.title);
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree] });
    },
  });

  if (!Number.isInteger(n) || n <= 0) {
    return <EmptyState title="Invalid merge request number" />;
  }
  if (mr.isPending) return <SkeletonRows rows={6} label="Loading merge request" />;
  if (mr.isError) return <ErrorState error={mr.error} onRetry={() => void mr.refetch()} />;

  const m = mr.data;
  const approvals = m.approvals ?? [];
  const rule = (protections.data ?? []).find((p) => pathMatch(p.pattern, m.target_branch));
  const required = rule?.required_approvals ?? 0;
  const approvedByMe = approvals.some((a) => a.account_id === account?.id);
  const open = m.state === "open";
  const error = act.isError ? describeMergeError(act.error) : null;

  return (
    <div className="flex flex-col gap-5">
      <div>
        <Link to={treePath(tenant, tree, "merge-requests")} className="text-[13px] text-muted hover:text-fg hover:underline">
          ← All merge requests
        </Link>
        <h2 className="mt-2 flex flex-wrap items-center gap-2 text-lg font-semibold">
          <span className="font-mono text-muted">!{m.number}</span>
          {m.title}
          <StateBadge value={m.state} />
        </h2>
        <p className="mt-1 text-[13px] text-muted">
          <span className="font-medium text-fg">{m.author}</span> wants to merge{" "}
          <code className="rounded bg-surface-2 px-1 font-mono text-fg">{m.source_branch}</code> into{" "}
          <code className="rounded bg-surface-2 px-1 font-mono text-fg">{m.target_branch}</code> · opened <RelativeTime value={m.created_at} />
        </p>
      </div>

      {error && (
        <Alert tone={error.kind === "review_required" ? "warning" : "danger"} title={error.title}>
          <p>{error.message}</p>
          {error.kind === "review_required" && required > 0 && (
            <p className="mt-1">
              {approvals.length} of {required} required approvals.
            </p>
          )}
          {error.detail !== error.message && <p className="mt-1 font-mono text-xs">Server: {error.detail}</p>}
        </Alert>
      )}

      {m.state === "merged" && (
        <Alert tone="success" title="Merged">
          {m.target_branch} was fast-forwarded <RelativeTime value={m.merged_at} />
          {m.merged_snapshot && (
            <>
              {" "}
              to{" "}
              <Link to={filesPath(tenant, tree, { snapshot: m.merged_snapshot })} className="font-mono text-accent hover:underline">
                {shortId(m.merged_snapshot)}
              </Link>
            </>
          )}
          .
        </Alert>
      )}

      <div className="grid gap-5 lg:grid-cols-3">
        <Section title="Description" className="lg:col-span-2">
          {m.description ? (
            <p className="text-sm whitespace-pre-wrap">{m.description}</p>
          ) : (
            <p className="text-[13px] text-subtle italic">No description provided.</p>
          )}
        </Section>

        <div className="flex flex-col gap-5">
          <Section title="Review">
            <p className="text-[13px]">
              <span className="font-semibold tabular-nums">{approvals.length}</span>
              {required > 0 ? ` of ${required} required` : ""} {approvals.length === 1 && required === 0 ? "approval" : "approvals"}
            </p>
            {rule && (
              <p className="mt-1 text-xs text-muted">
                Protected by <span className="font-mono">{rule.pattern}</span>
                {rule.no_direct_push && " · no direct push"}
              </p>
            )}
            {approvals.length > 0 && (
              <ul aria-label="Approvals" className="mt-3 flex flex-col gap-1.5 text-[13px]">
                {approvals.map((a) => (
                  <li key={a.account_id} className="flex items-center justify-between gap-2">
                    <span className="font-medium">{a.username}</span>
                    <RelativeTime value={a.at} className="text-xs text-muted" />
                  </li>
                ))}
              </ul>
            )}
          </Section>

          {open && (
            <Card className="flex flex-col gap-2 p-4">
              <Button
                onClick={() => act.mutate("approve")}
                loading={act.isPending && act.variables === "approve"}
                disabled={act.isPending || approvedByMe}
              >
                {approvedByMe ? "You approved" : "Approve"}
              </Button>
              <Button variant="primary" onClick={() => act.mutate("merge")} loading={act.isPending && act.variables === "merge"} disabled={act.isPending}>
                Merge (fast-forward)
              </Button>
              <ConfirmButton
                confirmLabel="Close merge request"
                onConfirm={() => act.mutate("close")}
                loading={act.isPending && act.variables === "close"}
                disabled={act.isPending}
                variant="ghost"
                size="md"
              >
                Close
              </ConfirmButton>
            </Card>
          )}
        </div>
      </div>
    </div>
  );
}
