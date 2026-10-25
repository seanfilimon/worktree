import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, Loader2, MailX } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { keys, useHealth } from "@/api/queries";
import { useAuthProviders, useInvitationPreview, useSecurityApi, type InvitationPreview } from "@/api/security";
import { useAuth } from "@/auth/context";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { AuthShell } from "./AuthShell";

type Unusable = { title: string; message: string };

function unusable({ invitation }: InvitationPreview, now: number): Unusable | null {
  if (invitation.revoked)
    return { title: "Invitation revoked", message: "This invitation was revoked. Ask for a new one." };
  if (invitation.accepted_at)
    return {
      title: "Invitation already used",
      message: `It was accepted${invitation.accepted_by ? ` by ${invitation.accepted_by}` : ""} ${relativeTime(invitation.accepted_at)}.`,
    };
  if (Date.parse(invitation.expires_at) < now)
    return {
      title: "Invitation expired",
      message: `It expired ${relativeTime(invitation.expires_at)}. Ask for a new one.`,
    };
  return null;
}

function Linkish({ to, from, children }: { to: string; from: string; children: string }) {
  return (
    <Link to={to} state={{ from }} className="font-medium text-foreground underline-offset-4 hover:underline">
      {children}
    </Link>
  );
}

/** Public page for `/invite/:code` (§12.2): preview, then accept or sign in. */
export function InvitePage() {
  const { code = "" } = useParams();
  const sec = useSecurityApi();
  const { token, account, signOut } = useAuth();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const toast = useToast();
  const preview = useInvitationPreview(code);
  const providers = useAuthProviders();
  const health = useHealth();
  const [now] = useState(Date.now);
  const here = `/invite/${encodeURIComponent(code)}`;

  const accept = useMutation({
    mutationFn: () => sec.invitations.accept(code),
    meta: { inlineError: true },
    onSuccess: async () => {
      const tenant = preview.data?.tenant;
      toast.success("Invitation accepted", tenant ? `You joined ${tenant.name || tenant.slug}.` : undefined);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.me }),
        queryClient.invalidateQueries({ queryKey: keys.tenants }),
      ]);
      void navigate(tenant ? `/tenants/${encodeURIComponent(tenant.slug)}` : "/", { replace: true });
    },
  });

  if (preview.isPending) {
    return (
      <AuthShell title="Invitation" description="Loading invitation…">
        <div role="status" aria-label="Loading invitation" className="flex flex-col gap-3">
          <Skeleton className="h-5 w-2/3" />
          <Skeleton className="h-4 w-1/2" />
          <Skeleton className="h-9 w-full" />
        </div>
      </AuthShell>
    );
  }

  if (preview.isError) {
    const missing = isApiError(preview.error) && (preview.error.status === 404 || preview.error.status === 410);
    return (
      <AuthShell
        title={missing ? "Invitation not found" : "Could not load invitation"}
        description={missing ? "The link is invalid, expired or was revoked." : "Something went wrong."}
        footer={
          <Linkish to="/" from="/">
            Go to the dashboard
          </Linkish>
        }
      >
        <Alert>
          {missing ? <MailX aria-hidden="true" /> : <CircleAlert aria-hidden="true" />}
          <AlertTitle>{missing ? "This invitation cannot be used" : "Request failed"}</AlertTitle>
          <AlertDescription>
            {missing ? "Ask the person who invited you for a new link." : errorMessage(preview.error)}
          </AlertDescription>
        </Alert>
      </AuthShell>
    );
  }

  const { invitation, tenant } = preview.data;
  const tenantName = tenant.name || tenant.slug;
  const problem = unusable(preview.data, now);
  const target = invitation.username ?? invitation.email;
  const mismatch =
    account !== undefined &&
    ((invitation.username !== undefined && invitation.username !== "" && invitation.username !== account.username) ||
      (invitation.email !== undefined &&
        invitation.email !== "" &&
        invitation.email.toLowerCase() !== account.email.toLowerCase()));
  const allowRegistration = providers.data ? providers.data.registration : health.data?.allow_registration !== false;

  return (
    <AuthShell
      title={`Join ${tenantName}`}
      description={`${invitation.invited_by || "Someone"} invited you to the ${tenantName} workspace on this server.`}
    >
      <div className="grid gap-4">
        <dl aria-label="Invitation details" className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
          <dt className="text-muted-foreground">Tenant</dt>
          <dd className="font-mono">{tenant.slug}</dd>
          <dt className="text-muted-foreground">Role</dt>
          <dd>
            <Badge variant="outline">{invitation.role}</Badge>
          </dd>
          {target && (
            <>
              <dt className="text-muted-foreground">For</dt>
              <dd className="truncate">{target}</dd>
            </>
          )}
          <dt className="text-muted-foreground">Expires</dt>
          <dd>
            <time dateTime={invitation.expires_at} title={absoluteTime(invitation.expires_at)}>
              {relativeTime(invitation.expires_at)}
            </time>
          </dd>
        </dl>

        {problem ? (
          <Alert>
            <MailX aria-hidden="true" />
            <AlertTitle>{problem.title}</AlertTitle>
            <AlertDescription>{problem.message}</AlertDescription>
          </Alert>
        ) : token ? (
          <>
            {account && (
              <p className="text-sm text-muted-foreground">
                Signed in as <span className="font-medium text-foreground">{account.username}</span>.
              </p>
            )}
            {mismatch && (
              <Alert>
                <CircleAlert aria-hidden="true" />
                <AlertTitle>This invitation is for {target}</AlertTitle>
                <AlertDescription>Sign in with that account to accept it.</AlertDescription>
              </Alert>
            )}
            {accept.error !== null && (
              <Alert>
                <CircleAlert aria-hidden="true" />
                <AlertTitle>Could not accept the invitation</AlertTitle>
                <AlertDescription>{errorMessage(accept.error)}</AlertDescription>
              </Alert>
            )}
            <Button className="w-full" disabled={accept.isPending || mismatch} onClick={() => accept.mutate()}>
              {accept.isPending && <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
              Accept invitation
            </Button>
            <Button variant="ghost" size="sm" onClick={() => void signOut()}>
              Use a different account
            </Button>
          </>
        ) : (
          <>
            <Button asChild className="w-full">
              <Link to="/login" state={{ from: here }}>
                Sign in to accept
              </Link>
            </Button>
            {allowRegistration && (
              <p className="text-center text-sm text-muted-foreground">
                New here?{" "}
                <Linkish to="/register" from={here}>
                  Create an account
                </Linkish>
              </p>
            )}
          </>
        )}
      </div>
    </AuthShell>
  );
}
