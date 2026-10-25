import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, CircleCheck, CircleX, Clock, History, RotateCw, Send } from "lucide-react";
import { useState } from "react";
import {
  isUnavailable,
  useV06Api,
  useWebhookDeliveries,
  useWebhookDelivery,
  v06Keys,
  type Webhook,
  type WebhookDelivery,
  type WebhookScope,
} from "@/api/v06";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { absoluteTime, relativeTime } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";

/** Filled = delivered, outline struck through = failed, outline clock = retry pending. */
export function DeliveryStatusBadge({ d }: { d: Pick<WebhookDelivery, "succeeded" | "status_code" | "next_retry_at"> }) {
  const code = d.status_code > 0 ? String(d.status_code) : "network error";
  if (d.succeeded) {
    return (
      <Badge>
        <CircleCheck aria-hidden="true" />
        {code}
      </Badge>
    );
  }
  return (
    <Badge variant="outline">
      {d.next_retry_at ? <Clock aria-hidden="true" /> : <CircleX aria-hidden="true" />}
      <span className="line-through">{code}</span>
      <span className="sr-only">{d.next_retry_at ? " (failed, retry scheduled)" : " (failed)"}</span>
    </Badge>
  );
}

function prettyPayload(v: unknown): string {
  if (v === undefined || v === null) return "";
  if (typeof v === "string") {
    try {
      return JSON.stringify(JSON.parse(v), null, 2);
    } catch {
      return v;
    }
  }
  return JSON.stringify(v, null, 2);
}

function DeliveryDetail({ scope, webhook, summary, editable }: { scope: WebhookScope; webhook: Webhook; summary: WebhookDelivery; editable: boolean }) {
  const api = useV06Api();
  const queryClient = useQueryClient();
  const toast = useToast();
  const detail = useWebhookDelivery(scope, webhook.id, summary.id);
  const d = detail.data ?? summary;

  const redeliver = useMutation({
    mutationFn: () => api.webhooks.redeliver(scope, webhook.id, summary.id),
    meta: { inlineError: true },
    onSuccess: async () => {
      toast.success("Redelivery queued", summary.event);
      await queryClient.invalidateQueries({ queryKey: v06Keys.deliveries(scope, webhook.id) });
      await queryClient.invalidateQueries({ queryKey: v06Keys.webhooks(scope), exact: true });
    },
  });

  const payload = prettyPayload(d.payload);
  return (
    <div className="flex flex-col gap-3 border-t bg-muted/30 px-4 py-3" aria-label={`Delivery ${summary.id}`} role="group">
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
        <dt className="text-muted-foreground">Delivery</dt>
        <dd className="font-mono break-all">{d.id}</dd>
        <dt className="text-muted-foreground">Attempts</dt>
        <dd className="tabular-nums">{d.attempts}</dd>
        {d.delivered_at && (
          <>
            <dt className="text-muted-foreground">Delivered</dt>
            <dd>
              <time dateTime={d.delivered_at}>{absoluteTime(d.delivered_at)}</time>
            </dd>
          </>
        )}
        {d.next_retry_at && (
          <>
            <dt className="text-muted-foreground">Next retry</dt>
            <dd>
              <time dateTime={d.next_retry_at} title={absoluteTime(d.next_retry_at)}>
                {relativeTime(d.next_retry_at)}
              </time>
            </dd>
          </>
        )}
        {d.error && (
          <>
            <dt className="text-muted-foreground">Error</dt>
            <dd className="font-mono font-semibold break-all">{d.error}</dd>
          </>
        )}
      </dl>
      {detail.isError && !isUnavailable(detail.error) && <QueryError error={detail.error} title="Could not load the delivery" />}
      {payload && (
        <div className="grid gap-1">
          <h4 className="text-xs font-semibold">Payload</h4>
          <pre aria-label="Payload" className="max-h-64 overflow-auto rounded-md border bg-background p-2 font-mono text-[11px] leading-relaxed">
            {payload}
          </pre>
        </div>
      )}
      <div className="grid gap-1">
        <h4 className="text-xs font-semibold">Response</h4>
        {d.response ? (
          <pre aria-label="Response" className="max-h-48 overflow-auto rounded-md border bg-background p-2 font-mono text-[11px] leading-relaxed whitespace-pre-wrap">
            {d.response}
          </pre>
        ) : (
          <p className="text-xs text-muted-foreground">No response body.</p>
        )}
      </div>
      <FormError error={redeliver.error} />
      {editable && (
        <div>
          <Button size="sm" variant="outline" disabled={redeliver.isPending} onClick={() => redeliver.mutate()}>
            <RotateCw aria-hidden="true" />
            {redeliver.isPending ? "Redelivering…" : "Redeliver"}
          </Button>
        </div>
      )}
    </div>
  );
}

