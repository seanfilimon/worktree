/** Merge request side panels for v0.7 review (§15.7): draft state, requested reviewers, code owners. */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Check, CircleCheck, CircleDashed, CircleX, Clock, FilePen, LoaderCircle, UserPlus, Users, X } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { errorMessage } from "@/api/client";
import { keys } from "@/api/queries";
import type { MergeRequest } from "@/api/types";
import { isUnavailable, type ReviewApproval, type ReviewState } from "@/api/v05tree";
import {
  parsePrincipal,
  reviewersOf,
  splitPrincipals,
  useMrOwners,
  useV07ReviewApi,
  type MergeRequestV07,
} from "@/api/v07review";
import { UserChip } from "@/components/mr/UserAvatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";
import { InlineUnavailable } from "../v05/shared";

/** Outline "Draft" badge. */
export function DraftBadge({ className }: { className?: string }) {
  return (
    <Badge variant="outline" data-draft className={cn("border-dashed", className)}>
      <FilePen aria-hidden="true" />
      Draft
    </Badge>
  );
}

/** POST .../ready: shown to everyone on drafts; the server checks author or mr:merge. */
export function ReadyForReviewButton({ tenant, tree, mr }: { tenant: string; tree: string; mr: MergeRequest }) {
  const api = useV07ReviewApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const ready = useMutation({
    mutationFn: () => api.mergeRequests.ready(tenant, tree, mr.number),
    meta: { inlineError: true },
    onSuccess: async (updated) => {
      if (updated && typeof updated === "object" && "number" in updated) {
        queryClient.setQueryData(keys.mergeRequest(tenant, tree, mr.number), updated);
      }
      toast.success(`!${mr.number} is ready for review`, mr.title);
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree] });
    },
    onError: (err) => toast.error(err, "Could not mark ready for review"),
  });
  return (
    <Button variant="outline" disabled={ready.isPending} onClick={() => ready.mutate()}>
      {ready.isPending ? <LoaderCircle aria-hidden="true" className="animate-spin" /> : <CircleCheck aria-hidden="true" />}
      Ready for review
    </Button>
  );
}

type ReviewerStatus = "approved" | "changes_requested" | "pending" | "team";

const STATUS: Record<ReviewerStatus, { label: string; icon: typeof Check; variant: "default" | "outline" | "secondary"; className?: string }> = {
  approved: { label: "approved", icon: Check, variant: "default" },
  changes_requested: { label: "changes requested", icon: CircleX, variant: "outline", className: "font-semibold" },
  pending: { label: "pending", icon: Clock, variant: "outline", className: "text-muted-foreground" },
  team: { label: "team", icon: Users, variant: "secondary" },
};

function statusOf(reviewer: string, reviews: readonly ReviewApproval[], stateOf: (r: ReviewApproval) => ReviewState): ReviewerStatus {
  const p = parsePrincipal(reviewer);
  if (p.kind === "team") return "team";
  const r = reviews.find((x) => x.username === p.name);
  if (!r) return "pending";
  const s = stateOf(r);
  return s === "approved" ? "approved" : s === "changes_requested" ? "changes_requested" : "pending";
}

