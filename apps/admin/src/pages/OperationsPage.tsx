import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FlaskConical, History, Recycle, Settings2, ShieldX, Trash2 } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { useTenants } from "@/api/queries";
import { isNotImplemented, opsKeys, useGcStatus, useOpsApi, type GCConfig, type GCReport, type GCStatus } from "@/api/v05ops";
import { useAuth } from "@/auth/context";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/dashboard/states";
import { PageHeader } from "@/components/PageHeader";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { FormError } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, formatBytes, formatDuration, formatNumber, relativeTime } from "@/lib/format";
import { NotImplementedNotice } from "@/pages/tenants/opsShared";
import { TenantLimitsCard } from "@/pages/tenants/TenantLimitsCard";
import { useToast } from "@/toast/store";

const breadcrumbs = [{ label: "Operations" }];

/** Wall-clock duration of a run, e.g. "1.2 s". */
function runDuration(r: GCReport): string {
  const ms = Date.parse(r.finished_at) - Date.parse(r.started_at);
  if (!Number.isFinite(ms) || ms < 0) return "—";
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return formatDuration(ms / 1000);
}

const CONFIG_LABELS: Record<string, string> = {
  gc_interval: "GC interval",
  interval: "GC interval",
  gc_grace: "Grace period",
  grace: "Grace period",
  staged_retention: "Staged retention",
  audit_retention: "Audit retention",
  rate_limit: "Rate limit (req/min)",
  default_max_trees: "Default max trees",
  default_max_storage: "Default max storage",
  default_max_storage_bytes: "Default max storage",
};

function configLabel(key: string): string {
  return CONFIG_LABELS[key] ?? key.replace(/_/g, " ").replace(/^\w/, (c) => c.toUpperCase());
}

const DURATION_KEY = /interval|grace|retention|duration|period/;
const BYTES_KEY = /storage|bytes/;

/** Renders a config value: Go durations (ns numbers or strings), sizes, flags. */
function formatConfigValue(key: string, value: unknown): string {
  if (value === null || value === undefined || value === "") return "—";
  if (typeof value === "boolean") return value ? "yes" : "no";
  if (typeof value === "number") {
    if (DURATION_KEY.test(key)) {
      if (value === 0) return key.includes("interval") ? "disabled" : "keep forever";
      // time.Duration marshals as nanoseconds.
      return formatDuration(value / 1e9);
    }
    if (BYTES_KEY.test(key)) return value === 0 ? "unlimited" : formatBytes(value);
    if (key.startsWith("default_max") || key === "rate_limit") return value === 0 ? "unlimited" : formatNumber(value);
    return formatNumber(value);
  }
  if (typeof value === "string") {
    if (DURATION_KEY.test(key) && /^0s?$/.test(value)) return key.includes("interval") ? "disabled" : "keep forever";
    return value;
  }
  return JSON.stringify(value);
}

