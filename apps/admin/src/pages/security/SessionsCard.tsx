import { useMutation, useQueryClient } from "@tanstack/react-query";
import { LogOut, MonitorSmartphone, ShieldCheck, Trash2 } from "lucide-react";
import { useState } from "react";
import { securityKeys, useSecurityApi, useSessions, type Session } from "@/api/security";
import { useAuth } from "@/auth/context";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { describeUserAgent } from "./userAgent";

function When({ iso, empty = "never" }: { iso: string | undefined; empty?: string }) {
  if (!iso) return <span className="text-muted-foreground">{empty}</span>;
  return (
    <time dateTime={iso} title={absoluteTime(iso)}>
      {relativeTime(iso)}
    </time>
  );
}

/** Settings → Security: every signed-in session of the caller. */
export function SessionsCard() {
  const sec = useSecurityApi();
  const { signOut } = useAuth();
  const sessions = useSessions();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [revoking, setRevoking] = useState<Session | null>(null);
  const [revokingOthers, setRevokingOthers] = useState(false);
  const [signingOut, setSigningOut] = useState(false);

  const revoke = useMutation({
    mutationFn: sec.sessions.revoke,
    onSuccess: async () => {
      toast.success("Session revoked");
      await queryClient.invalidateQueries({ queryKey: securityKeys.sessions });
    },
  });

  const revokeOthers = useMutation({
    mutationFn: sec.sessions.revokeOthers,
    onSuccess: async (res) => {
      const n = res?.revoked ?? 0;
      toast.success(n === 1 ? "1 other session revoked" : `${n} other sessions revoked`);
      await queryClient.invalidateQueries({ queryKey: securityKeys.sessions });
    },
  });

  const others = sessions.data?.filter((s) => !s.current).length ?? 0;

  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle>Sessions</CardTitle>
        <CardDescription>
          Browsers and devices signed in to your account. Revoke any you do not recognise.
        </CardDescription>
        <CardAction>
          <Button size="sm" variant="outline" disabled={others === 0} onClick={() => setRevokingOthers(true)}>
            <LogOut aria-hidden="true" />
            Sign out other sessions
          </Button>
        </CardAction>
      </CardHeader>
      {sessions.isPending ? (
        <TableSkeleton label="Loading sessions" rows={2} />
      ) : sessions.isError ? (
        <QueryError error={sessions.error} onRetry={() => void sessions.refetch()} title="Could not load sessions" />
      ) : sessions.data.length === 0 ? (
        <EmptyState icon={MonitorSmartphone} title="No active sessions" />
      ) : (
        <Table aria-label="Sessions">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="pl-6">Client</TableHead>
              <TableHead className="hidden md:table-cell">IP address</TableHead>
              <TableHead className="hidden sm:table-cell">Signed in</TableHead>
              <TableHead>Last active</TableHead>
              <TableHead>
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sessions.data.map((s) => {
              const client = describeUserAgent(s.user_agent);
              return (
                <TableRow key={s.id} aria-current={s.current ? "true" : undefined}>
                  <TableCell className="pl-6">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <span className="font-medium" title={s.user_agent}>
                        {client}
                      </span>
                      {s.current && <Badge>This browser</Badge>}
                      {s.mfa && (
                        <Badge variant="outline">
                          <ShieldCheck aria-hidden="true" />
                          2FA
                        </Badge>
                      )}
                    </div>
                    <span className="font-mono text-xs text-muted-foreground md:hidden">{s.last_ip}</span>
                  </TableCell>
                  <TableCell className="hidden font-mono text-xs md:table-cell">
                    {s.last_ip || <span className="text-muted-foreground">unknown</span>}
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground sm:table-cell">
                    <When iso={s.created_at} />
                  </TableCell>
                  <TableCell className="text-muted-foreground">
                    <When iso={s.last_used_at} empty="—" />
                  </TableCell>
                  <TableCell className="w-10 pr-4 text-right">
                    {!s.current && (
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Revoke session: ${client}`}
                        onClick={() => setRevoking(s)}
                      >
                        <Trash2 aria-hidden="true" />
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
      <CardFooter className="flex-wrap justify-between gap-2 border-t py-4 text-xs text-muted-foreground">
        <span>Changing your password also signs out every other session.</span>
        <Button
          variant="outline"
          size="sm"
          disabled={signingOut}
          onClick={() => {
            setSigningOut(true);
            void signOut();
          }}
        >
          <LogOut aria-hidden="true" />
          {signingOut ? "Signing out…" : "Sign out of this session"}
        </Button>
      </CardFooter>
      <ConfirmDialog
        open={revoking !== null}
        onOpenChange={(open) => !open && setRevoking(null)}
        title="Revoke this session?"
        description={`${describeUserAgent(revoking?.user_agent)}${revoking?.last_ip ? ` (${revoking.last_ip})` : ""} is signed out immediately.`}
        confirmLabel="Revoke"
        pending={revoke.isPending}
        onConfirm={() => revoke.mutateAsync(revoking?.id ?? "")}
      />
      <ConfirmDialog
        open={revokingOthers}
        onOpenChange={setRevokingOthers}
        title="Sign out every other session?"
        description={`${others === 1 ? "1 other session is" : `${others} other sessions are`} signed out. This browser stays signed in. API tokens are not affected.`}
        confirmLabel="Sign out others"
        pending={revokeOthers.isPending}
        onConfirm={() => revokeOthers.mutateAsync()}
      />
    </Card>
  );
}
