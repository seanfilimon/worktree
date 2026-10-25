import { Download, ScrollText } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useSearchParams } from "react-router";
import { useAudit, useTenants } from "@/api/queries";
import type { AuditEvent, AuditQuery } from "@/api/types";
import { AuditTable } from "@/components/dashboard/AuditTable";
import { downloadText, toCsv } from "@/components/dashboard/csv";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/dashboard/states";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

const PAGE = 50;
/** Server-side cap on `limit` (GET /api/v1/audit). */
const MAX_LIMIT = 1000;
const ALL_TENANTS = "__all";
const FILTER_KEYS = ["tenant", "action", "actor", "since"] as const;

/** Actions recorded by the server; the field also accepts any other value. */
const KNOWN_ACTIONS = [
  "account.update",
  "auth.login",
  "auth.login_failed",
  "auth.logout",
  "auth.register",
  "branch.create",
  "branch.delete",
  "branch.push",
  "branch.push_rejected",
  "license.delete",
  "license.denied",
  "license.put",
  "member.put",
  "member.remove",
  "mr.approve",
  "mr.close",
  "mr.merge",
  "mr.open",
  "protection.delete",
  "protection.put",
  "tag.create",
  "tag.delete",
  "tag.push",
  "tenant.create",
  "token.create",
  "token.revoke",
  "tree.create",
  "tree.delete",
  "tree.update",
];

/** `GET /audit` also takes `since` (RFC 3339), beyond the shared AuditQuery fields. */
type AuditListQuery = AuditQuery & { since?: string };

/** Local midnight of a `YYYY-MM-DD` date input as RFC 3339 (UTC), or undefined. */
function sinceParam(date: string): string | undefined {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) return undefined;
  const t = new Date(`${date}T00:00:00`);
  return Number.isNaN(t.getTime()) ? undefined : t.toISOString().replace(/\.\d{3}Z$/, "Z");
}

const CSV_HEADER = ["at", "actor", "actor_id", "action", "resource_type", "resource_id", "tenant", "remote_addr", "details"];

function auditCsv(events: readonly AuditEvent[], slug: (id: string) => string | undefined): string {
  return toCsv(
    CSV_HEADER,
    events.map((e) => [
      e.at,
      e.actor,
      e.actor_id,
      e.action,
      e.resource_type,
      e.resource_id,
      slug(e.tenant_id) ?? e.tenant_id,
      e.remote_addr,
      e.details && Object.keys(e.details).length > 0 ? JSON.stringify(e.details) : "",
    ]),
  );
}

