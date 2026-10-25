import {
  Ban,
  Bot,
  Building2,
  Check,
  CircleDashed,
  Crown,
  Eye,
  FolderTree,
  Info,
  KeyRound,
  Puzzle,
  ScrollText,
  ShieldCheck,
  ShieldX,
  Users,
  X,
  type LucideIcon,
} from "lucide-react";
import type { UseQueryResult } from "@tanstack/react-query";
import type { Decision, DecisionReason, EffectivePermissions } from "@/api/iam";
import { QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { IamRoleBadge, PermissionChips, SubjectLabel } from "./shared";
import { humanize } from "./util";

const SOURCE_ICON: Record<string, LucideIcon> = {
  server_admin: Crown,
  disabled: Ban,
  tenant_role: Building2,
  team_role: Users,
  tree_grant: FolderTree,
  custom_role: Puzzle,
  service_account: Bot,
  visibility: Eye,
  token_scope: KeyRound,
  policy: ScrollText,
  default: CircleDashed,
};

const EFFECT_LABEL: Record<Decision["effect"], string> = {
  allow: "Allowed",
  deny: "Explicit deny",
  implicit_deny: "Implicit deny (no role or policy allows it)",
};

function ReasonEffect({ effect }: { effect: DecisionReason["effect"] }) {
  const Icon = effect === "allow" ? Check : effect === "deny" ? X : Info;
  return (
    <Badge variant={effect === "deny" ? "default" : "outline"} className="uppercase">
      <Icon aria-hidden="true" />
      {effect}
    </Badge>
  );
}

/** The simulator's verdict: inverted ALLOWED / DENIED badge, effect and the ordered reasons. */
export function DecisionCard({ decision, summary }: { decision: Decision; summary: string }) {
  const Icon = decision.allowed ? ShieldCheck : ShieldX;
  return (
    <Card aria-label="Decision">
      <CardHeader>
        <CardDescription className="font-mono break-all">{summary}</CardDescription>
        <CardTitle>
          <span
            role="status"
            className={cn(
              "inline-flex items-center gap-3 rounded-lg border-2 border-foreground px-5 py-2.5 text-2xl font-black tracking-[0.2em] sm:text-3xl",
              decision.allowed ? "bg-foreground text-background" : "bg-background text-foreground",
            )}
          >
            <Icon aria-hidden="true" className="size-7" />
            {decision.allowed ? "ALLOWED" : "DENIED"}
          </span>
        </CardTitle>
        <p className="text-sm">
          <span className="text-muted-foreground">Effect: </span>
          <span className="font-medium">{EFFECT_LABEL[decision.effect] ?? decision.effect}</span>
        </p>
      </CardHeader>
      <CardContent>
        <h3 className="mb-2 text-sm font-semibold">Reasons</h3>
        {decision.reasons.length === 0 ? (
          <p className="text-sm text-muted-foreground">No reasons were reported.</p>
        ) : (
          <ol aria-label="Reasons" className="grid gap-2">
            {decision.reasons.map((r, i) => {
              const SourceIcon = SOURCE_ICON[r.source] ?? Info;
              return (
                <li
                  key={i}
                  className={cn("flex items-start gap-3 rounded-md border p-2.5", r.effect === "deny" && "border-foreground border-2")}
                >
                  <span className="mt-0.5 w-5 shrink-0 text-right font-mono text-xs text-muted-foreground tabular-nums">{i + 1}</span>
                  <span className="flex size-7 shrink-0 items-center justify-center rounded-full border bg-muted">
                    <SourceIcon aria-hidden="true" className="size-3.5" />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-medium">{humanize(r.source)}</span>
                    <span className="block text-sm break-words text-muted-foreground">{r.detail}</span>
                  </span>
                  <ReasonEffect effect={r.effect} />
                </li>
              );
            })}
          </ol>
        )}
      </CardContent>
    </Card>
  );
}

/** Role, team roles, grants and every permitted action for a principal in a scope. */
export function EffectivePanel({ query, scope }: { query: UseQueryResult<EffectivePermissions>; scope: string }) {
  return (
    <Card aria-label="Effective permissions">
      <CardHeader>
        <CardTitle>Effective permissions</CardTitle>
        <CardDescription className="font-mono break-all">{scope}</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-5">
        {query.isPending ? (
          <TableSkeleton label="Loading effective permissions" rows={3} />
        ) : query.isError ? (
          <QueryError error={query.error} onRetry={() => void query.refetch()} title="Could not load effective permissions" />
        ) : (
          <>
            <dl className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
              <div className="grid gap-1.5">
                <dt className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Tenant role</dt>
                <dd>{query.data.role ? <IamRoleBadge role={query.data.role} /> : <span className="text-sm text-muted-foreground">None</span>}</dd>
              </div>
              <div className="grid gap-1.5">
                <dt className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Team roles</dt>
                <dd>
                  {query.data.team_roles.length === 0 ? (
                    <span className="text-sm text-muted-foreground">None</span>
                  ) : (
                    <ul aria-label="Team roles" className="grid gap-1">
                      {query.data.team_roles.map((t) => (
                        <li key={`${t.team}:${t.role}`} className="flex items-center gap-2 text-sm">
                          <Users aria-hidden="true" className="size-3.5" />
                          <span className="font-mono">{t.team}</span>
                          <IamRoleBadge role={t.role} />
                        </li>
                      ))}
                    </ul>
                  )}
                </dd>
              </div>
            </dl>
            <div className="grid gap-1.5">
              <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Grants</h3>
              {query.data.grants.length === 0 ? (
                <span className="text-sm text-muted-foreground">None</span>
              ) : (
                <ul aria-label="Grants" className="grid gap-2">
                  {query.data.grants.map((g) => (
                    <li key={`${g.tree_id}:${g.subject_type}:${g.subject_id}`} className="flex items-center justify-between gap-2">
                      <SubjectLabel type={g.subject_type} name={g.subject} />
                      <IamRoleBadge role={g.role} />
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <div className="grid gap-2">
              <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                Permissions ({query.data.permissions.length})
              </h3>
              <PermissionChips permissions={query.data.permissions} emptyLabel="No permissions in this scope." />
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
