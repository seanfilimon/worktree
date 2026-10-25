import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArchiveX, ArrowRight, GitCompareArrows, History, RotateCcw } from "lucide-react";
import { useId, useState } from "react";
import { errorMessage, isApiError } from "@/api/client";
import { iamKeys, type Policy, type PolicyStatement } from "@/api/iam";
import { isUnavailable, useDeletedPolicies, usePolicyDiff, usePolicyVersions, useV06Api, v06Keys, type PolicyDiffChange, type PolicyVersion } from "@/api/v06";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Textarea } from "@/components/ui/textarea";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { absoluteTime, relativeTime } from "@/lib/format";
import { lineDiff } from "@/lib/lineDiff";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";

const CHANGE_LABEL: Record<string, string> = { create: "created", update: "updated", delete: "deleted", rollback: "rolled back" };

function ChangeBadge({ change }: { change: string }) {
  const label = CHANGE_LABEL[change] ?? change;
  return (
    <Badge variant={change === "rollback" ? "default" : "outline"} className={cn(change === "delete" && "line-through")}>
      {label}
    </Badge>
  );
}

function pretty(s: PolicyStatement | undefined): string {
  return s ? JSON.stringify(s, null, 2) : "";
}

/** Unified line diff of two JSON texts: added lines bold with "+", removed struck through with "-". */
function JsonDiff({ before, after, label }: { before: string; after: string; label: string }) {
  const rows = lineDiff(before, after);
  return (
    <pre aria-label={label} className="overflow-x-auto rounded-md border bg-muted/30 py-2 font-mono text-[11px] leading-relaxed">
      {rows.map((r, i) => (
        <div
          key={i}
          data-kind={r.kind}
          className={cn(
            "px-2 whitespace-pre",
            r.kind === "add" && "bg-foreground/10 font-bold",
            r.kind === "del" && "text-muted-foreground line-through",
          )}
        >
          <span aria-hidden="true" className="inline-block w-3 select-none">
            {r.kind === "add" ? "+" : r.kind === "del" ? "-" : " "}
          </span>
          {r.text}
        </div>
      ))}
    </pre>
  );
}

const OP_LABEL: Record<string, string> = { add: "added", remove: "removed", change: "changed" };

function DiffChange({ c }: { c: PolicyDiffChange }) {
  const before = c.op === "add" ? "" : pretty(c.before ?? c.statement);
  const after = c.op === "remove" ? "" : pretty(c.after ?? c.statement);
  return (
    <li className="flex flex-col gap-2" data-op={c.op}>
      <p className="flex flex-wrap items-center gap-2 text-sm">
        <Badge variant={c.op === "add" ? "default" : "outline"} className={cn(c.op === "remove" && "line-through")}>
          {OP_LABEL[c.op] ?? c.op}
        </Badge>
        <span className="font-mono font-semibold">{c.sid || "(statement without sid)"}</span>
      </p>
      <JsonDiff before={before} after={after} label={`Statement ${c.sid || ""} diff`} />
    </li>
  );
}

/** Name / description / enabled changes between two stored versions (client-side). */
function metaChanges(a: Policy | undefined, b: Policy | undefined): { field: string; from: string; to: string }[] {
  if (!a || !b) return [];
  const out: { field: string; from: string; to: string }[] = [];
  if (a.name !== b.name) out.push({ field: "Name", from: a.name, to: b.name });
  if ((a.description ?? "") !== (b.description ?? "")) out.push({ field: "Description", from: a.description || "—", to: b.description || "—" });
  if (a.enabled !== b.enabled) out.push({ field: "Enabled", from: a.enabled ? "yes" : "no", to: b.enabled ? "yes" : "no" });
  return out;
}

function DiffPanel({ tenant, policyId, versions, from, to }: { tenant: string; policyId: string; versions: PolicyVersion[]; from: number; to: number }) {
  const diff = usePolicyDiff(tenant, policyId, from, to);
  const meta = metaChanges(versions.find((v) => v.version === from)?.policy, versions.find((v) => v.version === to)?.policy);
  if (from === to) return <p className="text-sm text-muted-foreground">Pick two different versions to compare.</p>;
  return (
    <section aria-label={`Changes from v${from} to v${to}`} className="flex flex-col gap-3">
      {meta.length > 0 && (
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
          {meta.map((m) => (
            <div key={m.field} className="contents">
              <dt className="text-muted-foreground">{m.field}</dt>
              <dd className="flex flex-wrap items-center gap-1.5">
                <span className="line-through">{m.from}</span>
                <ArrowRight aria-hidden="true" className="size-3.5" />
                <span className="font-semibold">{m.to}</span>
              </dd>
            </div>
          ))}
        </dl>
      )}
      {diff.isPending ? (
        <TableSkeleton label="Loading diff" rows={2} columns={2} />
      ) : diff.isError ? (
        isUnavailable(diff.error) ? (
          <UnavailableNotice feature="Policy diff" />
        ) : (
          <QueryError error={diff.error} onRetry={() => void diff.refetch()} title="Could not load the diff" />
        )
      ) : diff.data.changes.length === 0 ? (
        <p className="text-sm text-muted-foreground">No statement changes between v{from} and v{to}.</p>
      ) : (
        <ul aria-label="Statement changes" className="flex flex-col gap-4">
          {diff.data.changes.map((c, i) => (
            <DiffChange key={`${c.sid}:${i}`} c={c} />
          ))}
        </ul>
      )}
    </section>
  );
}

