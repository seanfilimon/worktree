import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Gauge } from "lucide-react";
import { useState, type FormEvent } from "react";
import { keys } from "@/api/queries";
import {
  compactLimits,
  isNotImplemented,
  opsKeys,
  useOpsApi,
  useTenantLimits,
  useTenantUsage,
  type LimitKey,
  type TenantLimits,
  type TenantUsage,
} from "@/api/v05ops";
import { FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatBytes, formatNumber } from "@/lib/format";
import { useToast } from "@/toast/store";
import { NotImplementedNotice } from "./opsShared";

const COUNT_FIELDS: { key: Exclude<LimitKey, "max_storage_bytes">; label: string; hint: string }[] = [
  { key: "max_trees", label: "Max trees", hint: "Trees in this tenant" },
  { key: "max_branches_per_tree", label: "Max branches per tree", hint: "Enforced on push" },
  { key: "max_members", label: "Max members", hint: "Tenant memberships" },
  { key: "requests_per_minute", label: "Requests per minute", hint: "Per principal" },
];

const UNITS = { MiB: 1024 ** 2, GiB: 1024 ** 3, TiB: 1024 ** 4 } as const;
type Unit = keyof typeof UNITS;

/** Largest unit that represents `bytes` exactly (GiB when empty). */
function splitBytes(bytes: number | undefined): { value: string; unit: Unit } {
  if (!bytes) return { value: "", unit: "GiB" };
  for (const unit of ["TiB", "GiB", "MiB"] as Unit[]) {
    if (bytes % UNITS[unit] === 0) return { value: String(bytes / UNITS[unit]), unit };
  }
  return { value: String(Math.round((bytes / UNITS.MiB) * 100) / 100), unit: "MiB" };
}

interface Draft {
  counts: Record<Exclude<LimitKey, "max_storage_bytes">, string>;
  storage: string;
  unit: Unit;
}

function toDraft(l: TenantLimits): Draft {
  const s = splitBytes(l.max_storage_bytes);
  return {
    counts: {
      max_trees: l.max_trees ? String(l.max_trees) : "",
      max_branches_per_tree: l.max_branches_per_tree ? String(l.max_branches_per_tree) : "",
      max_members: l.max_members ? String(l.max_members) : "",
      requests_per_minute: l.requests_per_minute ? String(l.requests_per_minute) : "",
    },
    storage: s.value,
    unit: s.unit,
  };
}

/** Parses the draft; returns field errors or the limits. */
function fromDraft(d: Draft): { limits: TenantLimits; errors: Partial<Record<LimitKey, string>> } {
  const limits: TenantLimits = {};
  const errors: Partial<Record<LimitKey, string>> = {};
  for (const f of COUNT_FIELDS) {
    const raw = d.counts[f.key].trim();
    if (raw === "") continue;
    const n = Number(raw);
    if (!Number.isInteger(n) || n < 0) errors[f.key] = "Whole number, 0 or more.";
    else if (n > 0) limits[f.key] = n;
  }
  const rawStorage = d.storage.trim();
  if (rawStorage !== "") {
    const n = Number(rawStorage);
    if (!Number.isFinite(n) || n < 0) errors.max_storage_bytes = "A positive size.";
    else if (n > 0) limits.max_storage_bytes = Math.round(n * UNITS[d.unit]);
  }
  return { limits, errors };
}

function sameLimits(a: TenantLimits, b: TenantLimits): boolean {
  const x = compactLimits(a);
  const y = compactLimits(b);
  return JSON.stringify(x, Object.keys(x).sort()) === JSON.stringify(y, Object.keys(y).sort());
}

