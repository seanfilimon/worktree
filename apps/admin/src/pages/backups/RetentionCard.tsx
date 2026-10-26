import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Eraser, Play } from "lucide-react";
import { backupKeys } from "@/api/backups";
import {
  isUnavailable,
  retentionReportText,
  retentionRuleText,
  serverOpsKeys,
  useRetention,
  useServerOpsApi,
  type RetentionStatus,
} from "@/api/serverops";
import { ErrorState, SkeletonRows } from "@/components/dashboard/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { absoluteTime, formatNumber, relativeTime } from "@/lib/format";
import { NotImplementedNotice } from "@/pages/tenants/opsShared";
import { useToast } from "@/toast/store";

const Flag = ({ children }: { children: string }) => <span className="font-mono">{children}</span>;

function policyRows(s: RetentionStatus): [string, string][] {
  const c = s.config;
  return [
    ["Policy versions", retentionRuleText(c.policy_versions, "per policy")],
    ["Audit events", retentionRuleText(c.audit)],
    ["Webhook deliveries", retentionRuleText(c.webhook_deliveries, "per webhook")],
    ["Scheduled backups", retentionRuleText(c.backups, "backups")],
  ];
}

function intervalText(interval: string): string {
  return !interval || interval === "0s" ? "Off" : `Every ${interval}`;
}

function RetentionBody({ status }: { status: RetentionStatus }) {
  const report = status.last_report;
  const lastRun = status.last_run_at ?? report?.at;
  return (
    <>
      {status.last_error && (
        <p role="alert" className="border-b px-6 py-3 text-sm">
          <span className="font-medium">Last retention pass failed:</span>{" "}
          <span className="font-mono text-xs text-muted-foreground">{status.last_error}</span>
        </p>
      )}
      <dl aria-label="Retention policy" className="divide-y text-sm">
        {policyRows(status).map(([k, v]) => (
          <div key={k} className="flex items-center justify-between gap-4 px-6 py-2.5">
            <dt className="text-muted-foreground">{k}</dt>
            <dd className="truncate text-right font-mono text-xs" title={v}>
              {v}
            </dd>
          </div>
        ))}
      </dl>
      <dl aria-label="Retention runs" className="divide-y border-t text-sm">
        <div className="flex items-center justify-between gap-4 px-6 py-2.5">
          <dt className="text-muted-foreground">Schedule</dt>
          <dd className="font-mono text-xs">{intervalText(status.interval)}</dd>
        </div>
        <div className="flex items-center justify-between gap-4 px-6 py-2.5">
          <dt className="text-muted-foreground">Last run</dt>
          <dd className="text-right text-xs">
            {lastRun ? (
              <time dateTime={lastRun} title={absoluteTime(lastRun)}>
                {relativeTime(lastRun)}
              </time>
            ) : (
              "Never"
            )}
          </dd>
        </div>
        {report && (
          <div className="flex items-center justify-between gap-4 px-6 py-2.5">
            <dt className="text-muted-foreground">Pruned</dt>
            <dd className="text-right font-mono text-xs">
              {retentionReportText(report)} · took {formatNumber(report.took_ms)} ms
            </dd>
          </div>
        )}
      </dl>
    </>
  );
}

/**
 * Data retention (§16.11): what the hourly pass prunes, its last run and
 * a "Run now" action.
 */
export function RetentionCard({ enabled = true }: { enabled?: boolean }) {
  const api = useServerOpsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const retention = useRetention(enabled);

  const run = useMutation({
    mutationFn: api.retention.run,
    onSuccess: async (r) => {
      toast.success("Retention pass finished", `Pruned ${retentionReportText(r)} in ${formatNumber(r.took_ms)} ms`);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: serverOpsKeys.retention }),
        queryClient.invalidateQueries({ queryKey: backupKeys.status }),
      ]);
    },
  });

  const unavailable = isUnavailable(retention.error);
  const status = retention.data;

  return (
    <Card className="gap-0 pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle className="flex items-center gap-2">
          <Eraser aria-hidden="true" className="size-4" />
          Retention
          {status?.running && <Badge variant="outline">Running</Badge>}
        </CardTitle>
        <CardDescription>
          Old data is pruned on a schedule. Set with <Flag>--policy-version-retention</Flag>, <Flag>--audit-retention</Flag>,{" "}
          <Flag>--webhook-delivery-retention</Flag> / <Flag>--webhook-delivery-keep</Flag> and <Flag>--backup-max-age</Flag> /{" "}
          <Flag>--backup-keep</Flag>.
        </CardDescription>
        {!unavailable && (
          <CardAction>
            <Button size="sm" variant="outline" disabled={!status || status.running || run.isPending} onClick={() => run.mutate()}>
              <Play aria-hidden="true" />
              {run.isPending ? "Running…" : "Run now"}
            </Button>
          </CardAction>
        )}
      </CardHeader>
      {unavailable ? (
        <CardContent className="py-6">
          <NotImplementedNotice feature="Retention">
            This server does not prune old data. Upgrade the W0rkTree server to v0.8 to configure retention.
          </NotImplementedNotice>
        </CardContent>
      ) : retention.isError ? (
        <ErrorState error={retention.error} onRetry={() => void retention.refetch()} />
      ) : !status ? (
        <SkeletonRows rows={4} label="Loading retention" />
      ) : (
        <RetentionBody status={status} />
      )}
    </Card>
  );
}
