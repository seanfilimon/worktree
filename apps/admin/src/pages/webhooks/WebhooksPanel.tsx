import { useMutation, useQueryClient } from "@tanstack/react-query";
import { History, KeyRound, Pencil, Plus, Send, Trash2, TriangleAlert, Webhook as WebhookIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import { isUnavailable, useV06Api, useWebhooks, v06Keys, type Webhook, type WebhookScope } from "@/api/v06";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { absoluteTime, relativeTime } from "@/lib/format";
import { ConfirmAction } from "@/pages/iam/shared";
import { useToast } from "@/toast/store";
import { DeliveriesSheet } from "./DeliveriesSheet";
import { WebhookDialog } from "./WebhookDialog";

const SHOWN_EVENTS = 3;

function EventBadges({ events }: { events: readonly string[] }) {
  const shown = events.slice(0, SHOWN_EVENTS);
  return (
    <span className="flex flex-wrap gap-1" title={events.join(", ")}>
      {shown.map((e) => (
        <Badge key={e} variant="outline" className="font-mono">
          {e}
        </Badge>
      ))}
      {events.length > SHOWN_EVENTS && <Badge variant="outline">+{events.length - SHOWN_EVENTS}</Badge>}
    </span>
  );
}

function LastDelivery({ w }: { w: Webhook }) {
  if (!w.last_delivery_at) return <span className="text-muted-foreground">never</span>;
  const ok = (w.last_status ?? 0) >= 200 && (w.last_status ?? 0) < 300;
  return (
    <span className="flex flex-wrap items-center gap-1.5">
      <span className={ok ? "font-mono font-semibold" : "font-mono line-through"}>{w.last_status ? w.last_status : "network error"}</span>
      <time className="text-muted-foreground" dateTime={w.last_delivery_at} title={absoluteTime(w.last_delivery_at)}>
        {relativeTime(w.last_delivery_at)}
      </time>
    </span>
  );
}

interface WebhooksPanelProps {
  scope: WebhookScope;
  editable: boolean;
  /** Text above the table. */
  description?: ReactNode;
}

/** Webhooks of a tenant or tree: list, create (secret shown once), edit, toggle, delete, deliveries, test. */
export function WebhooksPanel({ scope, editable, description }: WebhooksPanelProps) {
  const hooks = useWebhooks(scope);
  const api = useV06Api();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [editing, setEditing] = useState<Webhook | "new" | null>(null);
  const [deleting, setDeleting] = useState<Webhook | null>(null);
  const [viewingId, setViewingId] = useState("");
  const unavailable = hooks.isError && isUnavailable(hooks.error);
  const viewing = hooks.data?.find((w) => w.id === viewingId) ?? null;

  const invalidate = () => queryClient.invalidateQueries({ queryKey: v06Keys.webhooks(scope), exact: true });
  const toggle = useMutation({
    mutationFn: (w: Webhook) => api.webhooks.update(scope, w.id, { active: !w.active }),
    onSuccess: async (w) => {
      toast.success(w.active ? "Webhook activated" : "Webhook deactivated", w.url);
      await invalidate();
    },
  });
  const test = useMutation({
    mutationFn: (w: Webhook) => api.webhooks.test(scope, w.id),
    onSuccess: async (d, w) => {
      toast.success("Test ping sent", d ? `${d.status_code || "no response"} in ${d.duration_ms} ms` : w.url);
      await queryClient.invalidateQueries({ queryKey: v06Keys.deliveries(scope, w.id) });
    },
  });
  const remove = useMutation({
    mutationFn: (w: Webhook) => api.webhooks.remove(scope, w.id),
    meta: { inlineError: true },
    onSuccess: async (_r, w) => {
      toast.success("Webhook deleted", w.url);
      await invalidate();
    },
  });

  return (
    <>
      <Card className="gap-0 overflow-hidden py-0">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b p-3">
          <p className="text-sm text-muted-foreground">
            {description ?? "Webhooks POST signed JSON events to your services when something happens."}
          </p>
          {editable && !unavailable && (
            <Button size="sm" onClick={() => setEditing("new")}>
              <Plus aria-hidden="true" />
              Add webhook
            </Button>
          )}
        </div>
        {hooks.isPending ? (
          <TableSkeleton label="Loading webhooks" rows={3} />
        ) : unavailable ? (
          <div className="p-4">
            <UnavailableNotice feature="Webhooks" />
          </div>
        ) : hooks.isError ? (
          <QueryError error={hooks.error} onRetry={() => void hooks.refetch()} title="Could not load webhooks" />
        ) : hooks.data.length === 0 ? (
          <EmptyState icon={WebhookIcon} title="No webhooks">
            Add one to notify CI, chat or deploy tooling about pushes, merge requests and more.
          </EmptyState>
        ) : (
          <Table aria-label="Webhooks">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Endpoint</TableHead>
                <TableHead className="hidden md:table-cell">Events</TableHead>
                <TableHead>Active</TableHead>
                <TableHead className="hidden sm:table-cell">Last delivery</TableHead>
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {hooks.data.map((w) => (
                <TableRow key={w.id} className={w.active ? undefined : "text-muted-foreground"}>
                  <TableCell className="max-w-72">
                    <p className={w.active ? "truncate font-mono text-sm font-medium" : "truncate font-mono text-sm font-medium line-through"} title={w.url}>
                      {w.url}
                    </p>
                    <p className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                      {w.description && <span className="truncate">{w.description}</span>}
                      {w.has_secret && (
                        <span className="inline-flex items-center gap-1" title="Deliveries are signed">
                          <KeyRound aria-hidden="true" className="size-3" />
                          signed
                        </span>
                      )}
                      {w.failure_count > 0 && (
                        <Badge variant="outline" className="font-semibold">
                          <TriangleAlert aria-hidden="true" />
                          {w.failure_count} failing
                        </Badge>
                      )}
                    </p>
                  </TableCell>
                  <TableCell className="hidden md:table-cell">
                    <EventBadges events={w.events} />
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={w.active}
                      disabled={!editable || (toggle.isPending && toggle.variables.id === w.id)}
                      onCheckedChange={() => toggle.mutate(w)}
                      aria-label={`Active: ${w.url}`}
                    />
                  </TableCell>
                  <TableCell className="hidden text-xs sm:table-cell">
                    <LastDelivery w={w} />
                  </TableCell>
                  <TableCell className="w-px text-right whitespace-nowrap">
                    <Button variant="ghost" size="icon-sm" aria-label={`Deliveries for ${w.url}`} onClick={() => setViewingId(w.id)}>
                      <History aria-hidden="true" />
                    </Button>
                    {editable && (
                      <>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          aria-label={`Send test ping to ${w.url}`}
                          disabled={test.isPending && test.variables.id === w.id}
                          onClick={() => test.mutate(w)}
                        >
                          <Send aria-hidden="true" />
                        </Button>
                        <Button variant="ghost" size="icon-sm" aria-label={`Edit ${w.url}`} onClick={() => setEditing(w)}>
                          <Pencil aria-hidden="true" />
                        </Button>
                        <Button variant="ghost" size="icon-sm" aria-label={`Delete ${w.url}`} onClick={() => setDeleting(w)}>
                          <Trash2 aria-hidden="true" />
                        </Button>
                      </>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>
      {editable && editing !== null && (
        <WebhookDialog key={editing === "new" ? "new" : editing.id} scope={scope} webhook={editing === "new" ? null : editing} onClose={() => setEditing(null)} />
      )}
      <DeliveriesSheet scope={scope} webhook={viewing} editable={editable} onClose={() => setViewingId("")} />
      <ConfirmAction
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        title="Delete this webhook?"
        description={
          <>
            <span className="font-mono break-all">{deleting?.url}</span> stops receiving events and its delivery log is removed.
          </>
        }
        confirmLabel="Delete webhook"
        onConfirm={() => (deleting ? remove.mutateAsync(deleting) : Promise.resolve())}
      />
    </>
  );
}