function UsageRow({ label, used, limit, format }: { label: string; used: number; limit: number | undefined; format: (n: number) => string }) {
  const pct = limit ? Math.min(100, Math.round((used / limit) * 100)) : undefined;
  return (
    <TableRow>
      <TableCell className="pl-6 font-medium">{label}</TableCell>
      <TableCell className="font-mono text-xs tabular-nums">{format(used)}</TableCell>
      <TableCell className="font-mono text-xs tabular-nums">
        {limit ? format(limit) : <span className="text-muted-foreground">unlimited</span>}
      </TableCell>
      <TableCell className="hidden w-48 pr-6 sm:table-cell">
        {pct !== undefined && (
          <div className="flex items-center gap-2">
            <Progress value={pct} aria-label={`${label} used`} className="h-1.5" />
            <span className={pct >= 90 ? "text-xs font-semibold tabular-nums" : "text-xs tabular-nums text-muted-foreground"}>
              {pct}%
            </span>
          </div>
        )}
      </TableCell>
    </TableRow>
  );
}

/** Current usage against the effective limits. */
export function UsageTable({ usage }: { usage: TenantUsage }) {
  const e = usage.effective;
  return (
    <Table aria-label="Usage">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead className="pl-6">Resource</TableHead>
          <TableHead>Used</TableHead>
          <TableHead>Effective limit</TableHead>
          <TableHead className="hidden pr-6 sm:table-cell">
            <span className="sr-only">Share used</span>
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <UsageRow label="Trees" used={usage.trees} limit={e.max_trees} format={formatNumber} />
        <UsageRow label="Members" used={usage.members} limit={e.max_members} format={formatNumber} />
        <UsageRow label="Storage" used={usage.storage_bytes} limit={e.max_storage_bytes} format={formatBytes} />
        <TableRow>
          <TableCell className="pl-6 font-medium">Branches</TableCell>
          <TableCell className="font-mono text-xs tabular-nums">{formatNumber(usage.branches)}</TableCell>
          <TableCell className="font-mono text-xs tabular-nums">
            {e.max_branches_per_tree ? (
              `${formatNumber(e.max_branches_per_tree)} per tree`
            ) : (
              <span className="text-muted-foreground">unlimited</span>
            )}
          </TableCell>
          <TableCell className="hidden sm:table-cell" />
        </TableRow>
        <TableRow>
          <TableCell className="pl-6 font-medium">Request rate</TableCell>
          <TableCell className="text-xs text-muted-foreground">—</TableCell>
          <TableCell className="font-mono text-xs tabular-nums">
            {e.requests_per_minute ? (
              `${formatNumber(e.requests_per_minute)} / min`
            ) : (
              <span className="text-muted-foreground">server default</span>
            )}
          </TableCell>
          <TableCell className="hidden sm:table-cell" />
        </TableRow>
      </TableBody>
    </Table>
  );
}

