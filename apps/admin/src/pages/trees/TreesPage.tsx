import { useQueries } from "@tanstack/react-query";
import { FolderTree, Plus } from "lucide-react";
import { useSearchParams } from "react-router";
import { useApi } from "@/api/context";
import { keys, useTenants } from "@/api/queries";
import { VISIBILITIES, type Visibility } from "@/api/types";
import { canMaintain, useAuth } from "@/auth/context";
import { CreateTreeDialog } from "@/components/people/CreateTreeDialog";
import { SearchInput } from "@/components/people/SearchInput";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import { DEFAULT_DIR, isTreeSortKey, type TreeRow, type TreeSort } from "@/components/people/treeSort";
import { PageHeader } from "@/components/PageHeader";
import { TreeTable } from "@/components/TreeTable";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { formatNumber } from "@/lib/format";

const ALL = "all";

function isVisibility(v: string | null): v is Visibility {
  return (VISIBILITIES as readonly string[]).includes(v ?? "");
}

function Totals({ rows }: { rows: TreeRow[] }) {
  const sum = (f: (r: TreeRow) => number) => rows.reduce((n, r) => n + f(r), 0);
  const items = [
    { label: "Trees", value: rows.length },
    { label: "Branches", value: sum((r) => r.summary.branch_count) },
    { label: "Snapshots", value: sum((r) => r.summary.snapshot_count) },
    { label: "Staged", value: sum((r) => r.summary.staged_count) },
    { label: "Open MRs", value: sum((r) => r.summary.open_mr_count) },
  ];
  return (
    <dl aria-label="Totals" className="grid grid-cols-2 gap-3 sm:grid-cols-5">
      {items.map((i) => (
        <Card key={i.label} className="gap-0 px-4 py-3">
          <dt className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{i.label}</dt>
          <dd className="font-mono text-xl font-semibold tabular-nums">{formatNumber(i.value)}</dd>
        </Card>
      ))}
    </dl>
  );
}

export function TreesPage() {
  const api = useApi();
  const tenants = useTenants();
  const { roleIn, isAdmin } = useAuth();
  const [params, setParams] = useSearchParams();

  const tenantFilter = params.get("tenant") ?? "";
  const visibilityParam = params.get("visibility");
  const visibility = isVisibility(visibilityParam) ? visibilityParam : "";
  const query = params.get("q") ?? "";
  const sortParam = params.get("sort");
  const sortKey = isTreeSortKey(sortParam) ? sortParam : "name";
  const dirParam = params.get("dir");
  const sort: TreeSort = { key: sortKey, dir: dirParam === "asc" || dirParam === "desc" ? dirParam : DEFAULT_DIR[sortKey] };
  const creating = params.get("new") === "1";

  const update = (next: Record<string, string | null>) =>
    setParams(
      (p) => {
        for (const [k, v] of Object.entries(next)) {
          if (v) p.set(k, v);
          else p.delete(k);
        }
        return p;
      },
      { replace: true },
    );

  const allTenants = tenants.data ?? [];
  const scoped = allTenants.filter((t) => !tenantFilter || t.slug === tenantFilter);
  const results = useQueries({
    queries: scoped.map((t) => ({ queryKey: keys.tenantTrees(t.slug), queryFn: () => api.tenants.trees(t.slug) })),
  });
  const creatable = allTenants.filter((t) => canMaintain(roleIn(t.id), isAdmin));

  const q = query.trim().toLowerCase();
  const rows: TreeRow[] = scoped
    .flatMap((t, i) => (results[i]?.data ?? []).map((summary) => ({ tenant: t.slug, summary })))
    .filter(({ summary: { tree } }) => !visibility || tree.visibility === visibility)
    .filter(({ tenant, summary: { tree } }) => !q || `${tenant}/${tree.name}`.toLowerCase().includes(q) || tree.description.toLowerCase().includes(q));
  const loading = tenants.isPending || results.some((r) => r.isPending);
  const failed = results.find((r) => r.isError);
  const filtered = Boolean(tenantFilter || visibility || q);

  return (
    <>
      <PageHeader
        title="Trees"
        breadcrumbs={[{ label: "Trees" }]}
        description="Every tree you can see, across tenants."
        actions={
          <Button onClick={() => update({ new: "1" })} disabled={creatable.length === 0}>
            <Plus aria-hidden="true" />
            New tree
          </Button>
        }
      />
      <div className="flex flex-col gap-4">
        {!loading && !failed && rows.length > 0 && <Totals rows={rows} />}
        <Card className="gap-0 overflow-hidden py-0">
          <div role="search" aria-label="Filter trees" className="flex flex-col gap-2 border-b p-3 sm:flex-row sm:items-center">
            <SearchInput label="Search trees" placeholder="Search trees…" value={query} onChange={(v) => update({ q: v })} />
            <Select value={tenantFilter || ALL} onValueChange={(v) => update({ tenant: v === ALL ? null : v })}>
              <SelectTrigger aria-label="Filter by tenant" className="w-full sm:w-44">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>All tenants</SelectItem>
                {allTenants.map((t) => (
                  <SelectItem key={t.id} value={t.slug}>
                    {t.slug}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select value={visibility || ALL} onValueChange={(v) => update({ visibility: v === ALL ? null : v })}>
              <SelectTrigger aria-label="Filter by visibility" className="w-full sm:w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>Any visibility</SelectItem>
                {VISIBILITIES.map((v) => (
                  <SelectItem key={v} value={v} className="capitalize">
                    {v}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {filtered && (
              <Button variant="ghost" size="sm" onClick={() => update({ tenant: null, visibility: null, q: null })}>
                Clear filters
              </Button>
            )}
          </div>
          {tenants.isError ? (
            <QueryError error={tenants.error} onRetry={() => void tenants.refetch()} title="Could not load tenants" />
          ) : loading ? (
            <TableSkeleton label="Loading trees" columns={5} />
          ) : failed ? (
            <QueryError error={failed.error} onRetry={() => void failed.refetch()} title="Could not load trees" />
          ) : rows.length === 0 ? (
            filtered ? (
              <EmptyState icon={FolderTree} title="No trees match these filters">
                Try another tenant, visibility or search term.
              </EmptyState>
            ) : (
              <EmptyState icon={FolderTree} title="No trees yet">
                Create a tree here, or run <code className="font-mono">wt push</code> from a worktree with a remote configured.
              </EmptyState>
            )
          ) : (
            <TreeTable
              rows={rows}
              sort={sort}
              onSortChange={(s) => update({ sort: s.key === "name" ? null : s.key, dir: s.dir === DEFAULT_DIR[s.key] ? null : s.dir })}
            />
          )}
        </Card>
      </div>
      {creatable.length > 0 && (
        <CreateTreeDialog
          key={tenantFilter}
          open={creating}
          onOpenChange={(open) => update({ new: open ? "1" : null })}
          tenants={creatable}
          defaultTenant={tenantFilter}
        />
      )}
    </>
  );
}
