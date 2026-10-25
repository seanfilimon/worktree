import { FolderTree, Plus, Settings, Users } from "lucide-react";
import { useState } from "react";
import { useParams, useSearchParams } from "react-router";
import { useTenant } from "@/api/queries";
import { canMaintain, canManage, useAuth } from "@/auth/context";
import { PlanBadge, RoleBadge, TenantKindBadge } from "@/components/people/badges";
import { CreateTreeDialog } from "@/components/people/CreateTreeDialog";
import { QueryError } from "@/components/people/states";
import { UserAvatar } from "@/components/people/UserAvatar";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { absoluteTime, relativeTime } from "@/lib/format";
import { TenantMembersTab } from "./TenantMembersTab";
import { TenantSettingsTab } from "./TenantSettingsTab";
import { TenantTreesTab } from "./TenantTreesTab";

const TABS = ["trees", "members", "settings"] as const;
type Tab = (typeof TABS)[number];

function isTab(v: string | null): v is Tab {
  return (TABS as readonly string[]).includes(v ?? "");
}

export function TenantDetailPage() {
  const { tenant: slug = "" } = useParams();
  const t = useTenant(slug);
  const { roleIn, isAdmin } = useAuth();
  const [params, setParams] = useSearchParams();
  const [creatingTree, setCreatingTree] = useState(false);
  const tabParam = params.get("tab");
  const tab: Tab = isTab(tabParam) ? tabParam : "trees";

  const tenant = t.data;
  const role = tenant ? roleIn(tenant.id) : undefined;
  const manage = canManage(role, isAdmin);
  const createTrees = canMaintain(role, isAdmin);

  const setTab = (v: string) =>
    setParams(
      (p) => {
        if (v === "trees") p.delete("tab");
        else p.set("tab", v);
        return p;
      },
      { replace: true },
    );

  return (
    <>
      <PageHeader
        title={
          <span className="flex items-center gap-3">
            <UserAvatar name={tenant?.name || slug} square size="lg" />
            <span className="truncate">{tenant?.name || slug}</span>
          </span>
        }
        documentTitle={tenant?.name || slug}
        breadcrumbs={[{ label: "Tenants", to: "/tenants" }, { label: slug }]}
        description={
          tenant ? (
            <span className="flex flex-wrap items-center gap-2">
              <span className="font-mono">{tenant.slug}</span>
              <TenantKindBadge kind={tenant.kind} />
              <PlanBadge plan={tenant.plan} />
              {role && <RoleBadge role={role} />}
              <span>
                created{" "}
                <time dateTime={tenant.created_at} title={absoluteTime(tenant.created_at)}>
                  {relativeTime(tenant.created_at)}
                </time>
              </span>
            </span>
          ) : t.isPending ? (
            <Skeleton className="h-4 w-64" />
          ) : undefined
        }
        actions={
          createTrees && (
            <Button onClick={() => setCreatingTree(true)}>
              <Plus aria-hidden="true" />
              New tree
            </Button>
          )
        }
      />
      {t.isError ? (
        <QueryError error={t.error} onRetry={() => void t.refetch()} title="Could not load tenant" />
      ) : (
        <Tabs value={tab} onValueChange={setTab} className="gap-4">
          <TabsList>
            <TabsTrigger value="trees">
              <FolderTree aria-hidden="true" />
              Trees
            </TabsTrigger>
            <TabsTrigger value="members">
              <Users aria-hidden="true" />
              Members
            </TabsTrigger>
            <TabsTrigger value="settings">
              <Settings aria-hidden="true" />
              Settings
            </TabsTrigger>
          </TabsList>
          <TabsContent value="trees">
            <TenantTreesTab slug={slug} canCreate={createTrees} onCreate={() => setCreatingTree(true)} />
          </TabsContent>
          <TabsContent value="members">
            <TenantMembersTab slug={slug} editable={manage} canGrantOwner={isAdmin || role === "owner"} />
          </TabsContent>
          <TabsContent value="settings">{tenant && <TenantSettingsTab tenant={tenant} editable={manage} />}</TabsContent>
        </Tabs>
      )}
      {tenant && createTrees && <CreateTreeDialog open={creatingTree} onOpenChange={setCreatingTree} tenants={[tenant]} />}
    </>
  );
}