interface DeliveriesSheetProps {
  scope: WebhookScope;
  webhook: Webhook | null;
  editable: boolean;
  onClose: () => void;
}

/** Recent deliveries of one webhook with status, duration and response; redeliver and test ping. */
export function DeliveriesSheet({ scope, webhook, editable, onClose }: DeliveriesSheetProps) {
  const api = useV06Api();
  const queryClient = useQueryClient();
  const toast = useToast();
  const deliveries = useWebhookDeliveries(scope, webhook?.id ?? "", webhook !== null);
  const [openId, setOpenId] = useState("");

  const test = useMutation({
    mutationFn: (w: Webhook) => api.webhooks.test(scope, w.id),
    meta: { inlineError: true },
    onSuccess: async (d, w) => {
      toast.success("Test ping sent", d ? `${d.status_code || "no response"} in ${d.duration_ms} ms` : w.url);
      await queryClient.invalidateQueries({ queryKey: v06Keys.deliveries(scope, w.id) });
    },
  });

  return (
    <Sheet open={webhook !== null} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-2xl">
        <SheetHeader className="border-b">
          <SheetTitle className="flex items-center gap-2">
            <History aria-hidden="true" className="size-4" />
            Recent deliveries
          </SheetTitle>
          <SheetDescription className="font-mono break-all">{webhook?.url}</SheetDescription>
          {webhook && editable && (
            <div className="pt-2">
              <Button size="sm" variant="outline" disabled={test.isPending} onClick={() => test.mutate(webhook)}>
                <Send aria-hidden="true" />
                {test.isPending ? "Sending…" : "Send test ping"}
              </Button>
            </div>
          )}
        </SheetHeader>
        {test.isError && (
          <div className="p-4">
            {isUnavailable(test.error) ? <UnavailableNotice feature="Webhook test" /> : <FormError error={test.error} />}
          </div>
        )}
        {webhook &&
          (deliveries.isPending ? (
            <TableSkeleton label="Loading deliveries" rows={4} />
          ) : deliveries.isError ? (
            isUnavailable(deliveries.error) ? (
              <div className="p-4">
                <UnavailableNotice feature="Webhook deliveries" />
              </div>
            ) : (
              <QueryError error={deliveries.error} onRetry={() => void deliveries.refetch()} title="Could not load deliveries" />
            )
          ) : deliveries.data.length === 0 ? (
            <EmptyState icon={History} title="No deliveries yet">
              Deliveries appear here once a matching event happens. Send a test ping to try the endpoint.
            </EmptyState>
          ) : (
            <ul aria-label="Deliveries" className="divide-y">
              {deliveries.data.map((d) => {
                const open = openId === d.id;
                return (
                  <li key={d.id}>
                    <button
                      type="button"
                      aria-expanded={open}
                      onClick={() => setOpenId(open ? "" : d.id)}
                      className="flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2.5 text-left text-sm hover:bg-muted/50 focus-visible:bg-muted/50 focus-visible:outline-none"
                    >
                      <ChevronRight aria-hidden="true" className={cn("size-4 shrink-0 transition-transform", open && "rotate-90")} />
                      <DeliveryStatusBadge d={d} />
                      <span className="min-w-0 flex-1 truncate font-mono text-xs font-medium">{d.event}</span>
                      {d.redelivery && <Badge variant="outline">redelivery</Badge>}
                      <span className="text-xs text-muted-foreground tabular-nums">{d.duration_ms} ms</span>
                      {d.attempts > 1 && <span className="text-xs text-muted-foreground tabular-nums">{d.attempts} attempts</span>}
                      <time className="text-xs text-muted-foreground" dateTime={d.created_at} title={absoluteTime(d.created_at)}>
                        {relativeTime(d.created_at)}
                      </time>
                    </button>
                    {open && <DeliveryDetail scope={scope} webhook={webhook} summary={d} editable={editable} />}
                  </li>
                );
              })}
            </ul>
          ))}
      </SheetContent>
    </Sheet>
  );
}
