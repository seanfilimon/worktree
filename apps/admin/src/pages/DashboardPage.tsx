import { Activity, Plus } from "lucide-react";
import { Link } from "react-router";
import { useApi } from "@/api/context";
import { useHealth, useStats } from "@/api/queries";
import { useAuth } from "@/auth/context";
import { ActivityCard } from "@/components/dashboard/ActivityCard";
import { MemberOverview } from "@/components/dashboard/MemberOverview";
import { RecentPushes } from "@/components/dashboard/RecentPushes";
import { RequestMetricsCard } from "@/components/dashboard/RequestMetricsCard";
import { ErrorState } from "@/components/dashboard/states";
import { StatsGrid } from "@/components/dashboard/StatsGrid";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { formatDuration } from "@/lib/format";

export function DashboardPage() {
  const api = useApi();
  const { isAdmin, account, me } = useAuth();
  const health = useHealth();
  const stats = useStats(isAdmin);
  const memberships = me.data?.memberships ?? [];

  return (
    <>
      <PageHeader
        title="Dashboard"
        description={
          health.data ? (
            <>
              Server <span className="font-mono">{health.data.version}</span> · up {formatDuration(health.data.uptime_seconds)} ·{" "}
              <a href={`${api.client.baseUrl}/metrics`} className="underline-offset-4 hover:text-foreground hover:underline">
                Prometheus metrics
              </a>
            </>
          ) : (
            `Welcome back, ${account?.display_name || account?.username || ""}`
          )
        }
        actions={
          <nav aria-label="Quick actions" className="flex flex-wrap gap-2">
            <Button asChild size="sm">
              <Link to="/trees?new=1">
                <Plus aria-hidden="true" />
                New tree
              </Link>
            </Button>
            <Button asChild size="sm" variant="outline">
              <Link to="/tenants?new=1">
                <Plus aria-hidden="true" />
                New tenant
              </Link>
            </Button>
            {isAdmin && (
              <Button asChild size="sm" variant="ghost">
                <Link to="/system">
                  <Activity aria-hidden="true" />
                  System
                </Link>
              </Button>
            )}
          </nav>
        }
      />

      <div className="flex flex-col gap-5">
        {isAdmin ? (
          stats.isError ? (
            <Card className="py-0">
              <ErrorState error={stats.error} onRetry={() => void stats.refetch()} />
            </Card>
          ) : (
            <StatsGrid stats={stats.data} loading={stats.isPending} />
          )
        ) : (
          <MemberOverview memberships={memberships} />
        )}

        <div className="grid gap-5 lg:grid-cols-5">
          <ActivityCard className="lg:col-span-3" />
          <RecentPushes className="lg:col-span-2" />
        </div>

        {isAdmin && <RequestMetricsCard />}
      </div>
    </>
  );
}
