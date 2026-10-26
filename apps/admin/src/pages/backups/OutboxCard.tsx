import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Mail, RotateCcw, Trash2 } from "lucide-react";
import { useState } from "react";
import { isApiError } from "@/api/client";
import {
  canRetryEmail,
  isUnavailable,
  outboxEmailTime,
  serverOpsKeys,
  useOutbox,
  useServerOpsApi,
  type OutboxEmail,
  type OutboxList,
  type OutboxStatus,
} from "@/api/serverops";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/dashboard/states";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { absoluteTime, formatNumber, relativeTime } from "@/lib/format";
import { NotImplementedNotice } from "@/pages/tenants/opsShared";
import { useToast } from "@/toast/store";

type Filter = "all" | OutboxStatus;

const FILTERS: { value: Filter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "pending", label: "Pending" },
  { value: "dead", label: "Dead" },
  { value: "sent", label: "Sent" },
];

const EMPTY_TEXT: Record<Filter, string> = {
  all: "Notification emails are queued here before they are sent.",
  pending: "Nothing is waiting to be sent.",
  dead: "No email has been dead-lettered.",
  sent: "No email has been sent yet.",
};

/** Pending = outline; sent = secondary; dead = filled. */
function OutboxStatusBadge({ status }: { status: OutboxStatus }) {
  const variant = status === "pending" ? "outline" : status === "sent" ? "secondary" : "default";
  return <Badge variant={variant}>{status}</Badge>;
}

function OutboxCounts({ list }: { list: OutboxList }) {
  const items: [string, number][] = [
    ["Pending", list.counts.pending],
    ["Sent", list.counts.sent],
    ["Dead", list.counts.dead],
  ];
  return (
    <dl aria-label="Outbox counts" className="flex flex-wrap gap-x-6 gap-y-1 text-sm">
      {items.map(([k, v]) => (
        <div key={k} className="flex items-baseline gap-1.5">
          <dt className="text-muted-foreground">{k}</dt>
          <dd className="font-mono tabular-nums">{formatNumber(v)}</dd>
        </div>
      ))}
    </dl>
  );
}

