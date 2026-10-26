/**
 * Server-wide secret scanning and license compliance (docs/ENGINEERING_PLAN.md
 * §16.7): totals, scanning modes, per-tree findings and license rule health,
 * and the latest open findings. Server administrators only
 * (GET /api/v1/admin/compliance).
 */
import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, ShieldAlert } from "lucide-react";
import { Link } from "react-router";
import { useApi } from "@/api/context";
import { isUnavailable, retryV06 } from "@/api/v06";
import { PageHeader } from "@/components/PageHeader";
import {
  EmptyState,
  QueryError,
  TableSkeleton,
} from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { absoluteTime, formatNumber, relativeTime } from "@/lib/format";
import {
  complianceKeys,
  fetchComplianceSummary,
  MODE_LABELS,
  type ComplianceSummary,
  type ScanMode,
} from "@/pages/tree/complianceApi";

const breadcrumbs = [{ label: "Compliance" }];

const seg = encodeURIComponent;

function treeLink(tenant: string, tree: string): string {
  return `/trees/${seg(tenant)}/${seg(tree)}/compliance`;
}

function StatCard({
  title,
  value,
  detail,
}: {
  title: string;
  value: string;
  detail: string;
}) {
  return (
    <Card className="gap-1 py-4">
      <CardHeader className="px-4">
        <CardDescription>{title}</CardDescription>
        <CardTitle className="text-2xl tabular-nums">{value}</CardTitle>
        <p className="text-xs text-muted-foreground">{detail}</p>
      </CardHeader>
    </Card>
  );
}

function ModeBadge({ mode }: { mode: ScanMode }) {
  return (
    <Badge
      variant={
        mode === "block" ? "default" : mode === "off" ? "secondary" : "outline"
      }
    >
      {MODE_LABELS[mode]?.label ?? mode}
    </Badge>
  );
}

