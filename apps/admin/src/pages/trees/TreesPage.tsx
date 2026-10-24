import { useMutation, useQueries, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { useApi } from "@/api/context";
import { keys, useTenants } from "@/api/queries";
import { VISIBILITIES, type Tenant, type Visibility } from "@/api/types";
import { PageHeader } from "@/components/PageHeader";
import { TreeTable, type TreeRow } from "@/components/TreeTable";
import { Button } from "@/components/ui/Button";
import { Section } from "@/components/ui/Card";
import { SelectField, TextField } from "@/components/ui/Field";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";

const NAME_RE = /^[A-Za-z0-9._-]+$/;

function CreateTreeForm({ tenants, initialTenant, onDone }: { tenants: Tenant[]; initialTenant: string; onDone: () => void }) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const [tenant, setTenant] = useState(initialTenant || tenants[0]?.slug || "");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [visibility, setVisibility] = useState<Visibility>("private");
  const [defaultBranch, setDefaultBranch] = useState("main");
  const [touched, setTouched] = useState(false);
  const nameError = NAME_RE.test(name) ? undefined : "Letters, digits, '.', '_' and '-' only.";

  const create = useMutation({
    mutationFn: () =>
      api.tenants.createTree(tenant, { name, description: description.trim(), visibility, default_branch: defaultBranch.trim() || "main" }),
    onSuccess: async (tree) => {
      toast.success("Tree created", `${tenant}/${tree.name}`);
      await queryClient.invalidateQueries({ queryKey: keys.tenantTrees(tenant) });
      onDone();
      void navigate(treePath(tenant, tree.name));
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (!nameError && tenant) create.mutate();
  };

  return (
    <form onSubmit={onSubmit} aria-label="Create tree" className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4" noValidate>
      <SelectField label="Tenant" value={tenant} onChange={(e) => setTenant(e.target.value)}>
        {tenants.map((t) => (
          <option key={t.id} value={t.slug}>
            {t.slug}
          </option>
        ))}
      </SelectField>
      <TextField label="Name" value={name} onChange={(e) => setName(e.target.value)} error={touched ? nameError : undefined} placeholder="backend" autoFocus />
      <SelectField label="Visibility" value={visibility} onChange={(e) => setVisibility(e.target.value as Visibility)}>
        {VISIBILITIES.map((v) => (
          <option key={v} value={v}>
            {v}
          </option>
        ))}
      </SelectField>
      <TextField label="Default branch" value={defaultBranch} onChange={(e) => setDefaultBranch(e.target.value)} />
      <TextField
        label="Description"
        value={description}
        onChange={(e) => setDescription(e.target.value)}
        fieldClassName="sm:col-span-2 lg:col-span-3"
        placeholder="Optional"
      />
      <div className="flex items-end gap-2">
        <Button type="submit" variant="primary" loading={create.isPending}>
          Create tree
        </Button>
        <Button onClick={onDone}>Cancel</Button>
      </div>
    </form>
  );
}

export function TreesPage() {
  const api = useApi();
  const tenants = useTenants();
  const [params, setParams] = useSearchParams();
  const filter = params.get("tenant") ?? "";
  const creating = params.get("new") === "1";

  const visible = (tenants.data ?? []).filter((t) => !filter || t.slug === filter);
  const results = useQueries({
    queries: visible.map((t) => ({ queryKey: keys.tenantTrees(t.slug), queryFn: () => api.tenants.trees(t.slug) })),
  });

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

  const rows: TreeRow[] = visible.flatMap((t, i) => (results[i]?.data ?? []).map((summary) => ({ tenant: t.slug, summary })));
  const loading = tenants.isPending || results.some((r) => r.isPending);
  const failed = results.find((r) => r.isError);

  return (
    <>
      <PageHeader
        title="Trees"
        breadcrumbs={[{ label: "Trees" }]}
        description="Every tree you can see, across tenants."
        actions={
          !creating && (
            <Button variant="primary" onClick={() => update({ new: "1" })} disabled={!tenants.data?.length}>
              New tree
            </Button>
          )
        }
      />
      <div className="flex flex-col gap-5">
        {creating && tenants.data && (
          <Section title="New tree">
            <CreateTreeForm tenants={tenants.data} initialTenant={filter} onDone={() => update({ new: null })} />
          </Section>
        )}
        <Section
          title="Trees"
          flush
          actions={
            <SelectField label="Filter by tenant" hideLabel value={filter} onChange={(e) => update({ tenant: e.target.value })} className="w-44">
              <option value="">All tenants</option>
              {(tenants.data ?? []).map((t) => (
                <option key={t.id} value={t.slug}>
                  {t.slug}
                </option>
              ))}
            </SelectField>
          }
        >
          {tenants.isError ? (
            <ErrorState error={tenants.error} onRetry={() => void tenants.refetch()} />
          ) : loading ? (
            <SkeletonRows label="Loading trees" />
          ) : failed ? (
            <ErrorState error={failed.error} onRetry={() => void failed.refetch()} />
          ) : rows.length === 0 ? (
            <EmptyState title="No trees yet">
              Create a tree here, or run <code className="font-mono">wt push</code> from a worktree with a remote configured.
            </EmptyState>
          ) : (
            <TreeTable rows={rows} />
          )}
        </Section>
      </div>
    </>
  );
}
