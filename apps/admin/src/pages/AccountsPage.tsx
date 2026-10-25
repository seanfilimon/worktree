import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Ellipsis, KeyRound, Plus, UserRound, UsersRound } from "lucide-react";
import { useState } from "react";
import { useApi } from "@/api/context";
import { keys, useAccounts } from "@/api/queries";
import type { Account, UpdateAccountRequest } from "@/api/types";
import { RequireAdmin } from "@/auth/RequireAuth";
import { useAuth } from "@/auth/context";
import { AccountSheet } from "@/components/people/AccountSheet";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { CreateAccountDialog } from "@/components/people/CreateAccountDialog";
import { ResetPasswordDialog } from "@/components/people/ResetPasswordDialog";
import { SearchInput } from "@/components/people/SearchInput";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import { UserAvatar } from "@/components/people/UserAvatar";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, relativeTime } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";

interface PendingChange {
  account: Account;
  body: UpdateAccountRequest;
}

/** Copy for changes that need confirmation (everything except re-enabling). */
function describeChange({ account, body }: PendingChange): { title: string; description: string; confirm: string } {
  const u = account.username;
  if (body.is_admin === true)
    return { title: `Make ${u} a server admin?`, description: `${u} gets full control of every tenant, tree and account on this server.`, confirm: "Grant admin" };
  if (body.is_admin === false)
    return { title: `Remove admin rights from ${u}?`, description: `${u} keeps their tenant roles but loses server-wide administration.`, confirm: "Remove admin" };
  return { title: `Disable ${u}?`, description: `${u} can no longer sign in, and their tokens stop working until the account is re-enabled.`, confirm: "Disable account" };
}

function AccountsTable() {
  const api = useApi();
  const accounts = useAccounts();
  const { account: me } = useAuth();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  const [confirming, setConfirming] = useState<PendingChange | null>(null);
  const [resetting, setResetting] = useState<Account | null>(null);
  const [viewingId, setViewingId] = useState<string | null>(null);

  const update = useMutation({
    mutationFn: (v: PendingChange) => api.admin.updateAccount(v.account.username, v.body),
    onSuccess: (updated) => {
      queryClient.setQueryData<Account[]>(keys.accounts, (list) => list?.map((a) => (a.id === updated.id ? updated : a)));
      toast.success("Account updated", updated.username);
    },
  });

  const change = (account: Account, body: UpdateAccountRequest) => {
    if (body.disabled === false) update.mutate({ account, body });
    else setConfirming({ account, body });
  };

  const q = query.trim().toLowerCase();
  const all = accounts.data ?? [];
  const rows = all.filter(
    (a) => !q || a.username.includes(q) || a.email.toLowerCase().includes(q) || a.display_name.toLowerCase().includes(q),
  );
  const viewing = all.find((a) => a.id === viewingId) ?? null;
  const confirmCopy = confirming ? describeChange(confirming) : null;

  return (
    <>
      <Card className="gap-0 overflow-hidden py-0">
        <div className="flex flex-col gap-2 border-b p-3 sm:flex-row sm:items-center sm:justify-between">
          <SearchInput label="Search accounts" placeholder="Search by name or email…" value={query} onChange={setQuery} />
          <Button size="sm" onClick={() => setCreating(true)}>
            <Plus aria-hidden="true" />
            New account
          </Button>
        </div>
        {accounts.isPending ? (
          <TableSkeleton label="Loading accounts" columns={5} />
        ) : accounts.isError ? (
          <QueryError error={accounts.error} onRetry={() => void accounts.refetch()} title="Could not load accounts" />
        ) : rows.length === 0 ? (
          <EmptyState icon={UsersRound} title={q ? "No accounts match your search" : "No accounts"} />
        ) : (
          <Table aria-label="Accounts">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Account</TableHead>
                <TableHead className="hidden md:table-cell">Email</TableHead>
                <TableHead className="hidden lg:table-cell">Created</TableHead>
                <TableHead>Server admin</TableHead>
                <TableHead>Disabled</TableHead>
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((a) => {
                const self = a.id === me?.id;
                const busy = update.isPending && update.variables.account.id === a.id;
                return (
                  <TableRow key={a.id} className={cn(a.disabled && "text-muted-foreground")}>
                    <TableCell>
                      <div className="flex items-center gap-3">
                        <UserAvatar name={a.display_name || a.username} className={cn(a.disabled && "opacity-50")} />
                        <div className="min-w-0">
                          <button
                            type="button"
                            className={cn("flex items-center gap-1.5 font-medium text-foreground hover:underline", a.disabled && "line-through")}
                            onClick={() => setViewingId(a.id)}
                          >
                            {a.username}
                          </button>
                          <p className="flex flex-wrap items-center gap-1 text-xs text-muted-foreground">
                            {a.display_name && <span>{a.display_name}</span>}
                            {self && <Badge variant="outline">you</Badge>}
                            {a.is_admin && <Badge>admin</Badge>}
                            {a.disabled && <Badge variant="outline">disabled</Badge>}
                          </p>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="hidden md:table-cell">{a.email}</TableCell>
                    <TableCell className="hidden lg:table-cell">
                      <time dateTime={a.created_at} title={absoluteTime(a.created_at)}>
                        {relativeTime(a.created_at)}
                      </time>
                    </TableCell>
                    <TableCell>
                      <Switch
                        aria-label={`Server admin: ${a.username}`}
                        aria-describedby={self ? "accounts-self-note" : undefined}
                        checked={a.is_admin}
                        disabled={self || busy}
                        onCheckedChange={(v) => change(a, { is_admin: v })}
                      />
                    </TableCell>
                    <TableCell>
                      <Switch
                        aria-label={`Disabled: ${a.username}`}
                        aria-describedby={self ? "accounts-self-note" : undefined}
                        checked={a.disabled}
                        disabled={self || busy}
                        onCheckedChange={(v) => change(a, { disabled: v })}
                      />
                    </TableCell>
                    <TableCell className="w-10 text-right">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label={`Actions for ${a.username}`}>
                            <Ellipsis aria-hidden="true" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onSelect={() => setViewingId(a.id)}>
                            <UserRound aria-hidden="true" />
                            View details
                          </DropdownMenuItem>
                          <DropdownMenuItem disabled={self} onSelect={() => setResetting(a)}>
                            <KeyRound aria-hidden="true" />
                            Reset password
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
        <p id="accounts-self-note" className="border-t px-4 py-2 text-xs text-muted-foreground">
          You cannot demote, disable or reset your own account here; change your password in Settings.
        </p>
      </Card>
      <CreateAccountDialog open={creating} onOpenChange={setCreating} />
      <ResetPasswordDialog account={resetting} onClose={() => setResetting(null)} />
      <AccountSheet account={viewing} onClose={() => setViewingId(null)} />
      <ConfirmDialog
        open={confirming !== null}
        onOpenChange={(open) => !open && setConfirming(null)}
        title={confirmCopy?.title ?? ""}
        description={confirmCopy?.description ?? ""}
        confirmLabel={confirmCopy?.confirm ?? "Confirm"}
        pending={update.isPending}
        onConfirm={() => (confirming ? update.mutateAsync(confirming) : Promise.resolve())}
      />
    </>
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
