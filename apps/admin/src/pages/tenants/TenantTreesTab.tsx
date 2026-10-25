import { FolderTree, Plus } from "lucide-react";
import { useState } from "react";
import { useTenantTrees } from "@/api/queries";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import type { TreeSort } from "@/components/people/treeSort";
import { TreeTable } from "@/components/TreeTable";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";

interface TenantTreesTabProps {
  slug: string;
  canCreate: boolean;
  onCreate: () => void;
}

export function TenantTreesTab({ slug, canCreate, onCreate }: TenantTreesTabProps) {
  const trees = useTenantTrees(slug);
  const [sort, setSort] = useState<TreeSort>({ key: "name", dir: "asc" });

  return (
    <Card className="gap-0 overflow-hidden py-0">
      {trees.isPending ? (
        <TableSkeleton label="Loading trees" columns={5} />
      ) : trees.isError ? (
        <QueryError error={trees.error} onRetry={() => void trees.refetch()} title="Could not load trees" />
      ) : trees.data.length === 0 ? (
        <EmptyState
          icon={FolderTree}
          title="No trees in this tenant"
          action={
            canCreate && (
              <Button variant="outline" onClick={onCreate}>
                <Plus aria-hidden="true" />
                New tree
              </Button>
            )
          }
        >
          Create one here or push with <code className="font-mono">wt push</code>.
        </EmptyState>
      ) : (
        <TreeTable rows={trees.data.map((summary) => ({ tenant: slug, summary }))} showTenant={false} sort={sort} onSortChange={setSort} />
      )}
    </Card>
  );
}