/** Requested reviewers with their review status, plus request / remove (§15.7). */
export function ReviewersPanel({
  tenant,
  tree,
  mr,
  reviews,
  stateOf,
}: {
  tenant: string;
  tree: string;
  mr: MergeRequest;
  /** Latest review per account. */
  reviews: readonly ReviewApproval[];
  stateOf: (r: ReviewApproval) => ReviewState;
}) {
  const api = useV07ReviewApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [text, setText] = useState("");
  const open = mr.state === "open";
  const reviewers = reviewersOf(mr);

  const store = async (updated: MergeRequestV07) => {
    if (updated && typeof updated === "object" && "number" in updated) {
      queryClient.setQueryData(keys.mergeRequest(tenant, tree, mr.number), updated);
    }
    await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree, "mrs"] });
  };
  const add = useMutation({
    mutationFn: (list: string[]) => api.mergeRequests.addReviewers(tenant, tree, mr.number, list),
    meta: { inlineError: true },
    onSuccess: async (updated, list) => {
      setText("");
      toast.success(list.length === 1 ? "Review requested" : "Reviews requested", list.join(", "));
      await store(updated);
    },
    onError: (err) => toast.error(err, "Could not request review"),
  });
  const remove = useMutation({
    mutationFn: (reviewer: string) => api.mergeRequests.removeReviewers(tenant, tree, mr.number, [reviewer]),
    meta: { inlineError: true },
    onSuccess: async (updated, reviewer) => {
      toast.success("Reviewer removed", reviewer);
      await store(updated);
    },
    onError: (err) => toast.error(err, "Could not remove reviewer"),
  });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const list = splitPrincipals(text);
    if (list.length) add.mutate(list);
  };

  return (
    <Card role="region" aria-label="Requested reviewers" className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">Requested reviewers</CardTitle>
        {reviewers.length > 0 && (
          <CardAction>
            <span className="font-mono text-xs text-muted-foreground tabular-nums">{reviewers.length}</span>
          </CardAction>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-4">
        {reviewers.length === 0 ? (
          <p className="text-sm text-muted-foreground">No reviewers requested.</p>
        ) : (
          <ul aria-label="Reviewers" className="flex flex-col gap-1.5 text-sm">
            {reviewers.map((r) => {
              const p = parsePrincipal(r);
              const status = statusOf(r, reviews, stateOf);
              const s = STATUS[status];
              const Icon = s.icon;
              return (
                <li key={r} data-reviewer={r} data-status={status} className="flex items-center justify-between gap-2">
                  {p.kind === "team" ? (
                    <span className="inline-flex min-w-0 items-center gap-1.5">
                      <Users aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
                      <span className="truncate font-medium">{p.name}</span>
                    </span>
                  ) : (
                    <UserChip name={p.name} />
                  )}
                  <span className="flex shrink-0 items-center gap-1">
                    <Badge variant={s.variant} className={s.className}>
                      <Icon aria-hidden="true" />
                      {s.label}
                    </Badge>
                    {open && (
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        aria-label={`Remove reviewer ${r}`}
                        disabled={remove.isPending && remove.variables === r}
                        onClick={() => remove.mutate(r)}
                      >
                        <X aria-hidden="true" />
                      </Button>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
        {open && (
          <form onSubmit={submit} aria-label="Request reviewers" className="flex flex-col gap-1.5">
            <Label htmlFor={`${id}-who`} className="text-xs text-muted-foreground">
              Request review from
            </Label>
            <div className="flex gap-2">
              <Input
                id={`${id}-who`}
                value={text}
                onChange={(e) => setText(e.target.value)}
                placeholder="alice, @acme/core"
                autoComplete="off"
                spellCheck={false}
                className="h-8 font-mono text-xs"
                aria-describedby={`${id}-hint`}
              />
              <Button type="submit" size="sm" variant="outline" disabled={add.isPending || splitPrincipals(text).length === 0}>
                {add.isPending ? <LoaderCircle aria-hidden="true" className="animate-spin" /> : <UserPlus aria-hidden="true" />}
                Request
              </Button>
            </div>
            <p id={`${id}-hint`} className="text-[11px] text-muted-foreground">
              Users (alice, @alice, user:alice) or teams (@{tenant}/core, team:core), separated by spaces or commas.
            </p>
          </form>
        )}
      </CardContent>
    </Card>
  );
}

function OwnerToken({ owner }: { owner: string }) {
  const p = parsePrincipal(owner);
  return (
    <span className="inline-flex items-center gap-0.5 font-mono" title={owner}>
      {p.kind === "team" && <Users aria-hidden="true" className="size-3" />}
      {p.kind === "team" ? p.name : `@${p.name}`}
    </span>
  );
}

/** Owners of every changed path and whether they approved (`GET .../owners`). */
export function OwnersPanel({ tenant, tree, mr }: { tenant: string; tree: string; mr: MergeRequest }) {
  const owners = useMrOwners(tenant, tree, mr.number);
  const o = owners.data;
  const owned = o ? o.paths.filter((p) => p.owners.length > 0) : [];

  return (
    <Card role="region" aria-label="Code owners" className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">Code owners</CardTitle>
        {o && o.source !== "none" && (
          <CardAction>
            <Badge variant="outline" className="text-muted-foreground">
              {o.source === "config" ? "server config" : "OWNERS file"}
            </Badge>
          </CardAction>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-4 text-sm">
        {owners.isPending ? (
          <Skeleton className="h-4 w-40" />
        ) : owners.isError ? (
          isUnavailable(owners.error) ? (
            <InlineUnavailable feature="Code owners" />
          ) : (
            <p className="text-xs text-muted-foreground">Could not load owners: {errorMessage(owners.error)}</p>
          )
        ) : !o ? null : (
          <>
            <p
              data-owners-summary={o.required ? (o.satisfied ? "satisfied" : "required") : "optional"}
              className={cn("flex items-start gap-2", o.required && o.satisfied ? "font-medium" : o.required ? "font-semibold" : "text-muted-foreground")}
            >
              {o.satisfied ? (
                <CircleCheck aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
              ) : (
                <CircleX aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
              )}
              <span>
                {o.required
                  ? o.satisfied
                    ? "Owner approval satisfied"
                    : `Owner approval required · ${o.missing.length} ${o.missing.length === 1 ? "path" : "paths"} missing`
                  : o.source === "none"
                    ? "No owners configured for this tree."
                    : o.satisfied
                      ? "Owners approved (not required by protection)"
                      : "Owner approval not required by protection"}
              </span>
            </p>
            {o.paths.length > 0 && (
              <ul aria-label="Owned paths" className="flex max-h-72 flex-col gap-2 overflow-y-auto text-xs">
                {o.paths.map((p) => {
                  const state = p.owners.length === 0 ? "unowned" : p.approved ? "approved" : "missing";
                  return (
                    <li key={p.path} data-path={p.path} data-owner-state={state} className="flex items-start gap-2">
                      {state === "approved" ? (
                        <Check aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
                      ) : state === "missing" ? (
                        <CircleDashed aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
                      ) : (
                        <span aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
                      )}
                      <div className="min-w-0 flex-1">
                        <p className={cn("font-mono break-all", state === "approved" && "font-semibold", state === "unowned" && "text-muted-foreground")}>
                          <span className="sr-only">{state}: </span>
                          {p.path}
                        </p>
                        <p className="flex flex-wrap gap-x-1.5 text-muted-foreground">
                          {p.owners.length === 0 ? (
                            "no owners"
                          ) : (
                            <>
                              {p.owners.map((x) => (
                                <OwnerToken key={x} owner={x} />
                              ))}
                              <span>· {p.approved ? `approved by ${p.approved_by.join(", ")}` : "missing"}</span>
                            </>
                          )}
                        </p>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
            {owned.length === 0 && o.paths.length > 0 && o.source !== "none" && (
              <p className="text-xs text-muted-foreground">No changed path has owners.</p>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}
