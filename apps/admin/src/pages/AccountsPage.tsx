import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApi } from "@/api/context";
import { keys, useAccounts } from "@/api/queries";
import type { Account, UpdateAccountRequest } from "@/api/types";
import { RequireAdmin } from "@/auth/RequireAuth";
import { useAuth } from "@/auth/context";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/legacy/Badge";
import { Section } from "@/components/legacy/Card";
import { TextField } from "@/components/legacy/Field";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/legacy/States";
import { Switch } from "@/components/legacy/Switch";
import { Table, Td, Th, Tr } from "@/components/legacy/Table";
import { RelativeTime } from "@/components/legacy/Time";
import { useToast } from "@/toast/store";

function AccountsTable() {
  const api = useApi();
  const accounts = useAccounts();
  const { account: me } = useAuth();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [query, setQuery] = useState("");

  const update = useMutation({
    mutationFn: (v: { username: string; body: UpdateAccountRequest }) => api.admin.updateAccount(v.username, v.body),
    onSuccess: (updated) => {
      queryClient.setQueryData<Account[]>(keys.accounts, (list) => list?.map((a) => (a.id === updated.id ? updated : a)));
      toast.success("Account updated", updated.username);
    },
  });

  const q = query.trim().toLowerCase();
  const rows = (accounts.data ?? []).filter(
    (a) => !q || a.username.toLowerCase().includes(q) || a.email.toLowerCase().includes(q) || a.display_name.toLowerCase().includes(q),
  );
  const pendingFor = update.isPending ? update.variables.username : null;

  return (
    <Section
      title="Accounts"
      description="Grant server-wide admin rights or disable sign-in. You cannot change your own account here."
      flush
      actions={<TextField label="Search accounts" hideLabel type="search" placeholder="Search…" value={query} onChange={(e) => setQuery(e.target.value)} className="w-56" />}
    >
      {accounts.isPending ? (
        <SkeletonRows label="Loading accounts" />
      ) : accounts.isError ? (
        <ErrorState error={accounts.error} onRetry={() => void accounts.refetch()} />
      ) : rows.length === 0 ? (
        <EmptyState title={q ? "No accounts match your search" : "No accounts"} />
      ) : (
        <Table label="Accounts">
          <thead>
            <tr>
              <Th>Account</Th>
              <Th>Email</Th>
              <Th>Created</Th>
              <Th>Server admin</Th>
              <Th>Disabled</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map((a) => {
              const self = a.id === me?.id;
              const busy = pendingFor === a.username;
              return (
                <Tr key={a.id} className={a.disabled ? "opacity-70" : undefined}>
                  <Td>
                    <span className="font-medium">{a.username}</span>
                    {self && <Badge className="ml-1.5">you</Badge>}
                    {a.disabled && (
                      <Badge tone="danger" className="ml-1.5">
                        disabled
                      </Badge>
                    )}
                    {a.display_name && <p className="text-xs text-muted-foreground">{a.display_name}</p>}
                  </Td>
                  <Td className="text-muted-foreground">{a.email}</Td>
                  <Td>
                    <RelativeTime value={a.created_at} className="text-muted-foreground" />
                  </Td>
                  <Td>
                    <Switch
                      label={`Server admin: ${a.username}`}
                      checked={a.is_admin}
                      disabled={self || busy}
                      onChange={(v) => update.mutate({ username: a.username, body: { is_admin: v } })}
                    />
                  </Td>
                  <Td>
                    <Switch
                      label={`Disabled: ${a.username}`}
                      checked={a.disabled}
                      disabled={self || busy}
                      onChange={(v) => update.mutate({ username: a.username, body: { disabled: v } })}
                    />
                  </Td>
                </Tr>
              );
            })}
          </tbody>
        </Table>
      )}
    </Section>
  );
}

export function AccountsPage() {
  return (
    <>
      <PageHeader title="Accounts" breadcrumbs={[{ label: "Accounts" }]} description="Every account on this server." />
      <RequireAdmin>
        <AccountsTable />
      </RequireAdmin>
    </>
  );
}