function RollbackDialog({
  tenant,
  policy,
  version,
  onClose,
}: {
  tenant: string;
  policy: { id: string; name: string };
  version: number;
  onClose: () => void;
}) {
  const api = useV06Api();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [comment, setComment] = useState("");
  const [force, setForce] = useState(false);
  const rollback = useMutation({
    mutationFn: () => api.policyHistory.rollback(tenant, policy.id, version, comment, force),
    meta: { inlineError: true },
    onSuccess: async () => {
      toast.success(`Rolled back to v${version}`, policy.name);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: iamKeys.policies(tenant) }),
        queryClient.invalidateQueries({ queryKey: v06Keys.policyVersions(tenant, policy.id) }),
      ]);
      onClose();
    },
  });
  const managed = isApiError(rollback.error) && rollback.error.code === "managed_by_config";

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            Roll back {policy.name} to v{version}?
          </DialogTitle>
          <DialogDescription>A new version is created with the statements of v{version}. History is kept.</DialogDescription>
        </DialogHeader>
        <form
          id={`${id}-form`}
          aria-label="Roll back policy"
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            rollback.mutate();
          }}
        >
          <div className="grid gap-2">
            <Label htmlFor={`${id}-comment`}>Comment</Label>
            <Textarea
              id={`${id}-comment`}
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              rows={3}
              maxLength={500}
              placeholder="Why roll back? (optional, recorded in the history)"
            />
          </div>
          {managed ? (
            <Alert>
              <AlertTitle>Managed by config</AlertTitle>
              <AlertDescription>
                <p>{errorMessage(rollback.error)}</p>
                <p>The next access-config apply overwrites it again. Tenant owners may override.</p>
                <Label className="mt-1 flex items-center gap-2 font-normal text-foreground">
                  <Checkbox checked={force} onCheckedChange={(v) => setForce(v === true)} />
                  Override config management (owners only)
                </Label>
              </AlertDescription>
            </Alert>
          ) : (
            <FormError error={rollback.error} />
          )}
        </form>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form={`${id}-form`} disabled={rollback.isPending || (managed && !force)}>
            <RotateCcw aria-hidden="true" />
            {rollback.isPending ? "Rolling back…" : `Roll back to v${version}`}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface PolicyHistorySheetProps {
  tenant: string;
  /** null = closed. */
  policy: { id: string; name: string } | null;
  editable: boolean;
  /** Deleted policies cannot be rolled back from here. */
  deleted?: boolean;
  onClose: () => void;
}