function ReportStats({ report }: { report: GCReport }) {
  const items: [string, string][] = [
    ["Scanned", formatNumber(report.scanned)],
    ["Reachable", formatNumber(report.reachable)],
    ["Unreachable", formatNumber(report.unreachable)],
    ["Newly marked", formatNumber(report.marked)],
    ["Deleted", formatNumber(report.deleted)],
    ["Freed", formatBytes(report.freed_bytes)],
  ];
  return (
    <section aria-label="GC report" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        {report.dry_run ? (
          <Badge variant="outline">
            <FlaskConical aria-hidden="true" />
            Dry run
          </Badge>
        ) : (
          <Badge>
            <Trash2 aria-hidden="true" />
            Collection
          </Badge>
        )}
        <span className="text-muted-foreground">
          <time dateTime={report.started_at} title={absoluteTime(report.started_at)}>
            {relativeTime(report.started_at)}
          </time>{" "}
          · took {runDuration(report)}
        </span>
      </div>
      <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-md border bg-border sm:grid-cols-3 lg:grid-cols-6">
        {items.map(([label, value]) => (
          <div key={label} className="flex flex-col gap-0.5 bg-card px-4 py-3">
            <dt className="text-xs text-muted-foreground">{label}</dt>
            <dd className="font-mono text-lg font-semibold tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>
      {report.dry_run && (
        <p className="text-xs text-muted-foreground">
          A dry run changes nothing: it reports what a collection would mark and delete.
        </p>
      )}
    </section>
  );
}

function GcCard({ status }: { status: GCStatus | undefined }) {
  const ops = useOpsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [confirming, setConfirming] = useState(false);
  const [last, setLast] = useState<GCReport | null>(null);

  const run = useMutation({
    mutationFn: (dryRun: boolean) => ops.gc.run(dryRun),
    meta: { inlineError: true },
    onSuccess: async (report) => {
      setLast(report);
      toast.success(
        report.dry_run ? "Dry run finished" : "Garbage collection finished",
        `${formatNumber(report.unreachable)} unreachable · ${formatNumber(report.deleted)} deleted`,
      );
      await queryClient.invalidateQueries({ queryKey: opsKeys.gc });
    },
  });

  const shown = last ?? status?.runs[0] ?? null;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Recycle aria-hidden="true" className="size-4" />
          Garbage collection
        </CardTitle>
        <CardDescription>
          Marks objects unreachable from every branch, tag, staged snapshot, merge request and release; deletes them once the
          grace period has passed. Also prunes expired staged snapshots and audit events.
        </CardDescription>
        <CardAction className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" disabled={run.isPending} onClick={() => run.mutate(true)}>
            <FlaskConical aria-hidden="true" />
            {run.isPending && run.variables ? "Running…" : "Dry run"}
          </Button>
          <Button size="sm" disabled={run.isPending} onClick={() => setConfirming(true)}>
            <Trash2 aria-hidden="true" />
            {run.isPending && run.variables === false ? "Running…" : "Run GC"}
          </Button>
        </CardAction>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {isNotImplemented(run.error) ? (
          <NotImplementedNotice feature="Garbage collection" />
        ) : (
          <FormError error={run.error} />
        )}
        {shown ? (
          <>
            {!last && <p className="text-xs font-medium text-muted-foreground">Latest run</p>}
            <ReportStats report={shown} />
          </>
        ) : (
          <p className="text-sm text-muted-foreground">No run yet. Start with a dry run to see what would be collected.</p>
        )}
      </CardContent>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title="Run garbage collection now?"
        description="Unreachable objects past the grace period are deleted permanently. Newly unreachable objects are only marked."
        confirmLabel="Run GC"
        pending={run.isPending}
        onConfirm={() => run.mutateAsync(false)}
      />
    </Card>
  );
}

function ConfigCard({ config }: { config: GCConfig }) {
  // The server sends both "interval" ("24h0m0s") and "interval_seconds"
  // (86400); show only the readable form when both exist.
  const entries = Object.entries(config).filter(
    ([key]) => !(key.endsWith("_seconds") && key.slice(0, -"_seconds".length) in config),
  );
  return (
    <Card className="gap-0 pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle className="flex items-center gap-2">
          <Settings2 aria-hidden="true" className="size-4" />
          Server configuration
        </CardTitle>
        <CardDescription>
          Set with server flags (<span className="font-mono">--gc-interval</span>, <span className="font-mono">--gc-grace</span>,{" "}
          <span className="font-mono">--staged-retention</span>, <span className="font-mono">--audit-retention</span>, …).
        </CardDescription>
      </CardHeader>
      {entries.length === 0 ? (
        <EmptyState title="No configuration reported" />
      ) : (
        <dl aria-label="Server configuration" className="divide-y text-sm">
          {entries.map(([k, v]) => (
            <div key={k} className="flex items-center justify-between gap-4 px-6 py-2.5">
              <dt className="text-muted-foreground" title={k}>
                {configLabel(k)}
              </dt>
              <dd className="font-mono text-xs">{formatConfigValue(k, v)}</dd>
            </div>
          ))}
        </dl>
      )}
    </Card>
  );
}

function HistoryCard({ runs }: { runs: GCReport[] }) {
  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle className="flex items-center gap-2">
          <History aria-hidden="true" className="size-4" />
          Run history
        </CardTitle>
        <CardDescription>The last {runs.length > 0 ? runs.length : 20} runs, newest first.</CardDescription>
      </CardHeader>
      {runs.length === 0 ? (
        <EmptyState icon={History} title="No runs recorded" />
      ) : (
        <div className="max-h-96 overflow-auto">
          <Table aria-label="GC runs">
            <TableHeader className="sticky top-0 bg-card">
              <TableRow className="hover:bg-transparent">
                <TableHead className="pl-6">Started</TableHead>
                <TableHead>Mode</TableHead>
                <TableHead className="text-right">Unreachable</TableHead>
                <TableHead className="hidden text-right sm:table-cell">Marked</TableHead>
                <TableHead className="text-right">Deleted</TableHead>
                <TableHead className="hidden text-right md:table-cell">Freed</TableHead>
                <TableHead className="hidden pr-6 text-right lg:table-cell">Took</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {runs.map((r) => (
                <TableRow key={r.started_at}>
                  <TableCell className="pl-6">
                    <time dateTime={r.started_at} title={absoluteTime(r.started_at)}>
                      {relativeTime(r.started_at)}
                    </time>
                  </TableCell>
                  <TableCell>{r.dry_run ? <Badge variant="outline">dry run</Badge> : <Badge>collect</Badge>}</TableCell>
                  <TableCell className="text-right font-mono text-xs tabular-nums">{formatNumber(r.unreachable)}</TableCell>
                  <TableCell className="hidden text-right font-mono text-xs tabular-nums sm:table-cell">
                    {formatNumber(r.marked)}
                  </TableCell>
                  <TableCell className="text-right font-mono text-xs font-semibold tabular-nums">{formatNumber(r.deleted)}</TableCell>
                  <TableCell className="hidden text-right font-mono text-xs tabular-nums md:table-cell">
                    {formatBytes(r.freed_bytes)}
                  </TableCell>
                  <TableCell className="hidden pr-6 text-right font-mono text-xs tabular-nums lg:table-cell">
                    {runDuration(r)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </Card>
  );
}

function TenantQuotas() {
  const tenants = useTenants();
  const [picked, setPicked] = useState("");
  const slug = picked || tenants.data?.[0]?.slug || "";

  return (
    <section aria-labelledby="ops-tenants" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 id="ops-tenants" className="text-lg font-semibold tracking-tight">
            Tenant usage and limits
          </h2>
          <p className="text-sm text-muted-foreground">Per-tenant quotas override the server defaults.</p>
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="ops-tenant">Tenant</Label>
          <Select value={slug} onValueChange={setPicked} disabled={!tenants.data?.length}>
            <SelectTrigger id="ops-tenant" className="w-52">
              <SelectValue placeholder="Select a tenant" />
            </SelectTrigger>
            <SelectContent>
              {(tenants.data ?? []).map((t) => (
                <SelectItem key={t.id} value={t.slug}>
                  {t.slug}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      {tenants.isError ? (
        <ErrorState className="m-0" error={tenants.error} onRetry={() => void tenants.refetch()} />
      ) : slug ? (
        <>
          <TenantLimitsCard key={slug} slug={slug} editable />
          <p className="text-xs text-muted-foreground">
            Status (suspend / deactivate) is on the tenant's{" "}
            <Link className="underline underline-offset-4" to={`/tenants/${encodeURIComponent(slug)}?tab=settings`}>
              settings tab
            </Link>
            .
          </p>
        </>
      ) : (
        tenants.isPending && (
          <Card className="py-0">
            <SkeletonRows rows={3} label="Loading tenants" />
          </Card>
        )
      )}
    </section>
  );
}

/** Garbage collection, retention, storage and limits (server administrators). */
export function OperationsPage() {
  const { isAdmin } = useAuth();
  const gc = useGcStatus(isAdmin);

  if (!isAdmin) {
    return (
      <>
        <PageHeader title="Operations" breadcrumbs={breadcrumbs} />
        <Card className="py-0">
          <EmptyState icon={ShieldX} title="Server administrators only">
            Garbage collection, retention and tenant limits are restricted to server administrators.
          </EmptyState>
        </Card>
      </>
    );
  }

  const gcUnavailable = isNotImplemented(gc.error);

  return (
    <>
      <PageHeader
        title="Operations"
        breadcrumbs={breadcrumbs}
        description="Garbage collection, retention and per-tenant quotas."
      />
      <div className="flex flex-col gap-6">
        {gcUnavailable ? (
          <NotImplementedNotice feature="Garbage collection" />
        ) : gc.isError ? (
          <ErrorState className="m-0" error={gc.error} onRetry={() => void gc.refetch()} />
        ) : gc.isPending ? (
          <Card className="py-0">
            <SkeletonRows rows={5} label="Loading garbage collection status" />
          </Card>
        ) : (
          <>
            <GcCard status={gc.data} />
            <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
              <ConfigCard config={gc.data.config} />
              <HistoryCard runs={gc.data.runs} />
            </div>
          </>
        )}
        <TenantQuotas />
      </div>
    </>
  );
}
