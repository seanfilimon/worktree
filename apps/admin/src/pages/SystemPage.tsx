import { ShieldX } from "lucide-react";
import { useSystemInfo } from "@/api/queries";
import { useAuth } from "@/auth/context";
import { MetricsViewer } from "@/components/dashboard/MetricsViewer";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/dashboard/states";
import { SystemHistory } from "@/components/dashboard/SystemHistory";
import { SystemOverview } from "@/components/dashboard/SystemOverview";
import { useSampledHistory } from "@/components/dashboard/useSampledHistory";
import { PageHeader } from "@/components/PageHeader";
import { Card } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

const breadcrumbs = [{ label: "System" }];

/** Server runtime, storage and metrics (server administrators). */
export function SystemPage() {
  const { isAdmin } = useAuth();
  const info = useSystemInfo(isAdmin);
  const samples = useSampledHistory(info.data, info.dataUpdatedAt, 60);

  if (!isAdmin) {
    return (
      <>
        <PageHeader title="System" breadcrumbs={breadcrumbs} />
        <Card className="py-0">
          <EmptyState icon={ShieldX} title="Server administrators only">
            System information and metrics are restricted to server administrators.
          </EmptyState>
        </Card>
      </>
    );
  }

  return (
    <>
      <PageHeader
        title="System"
        breadcrumbs={breadcrumbs}
        description={
          info.data ? (
            <>
              W0rkTree <span className="font-mono">{info.data.version}</span> on{" "}
              <span className="font-mono">
                {info.data.os}/{info.data.arch}
              </span>{" "}
              · refreshed every 10 seconds
            </>
          ) : (
            "Runtime, storage and event bus health"
          )
        }
      />
      <Tabs defaultValue="overview">
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="metrics">Raw metrics</TabsTrigger>
        </TabsList>
        <TabsContent value="overview" className="flex flex-col gap-4">
          {info.isError && <ErrorState className="m-0" error={info.error} onRetry={() => void info.refetch()} />}
          {info.data ? (
            <>
              <SystemOverview info={info.data} updatedAt={info.dataUpdatedAt} />
              <SystemHistory samples={samples} />
            </>
          ) : (
            info.isPending && (
              <Card className="py-0">
                <SkeletonRows rows={6} label="Loading system information" />
              </Card>
            )
          )}
        </TabsContent>
        <TabsContent value="metrics">
          <MetricsViewer />
        </TabsContent>
      </Tabs>
    </>
  );
}