/** Version history of a policy: list, diff between any two versions, rollback with comment. */
export function PolicyHistorySheet({ tenant, policy, editable, deleted = false, onClose }: PolicyHistorySheetProps) {
  const id = useId();
  const versions = usePolicyVersions(tenant, policy?.id ?? "", policy !== null);
  const list = versions.data ?? [];
  const latest = list[0]?.version ?? 0;
  const [range, setRange] = useState<{ from: number; to: number } | null>(null);
  const [rollingBack, setRollingBack] = useState(0);
  const to = range?.to ?? latest;
  const from = range?.from ?? (list[1]?.version ?? 0);

  return (
    <Sheet
      open={policy !== null}
      onOpenChange={(o) => {
        if (!o) {
          setRange(null);
          onClose();
        }
      }}
    >
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-2xl">
        <SheetHeader className="border-b">
          <SheetTitle className="flex items-center gap-2">
            <History aria-hidden="true" className="size-4" />
            Version history
          </SheetTitle>
          <SheetDescription>
            Policy <span className="font-mono text-foreground">{policy?.name}</span> in {tenant}
          </SheetDescription>
        </SheetHeader>
        {policy &&
          (versions.isPending ? (
            <TableSkeleton label="Loading versions" rows={4} />
          ) : versions.isError ? (
            isUnavailable(versions.error) ? (
              <div className="p-4">
                <UnavailableNotice feature="Policy history" />
              </div>
            ) : (
              <QueryError error={versions.error} onRetry={() => void versions.refetch()} title="Could not load the history" />
            )
          ) : list.length === 0 ? (
            <EmptyState icon={History} title="No versions recorded">
              Versions are recorded for every change from now on.
            </EmptyState>
          ) : (
            <div className="flex flex-col gap-6 p-4">
              <ol aria-label="Policy versions" className="flex flex-col divide-y rounded-md border">
                {list.map((v, i) => {
                  const prev = list[i + 1]?.version;
                  return (
                    <li key={v.version} className="flex flex-wrap items-start gap-x-3 gap-y-1 px-3 py-2.5">
                      <span className="font-mono text-sm font-semibold">v{v.version}</span>
                      <ChangeBadge change={v.change} />
                      {v.version === latest && <Badge variant="outline">current</Badge>}
                      <span className="min-w-0 flex-1 text-xs text-muted-foreground">
                        <span className="font-medium text-foreground">{v.changed_by || "system"}</span> ·{" "}
                        <time dateTime={v.changed_at} title={absoluteTime(v.changed_at)}>
                          {relativeTime(v.changed_at)}
                        </time>
                        {v.comment && <span className="block text-foreground italic">“{v.comment}”</span>}
                      </span>
                      <span className="flex gap-1">
                        {prev !== undefined && (
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            aria-label={`Compare v${prev} with v${v.version}`}
                            onClick={() => setRange({ from: prev, to: v.version })}
                          >
                            <GitCompareArrows aria-hidden="true" />
                          </Button>
                        )}
                        {editable && !deleted && v.version !== latest && v.change !== "delete" && (
                          <Button variant="ghost" size="icon-sm" aria-label={`Roll back to v${v.version}`} onClick={() => setRollingBack(v.version)}>
                            <RotateCcw aria-hidden="true" />
                          </Button>
                        )}
                      </span>
                    </li>
                  );
                })}
              </ol>
              {list.length > 1 && (
                <section aria-labelledby={`${id}-diff`} className="flex flex-col gap-3">
                  <div className="flex flex-wrap items-end gap-2">
                    <h3 id={`${id}-diff`} className="mr-auto text-sm font-semibold">
                      Compare versions
                    </h3>
                    <VersionSelect label="From version" value={from} versions={list} onChange={(v) => setRange({ from: v, to })} />
                    <ArrowRight aria-hidden="true" className="mb-2 size-4" />
                    <VersionSelect label="To version" value={to} versions={list} onChange={(v) => setRange({ from, to: v })} />
                  </div>
                  <DiffPanel tenant={tenant} policyId={policy.id} versions={list} from={from} to={to} />
                </section>
              )}
            </div>
          ))}
        {policy && rollingBack > 0 && <RollbackDialog tenant={tenant} policy={policy} version={rollingBack} onClose={() => setRollingBack(0)} />}
      </SheetContent>
    </Sheet>
  );
}

/** Policies deleted from the tenant; their history stays readable. */
export function DeletedPoliciesSheet({
  tenant,
  open,
  onClose,
  onHistory,
}: {
  tenant: string;
  open: boolean;
  onClose: () => void;
  onHistory: (p: Policy) => void;
}) {
  const deleted = useDeletedPolicies(tenant, open);
  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-lg">
        <SheetHeader className="border-b">
          <SheetTitle className="flex items-center gap-2">
            <ArchiveX aria-hidden="true" className="size-4" />
            Deleted policies
          </SheetTitle>
          <SheetDescription>Deleted policies no longer apply; their version history is kept.</SheetDescription>
        </SheetHeader>
        {open &&
          (deleted.isPending ? (
            <TableSkeleton label="Loading deleted policies" rows={3} />
          ) : deleted.isError ? (
            isUnavailable(deleted.error) ? (
              <div className="p-4">
                <UnavailableNotice feature="Deleted policies" />
              </div>
            ) : (
              <QueryError error={deleted.error} onRetry={() => void deleted.refetch()} title="Could not load deleted policies" />
            )
          ) : deleted.data.length === 0 ? (
            <EmptyState icon={ArchiveX} title="No deleted policies" />
          ) : (
            <ul aria-label="Deleted policies" className="divide-y">
              {deleted.data.map((p) => (
                <li key={p.id} className="flex items-center gap-3 px-4 py-2.5">
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-medium line-through">{p.name}</span>
                    {p.description && <span className="block truncate text-xs text-muted-foreground">{p.description}</span>}
                  </span>
                  <Button variant="outline" size="sm" aria-label={`History of ${p.name}`} onClick={() => onHistory(p)}>
                    <History aria-hidden="true" />
                    History
                  </Button>
                </li>
              ))}
            </ul>
          ))}
      </SheetContent>
    </Sheet>
  );
}

function VersionSelect({ label, value, versions, onChange }: { label: string; value: number; versions: PolicyVersion[]; onChange: (v: number) => void }) {
  return (
    <Select value={String(value)} onValueChange={(v) => onChange(Number(v))}>
      <SelectTrigger size="sm" aria-label={label} className="w-24 font-mono">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {versions.map((v) => (
          <SelectItem key={v.version} value={String(v.version)} className="font-mono">
            v{v.version}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