function Summary({ s }: { s: ComplianceSummary }) {
  return (
    <div className="grid gap-4">
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard
          title="Open findings"
          value={formatNumber(s.open_findings)}
          detail={`${formatNumber(s.resolved_findings)} resolved`}
        />
        <StatCard
          title="Scanning modes"
          value={`${formatNumber(s.modes.block)} block`}
          detail={`${formatNumber(s.modes.warn)} warn · ${formatNumber(s.modes.off)} off · ${formatNumber(s.trees)} trees`}
        />
        <StatCard
          title="Allowlisted secrets"
          value={formatNumber(s.allowlisted)}
          detail={`${formatNumber(s.scan_rules)} scan rules (rule set v${s.rules_version})`}
        />
        <StatCard
          title="License rules failing the SPDX check"
          value={formatNumber(s.invalid_rules)}
          detail={`of ${formatNumber(s.license_rules)} rules · SPDX License List ${s.spdx_list_version}`}
        />
      </div>

      <Card className="gap-0 overflow-hidden py-0">
        <div className="border-b p-3">
          <h2 className="font-semibold">Trees</h2>
          <p className="text-xs text-muted-foreground">
            Most open findings first.
          </p>
        </div>
        {s.tree_list.length === 0 ? (
          <p className="p-4 text-sm text-muted-foreground">
            There are no trees on this server yet.
          </p>
        ) : (
          <Table aria-label="Trees">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>Tree</TableHead>
                <TableHead>Mode</TableHead>
                <TableHead className="text-right">Open</TableHead>
                <TableHead className="hidden text-right sm:table-cell">
                  Blocked
                </TableHead>
                <TableHead className="hidden text-right md:table-cell">
                  Resolved
                </TableHead>
                <TableHead className="hidden text-right md:table-cell">
                  Allowlisted
                </TableHead>
                <TableHead className="hidden text-right lg:table-cell">
                  License rules
                </TableHead>
                <TableHead className="hidden lg:table-cell">
                  Last finding
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {s.tree_list.map((t) => (
                <TableRow key={t.tree_id}>
                  <TableCell>
                    <Link
                      to={treeLink(t.tenant, t.tree)}
                      className="font-mono text-sm font-medium underline-offset-4 hover:underline"
                    >
                      {t.tenant}/{t.tree}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <ModeBadge mode={t.mode} />
                  </TableCell>
                  <TableCell
                    className={
                      t.open_findings > 0
                        ? "text-right font-bold tabular-nums"
                        : "text-right tabular-nums"
                    }
                  >
                    {formatNumber(t.open_findings)}
                  </TableCell>
                  <TableCell className="hidden text-right tabular-nums sm:table-cell">
                    {formatNumber(t.blocked_findings)}
                  </TableCell>
                  <TableCell className="hidden text-right tabular-nums md:table-cell">
                    {formatNumber(t.resolved_findings)}
                  </TableCell>
                  <TableCell className="hidden text-right tabular-nums md:table-cell">
                    {formatNumber(t.allowlisted)}
                  </TableCell>
                  <TableCell className="hidden text-right tabular-nums lg:table-cell">
                    {formatNumber(t.license_rules)}
                    {t.invalid_rules > 0 && (
                      <span className="font-semibold">
                        {" "}
                        ({formatNumber(t.invalid_rules)} invalid)
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="hidden text-xs text-muted-foreground lg:table-cell">
                    {t.last_finding_at ? (
                      <time
                        dateTime={t.last_finding_at}
                        title={absoluteTime(t.last_finding_at)}
                      >
                        {relativeTime(t.last_finding_at)}
                      </time>
                    ) : (
                      "—"
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>

      <Card className="gap-0 overflow-hidden py-0">
        <div className="border-b p-3">
          <h2 className="font-semibold">Latest open findings</h2>
        </div>
        {s.recent.length === 0 ? (
          <EmptyState icon={CheckCircle2} title="No open findings">
            No possible secrets are waiting for review.
          </EmptyState>
        ) : (
          <Table aria-label="Latest open findings">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>Tree</TableHead>
                <TableHead>Location</TableHead>
                <TableHead className="hidden md:table-cell">Excerpt</TableHead>
                <TableHead>Push</TableHead>
                <TableHead className="hidden sm:table-cell">Seen</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {s.recent.map((f) => (
                <TableRow key={`${f.tree_id}/${f.id}`}>
                  <TableCell>
                    <Link
                      to={treeLink(f.tenant, f.tree_name)}
                      className="font-mono text-sm underline-offset-4 hover:underline"
                    >
                      {f.tenant}/{f.tree_name}
                    </Link>
                  </TableCell>
                  <TableCell className="max-w-64">
                    <p
                      className="truncate font-mono text-xs"
                      title={`${f.path}:${f.line}`}
                    >
                      {f.path}:{f.line}
                    </p>
                    <p className="text-xs text-muted-foreground">{f.rule}</p>
                  </TableCell>
                  <TableCell className="hidden max-w-80 md:table-cell">
                    <code
                      className="line-clamp-1 font-mono text-xs break-all"
                      title={f.excerpt}
                    >
                      {f.excerpt}
                    </code>
                  </TableCell>
                  <TableCell>
                    <Badge
                      variant={f.action === "blocked" ? "default" : "outline"}
                    >
                      {f.action}
                    </Badge>
                  </TableCell>
                  <TableCell className="hidden text-xs text-muted-foreground sm:table-cell">
                    {relativeTime(f.last_seen_at)} · {f.pushed_by || "unknown"}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>
    </div>
  );
}

/** Server-wide secret-scan findings and license compliance (§16.7). */
export function CompliancePage() {
  const { client } = useApi();
  const summary = useQuery({
    queryKey: complianceKeys.summary,
    queryFn: () => fetchComplianceSummary(client),
    retry: retryV06,
  });
  return (
    <div>
      <PageHeader
        title={
          <span className="flex items-center gap-2">
            <ShieldAlert aria-hidden="true" className="size-6" />
            Compliance
          </span>
        }
        documentTitle="Compliance"
        breadcrumbs={breadcrumbs}
        description="Secret scanning of pushes and license rule health across every tree."
      />
      {summary.isPending ? (
        <TableSkeleton
          label="Loading the compliance summary"
          rows={4}
          columns={5}
        />
      ) : summary.isError ? (
        isUnavailable(summary.error) ? (
          <UnavailableNotice feature="Secret scanning and license compliance" />
        ) : (
          <QueryError
            error={summary.error}
            onRetry={() => void summary.refetch()}
            title="Could not load the compliance summary"
          />
        )
      ) : (
        <Summary s={summary.data} />
      )}
    </div>
  );
}