function OutboxTable({ emails, onDelete }: { emails: OutboxEmail[]; onDelete: (e: OutboxEmail) => void }) {
  const api = useServerOpsApi();
  const queryClient = useQueryClient();
  const toast = useToast();

  const retry = useMutation({
    mutationFn: (e: OutboxEmail) => api.outbox.retry(e.id),
    meta: { inlineError: true },
    onSuccess: async (e) => {
      toast.success("Email requeued", `${e.subject} → ${e.to}`);
      await queryClient.invalidateQueries({ queryKey: serverOpsKeys.outbox });
    },
    onError: async (err, e) => {
      if (isApiError(err) && err.code === "already_sent") {
        toast.success("Email already sent", e.to);
        await queryClient.invalidateQueries({ queryKey: serverOpsKeys.outbox });
        return;
      }
      toast.error(err, "Could not retry the email");
    },
  });

  return (
    <div className="max-h-[32rem] overflow-auto">
      <Table aria-label="Email outbox">
        <TableHeader className="sticky top-0 bg-card">
          <TableRow className="hover:bg-transparent">
            <TableHead className="pl-6">Recipient</TableHead>
            <TableHead>Subject</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="text-right">Attempts</TableHead>
            <TableHead className="hidden md:table-cell">When</TableHead>
            <TableHead className="hidden lg:table-cell">Last error</TableHead>
            <TableHead className="pr-6 text-right">
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {emails.map((e) => {
            const when = outboxEmailTime(e);
            return (
              <TableRow key={e.id}>
                <TableCell className="max-w-56 truncate pl-6" title={e.to}>
                  {e.to}
                </TableCell>
                <TableCell className="max-w-72 truncate" title={e.subject}>
                  {e.subject}
                </TableCell>
                <TableCell>
                  <OutboxStatusBadge status={e.status} />
                </TableCell>
                <TableCell className="text-right font-mono text-xs tabular-nums">{formatNumber(e.attempts)}</TableCell>
                <TableCell className="hidden whitespace-nowrap md:table-cell">
                  <span className="text-muted-foreground">{when.label}</span>{" "}
                  {when.at ? (
                    <time dateTime={when.at} title={absoluteTime(when.at)}>
                      {relativeTime(when.at)}
                    </time>
                  ) : (
                    "—"
                  )}
                </TableCell>
                <TableCell
                  className="hidden max-w-64 truncate font-mono text-xs text-muted-foreground lg:table-cell"
                  title={e.last_error}
                >
                  {e.last_error || "—"}
                </TableCell>
                <TableCell className="pr-6">
                  <div className="flex justify-end gap-1">
                    {canRetryEmail(e) && (
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={`Retry ${e.id}`}
                        disabled={retry.isPending && retry.variables?.id === e.id}
                        onClick={() => retry.mutate(e)}
                      >
                        <RotateCcw aria-hidden="true" />
                      </Button>
                    )}
                    <Button variant="ghost" size="icon" aria-label={`Delete ${e.id}`} onClick={() => onDelete(e)}>
                      <Trash2 aria-hidden="true" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </div>
  );
}

/**
 * Durable email outbox (§16.11): queued notification emails with retry
 * and dead-lettering. Polls while emails are pending.
 */
export function OutboxCard({ enabled = true }: { enabled?: boolean }) {
  const api = useServerOpsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [filter, setFilter] = useState<Filter>("all");
  const [deleting, setDeleting] = useState<OutboxEmail | null>(null);
  const outbox = useOutbox(filter === "all" ? {} : { status: filter }, enabled);

  const retryDead = useMutation({
    mutationFn: api.outbox.retryDead,
    onSuccess: async ({ retried }) => {
      toast.success("Dead emails requeued", `${formatNumber(retried)} ${retried === 1 ? "email" : "emails"}`);
      await queryClient.invalidateQueries({ queryKey: serverOpsKeys.outbox });
    },
  });
  const remove = useMutation({
    mutationFn: (e: OutboxEmail) => api.outbox.remove(e.id),
    onSuccess: async (_, e) => {
      toast.success("Email deleted", e.id);
      await queryClient.invalidateQueries({ queryKey: serverOpsKeys.outbox });
    },
  });

  const unavailable = isUnavailable(outbox.error);
  const list = outbox.data;

  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle className="flex items-center gap-2">
          <Mail aria-hidden="true" className="size-4" />
          Email outbox
        </CardTitle>
        <CardDescription>
          Notification emails are queued durably and retried with backoff.
          {list ? ` Emails that still fail after ${list.dead_after} are dead-lettered.` : ""}
          {list?.counts.pending ? " Refreshes every 10 s while emails are pending." : ""}
        </CardDescription>
        {!unavailable && (
          <CardAction>
            <Button
              size="sm"
              variant="outline"
              disabled={!list || list.counts.dead === 0 || retryDead.isPending}
              onClick={() => retryDead.mutate()}
            >
              <RotateCcw aria-hidden="true" />
              Retry all dead
            </Button>
          </CardAction>
        )}
      </CardHeader>
      {unavailable ? (
        <CardContent className="py-6">
          <NotImplementedNotice feature="Email outbox">
            This server has no durable email outbox. Upgrade the W0rkTree server to v0.8 to see queued emails.
          </NotImplementedNotice>
        </CardContent>
      ) : outbox.isError ? (
        <ErrorState error={outbox.error} onRetry={() => void outbox.refetch()} />
      ) : !list ? (
        <SkeletonRows rows={4} label="Loading email outbox" />
      ) : (
        <>
          {!list.enabled && (
            <p className="border-b px-6 py-3 text-sm text-muted-foreground">
              Email is not configured (<span className="font-mono">--smtp-url</span>); queued emails are not sent.
            </p>
          )}
          <div className="flex flex-wrap items-center justify-between gap-3 border-b px-6 py-3">
            <OutboxCounts list={list} />
            <Tabs value={filter} onValueChange={(v) => setFilter(v as Filter)}>
              <TabsList aria-label="Filter emails by status">
                {FILTERS.map((f) => (
                  <TabsTrigger key={f.value} value={f.value}>
                    {f.label}
                  </TabsTrigger>
                ))}
              </TabsList>
            </Tabs>
          </div>
          {list.emails.length === 0 ? (
            <EmptyState icon={Mail} title="No emails">
              {EMPTY_TEXT[filter]}
            </EmptyState>
          ) : (
            <OutboxTable emails={list.emails} onDelete={setDeleting} />
          )}
        </>
      )}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title="Delete this email?"
        description={
          <>
            The email to <span className="font-mono">{deleting?.to}</span> is removed from the outbox and will not be sent.
          </>
        }
        confirmLabel="Delete email"
        pending={remove.isPending}
        onConfirm={() => (deleting ? remove.mutateAsync(deleting) : Promise.resolve())}
      />
    </Card>
  );
}
