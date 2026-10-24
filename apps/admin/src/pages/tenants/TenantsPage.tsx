import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { useApi } from "@/api/context";
import { keys, useTenants } from "@/api/queries";
import { useAuth } from "@/auth/context";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Section } from "@/components/ui/Card";
import { TextField } from "@/components/ui/Field";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { Table, Td, Th, Tr } from "@/components/ui/Table";
import { RelativeTime } from "@/components/ui/Time";
import { useToast } from "@/toast/store";

const SLUG_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function CreateTenantForm({ onDone }: { onDone: () => void }) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const [slug, setSlug] = useState("");
  const [name, setName] = useState("");
  const [touched, setTouched] = useState(false);
  const slugError = SLUG_RE.test(slug) ? undefined : "Lowercase letters, digits and single dashes.";

  const create = useMutation({
    mutationFn: api.tenants.create,
    onSuccess: async (tenant) => {
      toast.success("Tenant created", tenant.slug);
      await queryClient.invalidateQueries({ queryKey: keys.tenants });
      await queryClient.invalidateQueries({ queryKey: keys.me });
      onDone();
      void navigate(`/tenants/${encodeURIComponent(tenant.slug)}`);
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (!slugError && name.trim()) create.mutate({ slug, name: name.trim() });
  };

  return (
    <form onSubmit={onSubmit} className="grid gap-3 sm:grid-cols-[1fr_1fr_auto] sm:items-start" aria-label="Create tenant" noValidate>
      <TextField
        label="Slug"
        placeholder="acme"
        value={slug}
        onChange={(e) => setSlug(e.target.value.toLowerCase())}
        error={touched ? slugError : undefined}
        hint={touched && slugError ? undefined : "Used in URLs and remotes: tenant/tree."}
        autoFocus
      />
      <TextField
        label="Display name"
        placeholder="Acme Corp"
        value={name}
        onChange={(e) => setName(e.target.value)}
        error={touched && !name.trim() ? "Required." : undefined}
      />
      <div className="flex gap-2 sm:pt-5">
        <Button type="submit" variant="primary" loading={create.isPending}>
          Create
        </Button>
        <Button onClick={onDone}>Cancel</Button>
      </div>
    </form>
  );
}

export function TenantsPage() {
  const tenants = useTenants();
  const { roleIn } = useAuth();
  const [creating, setCreating] = useState(false);

  return (
    <>
      <PageHeader
        title="Tenants"
        breadcrumbs={[{ label: "Tenants" }]}
        description="Organisations and personal namespaces that own trees."
        actions={
          !creating && (
            <Button variant="primary" onClick={() => setCreating(true)}>
              New tenant
            </Button>
          )
        }
      />
      <div className="flex flex-col gap-5">
        {creating && (
          <Section title="New organisation tenant">
            <CreateTenantForm onDone={() => setCreating(false)} />
          </Section>
        )}
        <Section title="All tenants" flush>
          {tenants.isPending ? (
            <SkeletonRows label="Loading tenants" />
          ) : tenants.isError ? (
            <ErrorState error={tenants.error} onRetry={() => void tenants.refetch()} />
          ) : tenants.data.length === 0 ? (
            <EmptyState title="No tenants" action={<Button onClick={() => setCreating(true)}>Create a tenant</Button>}>
              Create an organisation to group trees and members.
            </EmptyState>
          ) : (
            <Table label="Tenants">
              <thead>
                <tr>
                  <Th>Slug</Th>
                  <Th>Name</Th>
                  <Th>Kind</Th>
                  <Th>Plan</Th>
                  <Th>Your role</Th>
                  <Th>Created</Th>
                </tr>
              </thead>
              <tbody>
                {tenants.data.map((t) => (
                  <Tr key={t.id}>
                    <Td>
                      <Link to={`/tenants/${encodeURIComponent(t.slug)}`} className="font-mono font-medium text-accent hover:underline">
                        {t.slug}
                      </Link>
                    </Td>
                    <Td>{t.name}</Td>
                    <Td>
                      <Badge>{t.kind}</Badge>
                    </Td>
                    <Td className="capitalize">{t.plan}</Td>
                    <Td>{roleIn(t.id) ?? <span className="text-subtle">—</span>}</Td>
                    <Td>
                      <RelativeTime value={t.created_at} className="text-muted" />
                    </Td>
                  </Tr>
                ))}
              </tbody>
            </Table>
          )}
        </Section>
      </div>
    </>
  );
}