export function AuditPage() {
  const tenants = useTenants();
  const [params, setParams] = useSearchParams();
  const tenant = params.get("tenant") ?? "";
  const action = params.get("action") ?? "";
  const actor = params.get("actor") ?? "";
  const since = params.get("since") ?? "";
  const limit = Math.min(MAX_LIMIT, Math.max(PAGE, Number(params.get("limit")) || PAGE));

  const [draft, setDraft] = useState({ action, actor, since });
  const query: AuditListQuery = { tenant: tenant || undefined, action: action || undefined, since: sinceParam(since), limit };
  const audit = useAudit(query);

  const slugOf = (id: string) => tenants.data?.find((t) => t.id === id)?.slug;
  const actorTerm = actor.toLowerCase();
  const rows = (audit.data ?? []).filter((e) => !actorTerm || e.actor.toLowerCase().includes(actorTerm));
  const canLoadMore = (audit.data?.length ?? 0) >= limit && limit < MAX_LIMIT;
  const filtered = FILTER_KEYS.some((k) => params.has(k));

  const apply = (next: Record<string, string>) =>
    setParams(
      (p) => {
        for (const [k, v] of Object.entries(next)) {
          if (v) p.set(k, v);
          else p.delete(k);
        }
        // Filters change the result set: start again from the first page.
        if (!("limit" in next)) p.delete("limit");
        return p;
      },
      { replace: true },
    );

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    apply({ action: draft.action.trim(), actor: draft.actor.trim(), since: draft.since });
  };

  const clear = () => {
    setDraft({ action: "", actor: "", since: "" });
    apply({ tenant: "", action: "", actor: "", since: "" });
  };

  const exportCsv = () => {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[-:]/g, "").replace("T", "-");
    downloadText(`audit-${stamp}.csv`, auditCsv(rows, slugOf));
  };

  return (
    <>
      <PageHeader
        title="Audit log"
        breadcrumbs={[{ label: "Audit log" }]}
        description="Security-relevant actions, newest first. Requires tenant or server admin."
        actions={
          <Button size="sm" variant="outline" onClick={exportCsv} disabled={rows.length === 0}>
            <Download aria-hidden="true" />
            Export CSV
          </Button>
        }
      />
      <Card className="gap-0 py-0">
        <form onSubmit={onSubmit} aria-label="Audit filters" className="flex flex-wrap items-end gap-3 border-b p-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="audit-tenant">Tenant</Label>
            <Select value={tenant || ALL_TENANTS} onValueChange={(v) => apply({ tenant: v === ALL_TENANTS ? "" : v })}>
              <SelectTrigger id="audit-tenant" className="w-44">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL_TENANTS}>All tenants</SelectItem>
                {(tenants.data ?? []).map((t) => (
                  <SelectItem key={t.id} value={t.slug}>
                    {t.slug}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="audit-action">Action</Label>
            <Input
              id="audit-action"
              list="audit-actions"
              value={draft.action}
              onChange={(e) => setDraft((d) => ({ ...d, action: e.target.value }))}
              placeholder="e.g. branch.push"
              className="w-52 font-mono text-xs"
            />
            <datalist id="audit-actions">
              {KNOWN_ACTIONS.map((a) => (
                <option key={a} value={a} />
              ))}
            </datalist>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="audit-actor">Actor</Label>
            <Input
              id="audit-actor"
              value={draft.actor}
              onChange={(e) => setDraft((d) => ({ ...d, actor: e.target.value }))}
              placeholder="Username"
              className="w-40"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="audit-since">Since</Label>
            <Input
              id="audit-since"
              type="date"
              value={draft.since}
              onChange={(e) => setDraft((d) => ({ ...d, since: e.target.value }))}
              className="w-40"
            />
          </div>
          <div className="flex gap-2">
            <Button type="submit">Apply</Button>
            {filtered && (
              <Button type="button" variant="ghost" onClick={clear}>
                Clear filters
              </Button>
            )}
          </div>
        </form>

        <CardHeader className="py-3 [.border-b]:pb-3 border-b">
          <CardTitle className="text-sm">Events</CardTitle>
          <CardDescription aria-live="polite">
            {audit.data
              ? `${rows.length} ${rows.length === 1 ? "event" : "events"}${actor ? ` by actors matching “${actor}”` : ""} · ${audit.data.length} loaded`
              : "Loading…"}
          </CardDescription>
          {audit.isFetching && !audit.isPending && (
            <CardAction>
              <span className="text-xs text-muted-foreground">Refreshing…</span>
            </CardAction>
          )}
        </CardHeader>

        {audit.isPending ? (
          <SkeletonRows rows={8} label="Loading audit events" />
        ) : audit.isError ? (
          <ErrorState error={audit.error} onRetry={() => void audit.refetch()} />
        ) : rows.length === 0 ? (
          <EmptyState icon={ScrollText} title="No matching events">
            {filtered ? "Try widening the filters." : "Audited actions appear here as they happen."}
          </EmptyState>
        ) : (
          <AuditTable events={rows} tenantSlug={slugOf} />
        )}

        {canLoadMore && !audit.isError && (
          <div className="flex justify-center border-t p-3">
            <Button
              variant="outline"
              size="sm"
              disabled={audit.isFetching}
              onClick={() => apply({ limit: String(Math.min(MAX_LIMIT, limit + PAGE)) })}
            >
              {audit.isFetching ? "Loading…" : `Load ${PAGE} more`}
            </Button>
          </div>
        )}
      </Card>
    </>
  );
}