function LimitsEditor({ slug, current }: { slug: string; current: TenantLimits }) {
  const ops = useOpsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [draft, setDraft] = useState<Draft>(() => toDraft(current));
  const { limits, errors } = fromDraft(draft);
  const invalid = Object.keys(errors).length > 0;
  const dirty = !sameLimits(limits, current);

  const save = useMutation({
    mutationFn: (l: TenantLimits) => ops.tenants.setLimits(slug, l),
    meta: { inlineError: true },
    onSuccess: async (saved) => {
      queryClient.setQueryData(opsKeys.limits(slug), saved);
      setDraft(toDraft(saved));
      toast.success("Limits saved", slug);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: opsKeys.usage(slug) }),
        queryClient.invalidateQueries({ queryKey: keys.tenant(slug) }),
      ]);
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (!invalid && dirty) save.mutate(limits);
  };

  const hasOverrides = Object.keys(compactLimits(current)).length > 0;

  return (
    <form onSubmit={onSubmit} aria-label="Tenant limits" noValidate className="flex flex-col">
      <CardContent className="grid gap-4 border-t pt-6 sm:grid-cols-2 lg:grid-cols-3">
        {COUNT_FIELDS.map((f) => {
          const id = `limit-${slug}-${f.key}`;
          const err = errors[f.key];
          return (
            <div key={f.key} className="grid gap-1.5">
              <Label htmlFor={id}>{f.label}</Label>
              <Input
                id={id}
                inputMode="numeric"
                placeholder="Server default"
                value={draft.counts[f.key]}
                aria-invalid={err ? true : undefined}
                aria-describedby={`${id}-hint`}
                onChange={(e) => setDraft((d) => ({ ...d, counts: { ...d.counts, [f.key]: e.target.value } }))}
                className="font-mono"
              />
              <p id={`${id}-hint`} className={err ? "text-xs font-medium" : "text-xs text-muted-foreground"}>
                {err ?? f.hint}
              </p>
            </div>
          );
        })}
        <div className="grid gap-1.5">
          <Label htmlFor={`limit-${slug}-storage`}>Max storage</Label>
          <div className="flex gap-2">
            <Input
              id={`limit-${slug}-storage`}
              inputMode="decimal"
              placeholder="Server default"
              value={draft.storage}
              aria-invalid={errors.max_storage_bytes ? true : undefined}
              onChange={(e) => setDraft((d) => ({ ...d, storage: e.target.value }))}
              className="font-mono"
            />
            <Select value={draft.unit} onValueChange={(v) => setDraft((d) => ({ ...d, unit: v as Unit }))}>
              <SelectTrigger aria-label="Storage unit" className="w-24">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(Object.keys(UNITS) as Unit[]).map((u) => (
                  <SelectItem key={u} value={u}>
                    {u}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <p className={errors.max_storage_bytes ? "text-xs font-medium" : "text-xs text-muted-foreground"}>
            {errors.max_storage_bytes ?? "Object bytes of accepted pushes"}
          </p>
        </div>
        <div className="sm:col-span-2 lg:col-span-3">
          <FormError error={save.error} />
        </div>
      </CardContent>
      <CardFooter className="flex-wrap justify-between gap-2 border-t py-4">
        <span className="text-xs text-muted-foreground">Empty or 0 = server default.</span>
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            disabled={!hasOverrides || save.isPending}
            onClick={() => save.mutate({})}
          >
            Clear overrides
          </Button>
          <Button type="button" variant="outline" disabled={!dirty || save.isPending} onClick={() => setDraft(toDraft(current))}>
            Reset
          </Button>
          <Button type="submit" disabled={!dirty || invalid || save.isPending}>
            {save.isPending ? "Saving…" : "Save limits"}
          </Button>
        </div>
      </CardFooter>
    </form>
  );
}

/** Usage and quotas of one tenant; server admins edit the overrides. */
export function TenantLimitsCard({ slug, editable }: { slug: string; editable: boolean }) {
  const usage = useTenantUsage(slug);
  const limits = useTenantLimits(slug, editable);
  const unavailable = isNotImplemented(usage.error) || isNotImplemented(limits.error);

  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle className="flex items-center gap-2">
          <Gauge aria-hidden="true" className="size-4" />
          Usage and limits
        </CardTitle>
        <CardDescription>
          {editable
            ? "Tenant overrides take precedence over the server defaults."
            : "Quotas are set by server admins."}
        </CardDescription>
      </CardHeader>
      {unavailable ? (
        <div className="p-4">
          <NotImplementedNotice feature="Tenant usage and limits" />
        </div>
      ) : usage.isPending ? (
        <TableSkeleton label="Loading usage" rows={3} columns={3} />
      ) : usage.isError ? (
        <QueryError error={usage.error} onRetry={() => void usage.refetch()} title="Could not load usage" />
      ) : (
        <UsageTable usage={usage.data} />
      )}
      {editable && !unavailable && (
        limits.isPending ? (
          <TableSkeleton label="Loading limits" rows={2} columns={3} />
        ) : limits.isError ? (
          <QueryError error={limits.error} onRetry={() => void limits.refetch()} title="Could not load limits" />
        ) : (
          <LimitsEditor key={JSON.stringify(limits.data)} slug={slug} current={limits.data} />
        )
      )}
    </Card>
  );
}
