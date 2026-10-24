import { useState, type FormEvent } from "react";
import { useSearchParams } from "react-router";
import { useAudit, useTenants } from "@/api/queries";
import { PageHeader } from "@/components/PageHeader";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Section } from "@/components/ui/Card";
import { SelectField, TextField } from "@/components/ui/Field";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { Table, Td, Th, Tr } from "@/components/ui/Table";
import { RelativeTime } from "@/components/ui/Time";

const LIMITS = [50, 100, 200, 500];
const COMMON_ACTIONS = [
  "auth.login",
  "auth.register",
  "branch.push",
  "tree.create",
  "tree.delete",
  "member.set",
  "member.remove",
  "mr.open",
  "mr.approve",
  "mr.merge",
  "mr.close",
  "protection.set",
  "license.set",
];

function actionTone(action: string) {
  if (action.startsWith("auth.")) return "info" as const;
  if (action.endsWith(".delete") || action.endsWith(".remove")) return "danger" as const;
  if (action.startsWith("branch.") || action.startsWith("mr.")) return "accent" as const;
  return "neutral" as const;
}

export function AuditPage() {
  const tenants = useTenants();
  const [params, setParams] = useSearchParams();
  const tenant = params.get("tenant") ?? "";
  const action = params.get("action") ?? "";
  const limit = Number(params.get("limit")) || 100;
  const [actionDraft, setActionDraft] = useState(action);
  const audit = useAudit({ tenant: tenant || undefined, action: action || undefined, limit });

  const apply = (next: Record<string, string>) =>
    setParams(
      (p) => {
        for (const [k, v] of Object.entries(next)) {
          if (v) p.set(k, v);
          else p.delete(k);
        }
        return p;
      },
      { replace: true },
    );

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    apply({ action: actionDraft.trim() });
  };

  return (
    <>
      <PageHeader title="Audit log" breadcrumbs={[{ label: "Audit log" }]} description="Security-relevant actions, newest first. Requires tenant or server admin." />
      <Section
        title="Events"
        flush
        actions={audit.isFetching && !audit.isPending ? <span className="text-xs text-muted">Refreshing…</span> : undefined}
      >
        <form onSubmit={onSubmit} aria-label="Audit filters" className="flex flex-wrap items-end gap-3 border-b border-border p-4">
          <SelectField label="Tenant" value={tenant} onChange={(e) => apply({ tenant: e.target.value })} fieldClassName="w-48">
            <option value="">All tenants</option>
            {(tenants.data ?? []).map((t) => (
              <option key={t.id} value={t.slug}>
                {t.slug}
              </option>
            ))}
          </SelectField>
          <TextField
            label="Action"
            list="audit-actions"
            value={actionDraft}
            onChange={(e) => setActionDraft(e.target.value)}
            placeholder="e.g. branch.push"
            fieldClassName="w-56"
          />
          <datalist id="audit-actions">
            {COMMON_ACTIONS.map((a) => (
              <option key={a} value={a} />
            ))}
          </datalist>
          <SelectField label="Limit" value={String(limit)} onChange={(e) => apply({ limit: e.target.value })} fieldClassName="w-24">
            {LIMITS.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </SelectField>
          <Button type="submit">Apply</Button>
          {(tenant || action) && (
            <Button
              variant="ghost"
              onClick={() => {
                setActionDraft("");
                apply({ tenant: "", action: "" });
              }}
            >
              Clear filters
            </Button>
          )}
        </form>
        {audit.isPending ? (
          <SkeletonRows rows={8} label="Loading audit events" />
        ) : audit.isError ? (
          <ErrorState error={audit.error} onRetry={() => void audit.refetch()} />
        ) : audit.data.length === 0 ? (
          <EmptyState title="No matching events" />
        ) : (
          <Table label="Audit events">
            <thead>
              <tr>
                <Th>When</Th>
                <Th>Actor</Th>
                <Th>Action</Th>
                <Th>Resource</Th>
                <Th>Details</Th>
                <Th>Source</Th>
              </tr>
            </thead>
            <tbody>
              {audit.data.map((e) => (
                <Tr key={e.id}>
                  <Td className="whitespace-nowrap">
                    <RelativeTime value={e.at} className="text-muted" />
                  </Td>
                  <Td className="font-medium">{e.actor}</Td>
                  <Td>
                    <Badge tone={actionTone(e.action)}>{e.action}</Badge>
                  </Td>
                  <Td className="font-mono text-xs">
                    <span className="text-muted">{e.resource_type}</span>
                    {e.resource_id && <> {e.resource_id}</>}
                  </Td>
                  <Td className="max-w-sm">
                    {e.details && Object.keys(e.details).length > 0 ? (
                      <dl className="flex flex-wrap gap-x-3 gap-y-0.5 text-xs">
                        {Object.entries(e.details).map(([k, v]) => (
                          <div key={k} className="flex gap-1">
                            <dt className="text-muted">{k}</dt>
                            <dd className="font-mono break-all">{v}</dd>
                          </div>
                        ))}
                      </dl>
                    ) : (
                      <span className="text-subtle">—</span>
                    )}
                  </Td>
                  <Td className="font-mono text-xs text-muted">{e.remote_addr || "—"}</Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </Section>
    </>
  );
}
