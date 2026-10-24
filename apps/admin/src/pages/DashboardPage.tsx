import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useApi } from "@/api/context";
import { keys, useAudit, useHealth, useStats } from "@/api/queries";
import type { ServerEvent, ServerStats } from "@/api/types";
import { useServerEvents } from "@/api/useServerEvents";
import { useAuth } from "@/auth/context";
import { LiveIndicator } from "@/components/LiveIndicator";
import { PageHeader } from "@/components/PageHeader";
import { Card, Section } from "@/components/legacy/Card";
import { EmptyState, ErrorState, Skeleton, SkeletonRows } from "@/components/legacy/States";
import { RelativeTime } from "@/components/legacy/Time";
import { describeEvent } from "@/lib/describeEvent";
import { formatBytes, formatDuration, formatNumber } from "@/lib/format";

const MAX_FEED = 50;

const STAT_CARDS: { key: keyof ServerStats; label: string; format?: (n: number) => string }[] = [
  { key: "tenants", label: "Tenants" },
  { key: "accounts", label: "Accounts" },
  { key: "trees", label: "Trees" },
  { key: "branches", label: "Branches" },
  { key: "snapshots", label: "Snapshots" },
  { key: "staged", label: "Staged WIP" },
  { key: "objects", label: "Objects" },
  { key: "storage_bytes", label: "Storage", format: formatBytes },
];

function StatsGrid({ stats, loading }: { stats: ServerStats | undefined; loading: boolean }) {
  return (
    <ul aria-label="Server statistics" className="grid grid-cols-2 gap-3 sm:grid-cols-4">
      {STAT_CARDS.map(({ key, label, format = formatNumber }) => (
        <li key={key}>
          <Card className="px-4 py-3">
            <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{label}</p>
            {loading || !stats ? (
              <Skeleton className="mt-1.5 h-7 w-16" />
            ) : (
              <p className="mt-0.5 font-mono text-2xl font-semibold tabular-nums">{format(stats[key])}</p>
            )}
          </Card>
        </li>
      ))}
    </ul>
  );
}

interface FeedItem {
  id: number;
  event: ServerEvent;
}

function ActivityFeed({ items }: { items: FeedItem[] }) {
  if (items.length === 0) {
    return (
      <EmptyState title="Waiting for activity">
        Pushes, staged work, tags and merge requests appear here in real time.
      </EmptyState>
    );
  }
  return (
    <ol aria-label="Live activity" className="divide-y divide-border">
      {items.map(({ id, event }) => {
        const { action, target } = describeEvent(event);
        return (
          <li key={id} className="flex items-baseline gap-3 px-4 py-2.5 text-[13px]">
            <span className="w-28 shrink-0 truncate font-mono text-xs text-subtle">{event.type}</span>
            <span className="min-w-0 flex-1">
              <span className="font-medium">{event.actor || "system"}</span> {action}
              {target && <span className="text-muted-foreground"> — {target}</span>}
            </span>
            <RelativeTime value={event.at} className="shrink-0 text-xs text-subtle" />
          </li>
        );
      })}
    </ol>
  );
}

function RecentPushes() {
  const pushes = useAudit({ action: "branch.push", limit: 10 });
  if (pushes.isPending) return <SkeletonRows rows={4} label="Loading recent pushes" />;
  if (pushes.isError) return <ErrorState error={pushes.error} onRetry={() => void pushes.refetch()} />;
  if (pushes.data.length === 0) return <EmptyState title="No pushes yet">Run <code className="font-mono">wt push</code> to publish a branch.</EmptyState>;
  return (
    <ol aria-label="Recent pushes" className="divide-y divide-border">
      {pushes.data.map((e) => (
        <li key={e.id} className="flex items-baseline gap-3 px-4 py-2.5 text-[13px]">
          <span className="min-w-0 flex-1 truncate">
            <span className="font-medium">{e.actor}</span> pushed{" "}
            <span className="font-mono">{e.details?.branch ?? e.resource_id}</span>
            {e.details?.tree && <span className="text-muted-foreground"> to {e.details.tree}</span>}
          </span>
          <RelativeTime value={e.at} className="shrink-0 text-xs text-subtle" />
        </li>
      ))}
    </ol>
  );
}

export function DashboardPage() {
  const api = useApi();
  const { isAdmin, account, me } = useAuth();
  const health = useHealth();
  const stats = useStats(isAdmin);
  const queryClient = useQueryClient();
  const [feed, setFeed] = useState<FeedItem[]>([]);
  const nextId = useRef(1);

  const status = useServerEvents({}, (event) => {
    const id = nextId.current++;
    setFeed((items) => [{ id, event }, ...items].slice(0, MAX_FEED));
    if (event.type === "branch.pushed") void queryClient.invalidateQueries({ queryKey: ["audit"] });
    void queryClient.invalidateQueries({ queryKey: keys.stats });
  });

  const memberships = me.data?.memberships ?? [];

  return (
    <>
      <PageHeader
        title="Dashboard"
        description={
          health.data ? (
            <>
              Server <span className="font-mono">{health.data.version}</span> · up {formatDuration(health.data.uptime_seconds)} ·{" "}
              <a href={`${api.client.baseUrl}/metrics`} className="hover:text-fg hover:underline">
                Prometheus metrics
              </a>
            </>
          ) : (
            `Welcome back, ${account?.display_name || account?.username || ""}`
          )
        }
      />

      <div className="flex flex-col gap-5">
        {isAdmin ? (
          stats.isError ? (
            <Card>
              <ErrorState error={stats.error} onRetry={() => void stats.refetch()} />
            </Card>
          ) : (
            <StatsGrid stats={stats.data} loading={stats.isPending} />
          )
        ) : (
          <Card className="px-4 py-3 text-[13px] text-muted-foreground">
            You are a member of <span className="font-medium text-fg">{memberships.length}</span>{" "}
            {memberships.length === 1 ? "tenant" : "tenants"}. Server-wide statistics are visible to server administrators.
          </Card>
        )}

        <div className="grid gap-5 lg:grid-cols-5">
          <Section title="Live activity" actions={<LiveIndicator status={status} />} flush className="lg:col-span-3">
            <ActivityFeed items={feed} />
          </Section>
          <Section title="Recent pushes" flush className="lg:col-span-2">
            <RecentPushes />
          </Section>
        </div>
      </div>
    </>
  );
}
