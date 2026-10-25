import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Ellipsis, UserMinus, UserPlus, Users } from "lucide-react";
import { useState } from "react";
import { useApi } from "@/api/context";
import { keys, useMembers } from "@/api/queries";
import { ROLES, type Member, type Role } from "@/api/types";
import { useAuth } from "@/auth/context";
import { RoleBadge } from "@/components/people/badges";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { RoleLegend } from "@/components/people/RoleLegend";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import { UserAvatar } from "@/components/people/UserAvatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { useToast } from "@/toast/store";
import { AddMemberDialog } from "./AddMemberDialog";

interface TenantMembersTabProps {
  slug: string;
  /** Caller can manage members (tenant owner/admin or server admin). */
  editable: boolean;
  /** Caller can grant or revoke the owner role. */
  canGrantOwner: boolean;
}

export function TenantMembersTab({ slug, editable, canGrantOwner }: TenantMembersTabProps) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const { account } = useAuth();
  const members = useMembers(slug);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<Member | null>(null);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: keys.members(slug) });
  const setRole = useMutation({
    mutationFn: (v: { username: string; role: Role }) => api.tenants.setMember(slug, v.username, v.role),
    onSuccess: async (_m, v) => {
      toast.success("Role updated", `${v.username} is now ${v.role}`);
      await invalidate();
    },
  });
  const remove = useMutation({
    mutationFn: (username: string) => api.tenants.removeMember(slug, username),
    onSuccess: async (_r, username) => {
      toast.success("Member removed", username);
      await invalidate();
    },
  });

  const owners = members.data?.filter((m) => m.role === "owner").length ?? 0;

  const roleCell = (m: Member, self: boolean) => {
    const lockedOwner = m.role === "owner" && (!canGrantOwner || owners <= 1);
    if (!editable || self || lockedOwner) return <RoleBadge role={m.role} />;
    const busy = setRole.isPending && setRole.variables.username === m.account.username;
    return (
      <Select value={m.role} disabled={busy} onValueChange={(r) => setRole.mutate({ username: m.account.username, role: r as Role })}>
        <SelectTrigger size="sm" className="w-32 capitalize" aria-label={`Role of ${m.account.username}`}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {ROLES.map((r) => (
            <SelectItem key={r} value={r} disabled={r === "owner" && !canGrantOwner} className="capitalize">
              {r}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  };

  return (
    <div className="flex flex-col gap-4">
      <Card className="gap-0 overflow-hidden py-0">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b p-3">
          <p className="text-sm text-muted-foreground">
            {editable ? "Change roles inline. Owners and admins manage members." : "Only tenant owners and admins can change members."}
          </p>
          {editable && (
            <Button size="sm" onClick={() => setAdding(true)}>
              <UserPlus aria-hidden="true" />
              Add member
            </Button>
          )}
        </div>
        {members.isPending ? (
          <TableSkeleton label="Loading members" rows={3} />
        ) : members.isError ? (
          <QueryError error={members.error} onRetry={() => void members.refetch()} title="Could not load members" />
        ) : members.data.length === 0 ? (
          <EmptyState icon={Users} title="No members" />
        ) : (
          <Table aria-label="Members">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Account</TableHead>
                <TableHead className="hidden sm:table-cell">Email</TableHead>
                <TableHead>Role</TableHead>
                {editable && (
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {members.data.map((m) => {
                const self = m.account.id === account?.id;
                const removable = !self && !(m.role === "owner" && (!canGrantOwner || owners <= 1));
                return (
                  <TableRow key={m.account.id}>
                    <TableCell>
                      <div className="flex items-center gap-3">
                        <UserAvatar name={m.account.display_name || m.account.username} />
                        <div className="min-w-0">
                          <p className="flex items-center gap-1.5 font-medium">
                            {m.account.username}
                            {self && <Badge variant="outline">you</Badge>}
                          </p>
                          {m.account.display_name && <p className="text-xs text-muted-foreground">{m.account.display_name}</p>}
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="hidden text-muted-foreground sm:table-cell">{m.account.email}</TableCell>
                    <TableCell>{roleCell(m, self)}</TableCell>
                    {editable && (
                      <TableCell className="w-10 text-right">
                        {removable && (
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                              <Button variant="ghost" size="icon-sm" aria-label={`Actions for ${m.account.username}`}>
                                <Ellipsis aria-hidden="true" />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                              <DropdownMenuItem onSelect={() => setRemoving(m)}>
                                <UserMinus aria-hidden="true" />
                                Remove from tenant
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        )}
                      </TableCell>
                    )}
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Roles</CardTitle>
          <CardDescription>Roles are cumulative: each one includes everything the roles below it can do.</CardDescription>
        </CardHeader>
        <CardContent>
          <RoleLegend />
        </CardContent>
      </Card>
      {editable && <AddMemberDialog open={adding} onOpenChange={setAdding} slug={slug} canGrantOwner={canGrantOwner} />}
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title={`Remove ${removing?.account.username ?? ""}?`}
        description={`They lose access to every tree in ${slug} that is not internal or public. You can add them again later.`}
        confirmLabel="Remove member"
        pending={remove.isPending}
        onConfirm={() => remove.mutateAsync(removing?.account.username ?? "")}
      />
    </div>
  );
}
