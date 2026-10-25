import { Link } from "react-router";
import { useAuth } from "@/auth/context";
import { RoleBadge } from "@/components/people/badges";
import { UserAvatar } from "@/components/people/UserAvatar";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { tenantPath } from "@/lib/paths";

export function ProfileCard() {
  const { account, me } = useAuth();
  if (!account) return null;
  const memberships = me.data?.memberships ?? [];
  const personal = memberships.find((m) => m.tenant.id === account.tenant_id)?.tenant;
  const others = memberships.filter((m) => m.tenant.id !== account.tenant_id);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Profile</CardTitle>
        <CardDescription>Your account on this server.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        <div className="flex items-center gap-4">
          <UserAvatar name={account.display_name || account.username} size="lg" />
          <div className="min-w-0">
            <p className="flex flex-wrap items-center gap-2 font-semibold">
              {account.display_name || account.username}
              {account.is_admin && <Badge>Server admin</Badge>}
            </p>
            <p className="truncate text-sm text-muted-foreground">{account.email}</p>
          </div>
        </div>
        <dl className="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-[max-content_1fr]">
          <dt className="text-muted-foreground">Username</dt>
          <dd className="font-mono">{account.username}</dd>
          <dt className="text-muted-foreground">Email</dt>
          <dd className="break-all">{account.email}</dd>
          <dt className="text-muted-foreground">Personal tenant</dt>
          <dd className="font-mono">
            {personal ? (
              <Link to={tenantPath(personal.slug)} className="hover:underline">
                {personal.slug}
              </Link>
            ) : (
              "—"
            )}
          </dd>
          <dt className="text-muted-foreground">Organisations</dt>
          <dd>
            {others.length === 0 ? (
              <span className="text-muted-foreground">None</span>
            ) : (
              <ul aria-label="Your organisations" className="flex flex-wrap gap-2">
                {others.map((m) => (
                  <li key={m.tenant.id} className="flex items-center gap-1.5">
                    <Link to={tenantPath(m.tenant.slug)} className="font-mono hover:underline">
                      {m.tenant.slug}
                    </Link>
                    <RoleBadge role={m.role} />
                  </li>
                ))}
              </ul>
            )}
          </dd>
        </dl>
      </CardContent>
    </Card>
  );
}
