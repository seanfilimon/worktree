import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useParams } from "react-router";
import { useApi } from "@/api/context";
import { keys, useMembers, useTenant, useTenantTrees } from "@/api/queries";
import { ROLES, type Member, type Role } from "@/api/types";
import { canManage, useAuth } from "@/auth/context";
import { PageHeader } from "@/components/PageHeader";
import { TreeTable } from "@/components/TreeTable";
import { Badge } from "@/components/ui/Badge";
import { ButtonLink, Button } from "@/components/ui/Button";
import { Section } from "@/components/ui/Card";
import { ConfirmButton } from "@/components/ui/ConfirmButton";
import { SelectField, TextField } from "@/components/ui/Field";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { Table, Td, Th, Tr } from "@/components/ui/Table";
import { RelativeTime } from "@/components/ui/Time";
import { useToast } from "@/toast/store";

function RoleSelect({ value, onChange, label, disabled }: { value: Role; onChange: (r: Role) => void; label: string; disabled?: boolean }) {
  return (
    <SelectField label={label} hideLabel value={value} disabled={disabled} onChange={(e) => onChange(e.target.value as Role)} className="w-36">
      {ROLES.map((r) => (
        <option key={r} value={r}>
          {r}
        </option>
      ))}
    </SelectField>
  );
}

function MembersSection({ tenant, editable }: { tenant: string; editable: boolean }) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const { account } = useAuth();
  const members = useMembers(tenant);
  const [username, setUsername] = useState("");
  const [role, setRole] = useState<Role>("developer");

  const invalidate = () => queryClient.invalidateQueries({ queryKey: keys.members(tenant) });
  const setMember = useMutation({
    mutationFn: (v: { username: string; role: Role }) => api.tenants.setMember(tenant, v.username, v.role),
    onSuccess: async (_m, v) => {
      toast.success("Member updated", `${v.username} is now ${v.role}`);
      await invalidate();
    },
  });
  const removeMember = useMutation({
    mutationFn: (u: string) => api.tenants.removeMember(tenant, u),
    onSuccess: async (_r, u) => {
      toast.success("Member removed", u);
      await invalidate();
    },
  });

  const onAdd = (e: FormEvent) => {
    e.preventDefault();
    const u = username.trim();
    if (!u) return;
    setMember.mutate({ username: u, role }, { onSuccess: () => setUsername("") });
  };

  const row = (m: Member) => {
    const self = m.account.id === account?.id;
    return (
      <Tr key={m.account.id}>
        <Td>
          <span className="font-medium">{m.account.username}</span>
          {self && <Badge className="ml-1.5">you</Badge>}
          {m.account.display_name && <p className="text-xs text-muted">{m.account.display_name}</p>}
        </Td>
        <Td className="text-muted">{m.account.email}</Td>
        <Td>
          {editable ? (
            <RoleSelect
              label={`Role of ${m.account.username}`}
              value={m.role}
              disabled={setMember.isPending}
              onChange={(r) => setMember.mutate({ username: m.account.username, role: r })}
            />
          ) : (
            <Badge>{m.role}</Badge>
          )}
        </Td>
        <Td className="text-right">
          {editable && (
            <ConfirmButton confirmLabel="Remove" onConfirm={() => removeMember.mutate(m.account.username)} aria-label={`Remove ${m.account.username}`}>
              Remove
            </ConfirmButton>
          )}
        </Td>
      </Tr>
    );
  };

  return (
    <Section title="Members" description={editable ? "Change roles inline. Owners and admins can manage members." : undefined} flush>
      {editable && (
        <form onSubmit={onAdd} aria-label="Add member" className="flex flex-wrap items-end gap-2 border-b border-border p-4">
          <TextField label="Username" value={username} onChange={(e) => setUsername(e.target.value)} placeholder="jane" fieldClassName="w-56" />
          <SelectField label="Role" value={role} onChange={(e) => setRole(e.target.value as Role)} fieldClassName="w-40">
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </SelectField>
          <Button type="submit" variant="primary" loading={setMember.isPending} disabled={!username.trim()}>
            Add member
          </Button>
        </form>
      )}
      {members.isPending ? (
        <SkeletonRows rows={3} label="Loading members" />
      ) : members.isError ? (
        <ErrorState error={members.error} onRetry={() => void members.refetch()} />
      ) : members.data.length === 0 ? (
        <EmptyState title="No members" />
      ) : (
        <Table label="Members">
          <thead>
            <tr>
              <Th>Account</Th>
              <Th>Email</Th>
              <Th>Role</Th>
              <Th>
                <span className="sr-only">Actions</span>
              </Th>
            </tr>
          </thead>
          <tbody>{members.data.map(row)}</tbody>
        </Table>
      )}
    </Section>
  );
}

export function TenantDetailPage() {
  const { tenant = "" } = useParams();
  const t = useTenant(tenant);
  const trees = useTenantTrees(tenant);
  const { roleIn, isAdmin } = useAuth();
  const editable = t.data ? canManage(roleIn(t.data.id), isAdmin) : false;

  return (
    <>
      <PageHeader
        title={t.data?.name ?? tenant}
        breadcrumbs={[{ label: "Tenants", to: "/tenants" }, { label: tenant }]}
        description={
          t.data && (
            <span className="flex flex-wrap items-center gap-2">
              <span className="font-mono">{t.data.slug}</span>
              <Badge>{t.data.kind}</Badge>
              <Badge tone="info">{t.data.plan}</Badge>
              <span>
                created <RelativeTime value={t.data.created_at} />
              </span>
            </span>
          )
        }
        actions={
          <ButtonLink to={`/trees?tenant=${encodeURIComponent(tenant)}&new=1`} variant="primary">
            New tree
          </ButtonLink>
        }
      />
      {t.isError ? (
        <ErrorState error={t.error} onRetry={() => void t.refetch()} />
      ) : (
        <div className="flex flex-col gap-5">
          <Section title="Trees" flush>
            {trees.isPending ? (
              <SkeletonRows rows={3} label="Loading trees" />
            ) : trees.isError ? (
              <ErrorState error={trees.error} onRetry={() => void trees.refetch()} />
            ) : trees.data.length === 0 ? (
              <EmptyState title="No trees in this tenant">Create one here or push with <code className="font-mono">wt push</code>.</EmptyState>
            ) : (
              <TreeTable rows={trees.data.map((summary) => ({ tenant, summary }))} />
            )}
          </Section>
          <MembersSection tenant={tenant} editable={editable} />
        </div>
      )}
    </>
  );
}
