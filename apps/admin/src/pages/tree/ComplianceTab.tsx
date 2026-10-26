/**
 * Secret scanning and license compliance of a tree (docs/ENGINEERING_PLAN.md
 * §16.7): the scanning mode and its exclusions, findings recorded on push
 * (resolve, reopen, allowlist), the allowlist, and the license audit of the
 * default branch. The endpoints and helpers live in ./complianceApi.
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2,
  FileWarning,
  KeyRound,
  RotateCcw,
  Scale,
  ShieldAlert,
  ShieldCheck,
  ShieldOff,
  Trash2,
} from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { isUnavailable, retryV06 } from "@/api/v06";
import { ChipsInput } from "@/components/files/ChipsInput";
import {
  EmptyState,
  FormError,
  QueryError,
  TableSkeleton,
} from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import {
  absoluteTime,
  formatNumber,
  relativeTime,
  shortId,
} from "@/lib/format";
import { ConfirmAction } from "@/pages/iam/shared";
import { useToast } from "@/toast/store";
import {
  allowFormErrors,
  complianceKeys,
  createComplianceApi,
  maxFileText,
  MODE_LABELS,
  RESOLUTION_LABELS,
  type AllowEntry,
  type ComplianceSettings,
  type FindingStatus,
  type LicenseAudit,
  type Resolution,
  type ScanMode,
  type SecretFinding,
} from "./complianceApi";
import { useTreeContext } from "./context";

function useComplianceApi() {
  const { client } = useApi();
  const { tenant, tree } = useTreeContext();
  return useMemo(
    () => createComplianceApi(client, tenant, tree),
    [client, tenant, tree],
  );
}

/** Invalidates every compliance query of the tree. */
function useInvalidate() {
  const queryClient = useQueryClient();
  const { tenant, tree } = useTreeContext();
  return () =>
    queryClient.invalidateQueries({
      queryKey: complianceKeys.all(tenant, tree),
    });
}

// ---- pieces ----

function ModeBadge({ mode }: { mode: ScanMode }) {
  const Icon =
    mode === "block" ? ShieldAlert : mode === "off" ? ShieldOff : ShieldCheck;
  return (
    <Badge variant={mode === "block" ? "default" : "outline"}>
      <Icon aria-hidden="true" />
      {MODE_LABELS[mode].label}
    </Badge>
  );
}

function ActionBadge({ f }: { f: SecretFinding }) {
  switch (f.action) {
    case "blocked":
      return <Badge variant="default">Blocked</Badge>;
    case "bypassed":
      return (
        <Badge variant="outline" className="font-semibold">
          Bypassed
        </Badge>
      );
    default:
      return <Badge variant="secondary">Warned</Badge>;
  }
}

function Fingerprint({ value }: { value: string }) {
  return (
    <span className="font-mono text-xs break-all" title={value}>
      {value}
    </span>
  );
}

// ---- settings ----

function SettingsCard({ settings }: { settings: ComplianceSettings }) {
  const { canManage } = useTreeContext();
  const api = useComplianceApi();
  const invalidate = useInvalidate();
  const toast = useToast();
  const id = useId();
  const [mode, setMode] = useState<ScanMode>(settings.mode);
  const [exclude, setExclude] = useState<string[]>(settings.exclude);
  // The tab remounts this card (key) whenever the stored settings change.
  const [maxMiB, setMaxMiB] = useState(
    settings.max_file_bytes ? String(settings.max_file_bytes / (1 << 20)) : "",
  );

  const maxValue = maxMiB.trim() === "" ? 0 : Number(maxMiB);
  const maxError =
    Number.isNaN(maxValue) || maxValue < 0 || maxValue > 16
      ? "Between 0 and 16 MiB."
      : "";
  const dirty =
    mode !== settings.mode ||
    exclude.join("\n") !== settings.exclude.join("\n") ||
    Math.round(maxValue * (1 << 20)) !== settings.max_file_bytes;

  const save = useMutation({
    mutationFn: () =>
      api.updateSettings({
        mode,
        exclude,
        max_file_bytes: Math.round(maxValue * (1 << 20)),
      }),
    meta: { inlineError: true },
    onSuccess: async (s) => {
      toast.success(
        "Scanning settings saved",
        `Mode: ${MODE_LABELS[s.mode].label}`,
      );
      await invalidate();
    },
  });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (maxError) return;
    save.mutate();
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <KeyRound aria-hidden="true" className="size-4" />
          Secret scanning
          <ModeBadge mode={settings.mode} />
        </CardTitle>
        <CardDescription>
          Every push is scanned with the same rules as{" "}
          <span className="font-mono">wt snapshot</span>: added and changed text
          files of each new snapshot. Lines marked{" "}
          <span className="font-mono">wt:allow-secret</span> are ignored.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <form
          onSubmit={submit}
          aria-label="Secret scanning settings"
          className="grid gap-5"
          noValidate
        >
          <RadioGroup
            value={mode}
            onValueChange={(v) => setMode(v as ScanMode)}
            disabled={!canManage}
            aria-label="Scanning mode"
            className="grid gap-2 sm:grid-cols-3"
          >
            {(Object.keys(MODE_LABELS) as ScanMode[]).map((m) => (
              <Label
                key={m}
                htmlFor={`${id}-${m}`}
                className="flex cursor-pointer items-start gap-3 rounded-md border p-3 font-normal has-[[data-state=checked]]:border-foreground"
              >
                <RadioGroupItem
                  id={`${id}-${m}`}
                  value={m}
                  className="mt-0.5"
                />
                <span className="grid gap-1">
                  <span className="font-medium">{MODE_LABELS[m].label}</span>
                  <span className="text-xs text-muted-foreground">
                    {MODE_LABELS[m].description}
                  </span>
                </span>
              </Label>
            ))}
          </RadioGroup>
          <div className="grid gap-4 md:grid-cols-[2fr_1fr]">
            <div className="grid gap-2">
              <Label htmlFor={`${id}-exclude`}>Excluded paths</Label>
              {canManage ? (
                <ChipsInput
                  id={`${id}-exclude`}
                  values={exclude}
                  onChange={setExclude}
                  placeholder="fixtures/**, *.lock"
                  describedBy={`${id}-exclude-hint`}
                />
              ) : (
                <p id={`${id}-exclude`} className="font-mono text-sm">
                  {settings.exclude.length
                    ? settings.exclude.join(", ")
                    : "none"}
                </p>
              )}
              <p
                id={`${id}-exclude-hint`}
                className="text-xs text-muted-foreground"
              >
                License-rule globs (<span className="font-mono">**</span> across
                directories) never scanned.
              </p>
            </div>
            <div className="grid gap-2">
              <Label htmlFor={`${id}-max`}>Largest file scanned (MiB)</Label>
              <Input
                id={`${id}-max`}
                inputMode="decimal"
                value={maxMiB}
                placeholder="1"
                disabled={!canManage}
                onChange={(e) => setMaxMiB(e.target.value)}
                aria-invalid={maxError ? true : undefined}
                aria-describedby={`${id}-max-hint`}
              />
              <p
                id={`${id}-max-hint`}
                className={
                  maxError
                    ? "text-xs font-semibold"
                    : "text-xs text-muted-foreground"
                }
              >
                {maxError ||
                  `Now ${maxFileText(settings.max_file_bytes)}. Chunked and binary files are never scanned.`}
              </p>
            </div>
          </div>
          <FormError error={save.error} />
          {canManage ? (
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="text-xs text-muted-foreground">
                {settings.updated_by
                  ? `Last changed by ${settings.updated_by} ${relativeTime(settings.updated_at)}.`
                  : "Default settings."}
              </p>
              <Button
                type="submit"
                size="sm"
                disabled={!dirty || save.isPending || Boolean(maxError)}
              >
                {save.isPending ? "Saving…" : "Save settings"}
              </Button>
            </div>
          ) : (
            <p className="text-xs text-muted-foreground">
              Tree administrators can change these settings.
            </p>
          )}
        </form>
      </CardContent>
    </Card>
  );
}

// ---- dialogs ----

function ResolveDialog({
  finding,
  onClose,
}: {
  finding: SecretFinding;
  onClose: () => void;
}) {
  const api = useComplianceApi();
  const invalidate = useInvalidate();
  const toast = useToast();
  const id = useId();
  const [resolution, setResolution] = useState<Resolution>("revoked");
  const [note, setNote] = useState("");
  const resolve = useMutation({
    mutationFn: () =>
      api.resolve(finding.id, {
        resolution,
        ...(note.trim() && { note: note.trim() }),
      }),
    meta: { inlineError: true },
    onSuccess: async (f) => {
      toast.success("Finding resolved", `${f.path}:${f.line}`);
      await invalidate();
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Resolve finding</DialogTitle>
          <DialogDescription>
            <span className="font-mono break-all">
              {finding.path}:{finding.line}
            </span>{" "}
            ({finding.rule}). Resolving does not let the secret through blocked
            pushes; allowlist it for that.
          </DialogDescription>
        </DialogHeader>
        <form
          id={`${id}-form`}
          aria-label="Resolve finding"
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            resolve.mutate();
          }}
        >
          <RadioGroup
            value={resolution}
            onValueChange={(v) => setResolution(v as Resolution)}
            aria-label="Resolution"
          >
            {(Object.keys(RESOLUTION_LABELS) as Resolution[]).map((r) => (
              <div key={r} className="flex items-center gap-2">
                <RadioGroupItem id={`${id}-${r}`} value={r} />
                <Label htmlFor={`${id}-${r}`} className="font-normal">
                  {RESOLUTION_LABELS[r]}
                </Label>
              </div>
            ))}
          </RadioGroup>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-note`}>Note (optional)</Label>
            <Textarea
              id={`${id}-note`}
              value={note}
              onChange={(e) => setNote(e.target.value)}
              rows={3}
            />
          </div>
          <FormError error={resolve.error} />
        </form>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="submit"
            form={`${id}-form`}
            disabled={resolve.isPending}
          >
            {resolve.isPending ? "Saving…" : "Resolve"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function AllowDialog({
  fingerprint: initial,
  onClose,
}: {
  fingerprint: string;
  onClose: () => void;
}) {
  const api = useComplianceApi();
  const invalidate = useInvalidate();
  const toast = useToast();
  const id = useId();
  const [fingerprint, setFingerprint] = useState(initial);
  const [reason, setReason] = useState("");
  const [touched, setTouched] = useState(false);
  const errors = allowFormErrors(fingerprint, reason);
  const allow = useMutation({
    mutationFn: () =>
      api.allow(fingerprint.trim().toLowerCase(), reason.trim()),
    meta: { inlineError: true },
    onSuccess: async (e) => {
      toast.success("Secret allowlisted", e.fingerprint);
      await invalidate();
      onClose();
    },
  });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.fingerprint || errors.reason) return;
    allow.mutate();
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Allowlist a secret</DialogTitle>
          <DialogDescription>
            Pushes containing this secret are never reported or blocked on this
            tree, wherever it appears. Open findings of it are resolved as false
            positives.
          </DialogDescription>
        </DialogHeader>
        <form
          id={`${id}-form`}
          aria-label="Allowlist a secret"
          className="grid gap-4"
          onSubmit={submit}
          noValidate
        >
          <div className="grid gap-2">
            <Label htmlFor={`${id}-fp`}>Fingerprint</Label>
            <Input
              id={`${id}-fp`}
              value={fingerprint}
              onChange={(e) => setFingerprint(e.target.value)}
              className="font-mono"
              spellCheck={false}
              autoComplete="off"
              aria-invalid={touched && errors.fingerprint ? true : undefined}
              aria-describedby={`${id}-fp-hint`}
            />
            <p
              id={`${id}-fp-hint`}
              className={
                touched && errors.fingerprint
                  ? "text-xs font-semibold"
                  : "text-xs text-muted-foreground"
              }
            >
              {(touched && errors.fingerprint) ||
                "Shown with each finding and by wt compliance scan."}
            </p>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-reason`}>Reason</Label>
            <Textarea
              id={`${id}-reason`}
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              rows={3}
              aria-invalid={touched && errors.reason ? true : undefined}
              aria-describedby={`${id}-reason-hint`}
            />
            <p
              id={`${id}-reason-hint`}
              className={
                touched && errors.reason
                  ? "text-xs font-semibold"
                  : "text-xs text-muted-foreground"
              }
            >
              {(touched && errors.reason) ||
                "Why this value may be pushed (e.g. a test fixture). Recorded in the audit log."}
            </p>
          </div>
          <FormError error={allow.error} />
        </form>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form={`${id}-form`} disabled={allow.isPending}>
            {allow.isPending ? "Saving…" : "Allowlist"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---- findings ----

function FindingsCard() {
  const { tenant, tree, canManage } = useTreeContext();
  const api = useComplianceApi();
  const invalidate = useInvalidate();
  const toast = useToast();
  const [status, setStatus] = useState<FindingStatus | "all">("open");
  const [resolving, setResolving] = useState<SecretFinding | null>(null);
  const [allowing, setAllowing] = useState<string | null>(null);
  const findings = useQuery({
    queryKey: complianceKeys.findings(tenant, tree, status),
    queryFn: () => api.findings(status),
    retry: retryV06,
  });
  const reopen = useMutation({
    mutationFn: (f: SecretFinding) => api.resolve(f.id, { reopen: true }),
    onSuccess: async (f) => {
      toast.success("Finding reopened", `${f.path}:${f.line}`);
      await invalidate();
    },
  });

  return (
    <Card className="gap-0 overflow-hidden py-0">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b p-3">
        <div>
          <h3 className="flex items-center gap-2 font-semibold">
            <FileWarning aria-hidden="true" className="size-4" />
            Findings
          </h3>
          {findings.data && (
            <p className="text-xs text-muted-foreground">
              {formatNumber(findings.data.open)} open ·{" "}
              {formatNumber(findings.data.resolved)} resolved
            </p>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            value={status}
            onValueChange={(v) => v && setStatus(v as FindingStatus | "all")}
            aria-label="Finding status"
          >
            <ToggleGroupItem value="open">Open</ToggleGroupItem>
            <ToggleGroupItem value="resolved">Resolved</ToggleGroupItem>
            <ToggleGroupItem value="all">All</ToggleGroupItem>
          </ToggleGroup>
          {canManage && (
            <Button size="sm" variant="outline" onClick={() => setAllowing("")}>
              Allowlist a fingerprint
            </Button>
          )}
        </div>
      </div>
      {findings.isPending ? (
        <TableSkeleton label="Loading findings" rows={3} columns={5} />
      ) : findings.isError ? (
        <QueryError
          error={findings.error}
          onRetry={() => void findings.refetch()}
          title="Could not load findings"
        />
      ) : findings.data.findings.length === 0 ? (
        <EmptyState
          icon={CheckCircle2}
          title={status === "resolved" ? "No resolved findings" : "No findings"}
        >
          {status === "open"
            ? "No possible secrets were pushed to this tree."
            : "Nothing to show."}
        </EmptyState>
      ) : (
        <Table aria-label="Findings">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Location</TableHead>
              <TableHead className="hidden lg:table-cell">Excerpt</TableHead>
              <TableHead>Push</TableHead>
              <TableHead className="hidden md:table-cell">
                Fingerprint
              </TableHead>
              <TableHead className="hidden sm:table-cell">Seen</TableHead>
              <TableHead>
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {findings.data.findings.map((f) => (
              <TableRow
                key={f.id}
                className={
                  f.status === "resolved" ? "text-muted-foreground" : undefined
                }
              >
                <TableCell className="max-w-72">
                  <p
                    className="truncate font-mono text-sm font-medium"
                    title={`${f.path}:${f.line}`}
                  >
                    {f.path}:{f.line}
                  </p>
                  <p className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                    <Badge variant="outline" className="font-mono">
                      {f.rule}
                    </Badge>
                    {f.status === "resolved" && (
                      <span>
                        resolved:{" "}
                        {RESOLUTION_LABELS[f.resolution as Resolution] ??
                          f.resolution}
                      </span>
                    )}
                  </p>
                </TableCell>
                <TableCell className="hidden max-w-96 lg:table-cell">
                  <code
                    className="line-clamp-2 font-mono text-xs break-all"
                    title={f.excerpt}
                  >
                    {f.excerpt}
                  </code>
                </TableCell>
                <TableCell>
                  <ActionBadge f={f} />
                </TableCell>
                <TableCell className="hidden max-w-40 md:table-cell">
                  <Fingerprint value={f.fingerprint} />
                </TableCell>
                <TableCell className="hidden text-xs sm:table-cell">
                  <time
                    dateTime={f.last_seen_at}
                    title={absoluteTime(f.last_seen_at)}
                  >
                    {relativeTime(f.last_seen_at)}
                  </time>
                  <p className="text-muted-foreground">
                    {f.count > 1 ? `${f.count} pushes · ` : ""}
                    {f.pushed_by || "unknown"} on {f.branch || "?"} ·{" "}
                    {shortId(f.snapshot_id)}
                  </p>
                </TableCell>
                <TableCell className="w-px text-right whitespace-nowrap">
                  {canManage &&
                    (f.status === "open" ? (
                      <span className="inline-flex gap-1">
                        <Button
                          variant="outline"
                          size="sm"
                          aria-label={`Resolve ${f.path}:${f.line}`}
                          onClick={() => setResolving(f)}
                        >
                          Resolve
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          aria-label={`Allowlist ${f.path}:${f.line}`}
                          onClick={() => setAllowing(f.fingerprint)}
                        >
                          Allowlist
                        </Button>
                      </span>
                    ) : (
                      <Button
                        variant="ghost"
                        size="sm"
                        aria-label={`Reopen ${f.path}:${f.line}`}
                        disabled={reopen.isPending}
                        onClick={() => reopen.mutate(f)}
                      >
                        <RotateCcw aria-hidden="true" />
                        Reopen
                      </Button>
                    ))}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      {resolving && (
        <ResolveDialog finding={resolving} onClose={() => setResolving(null)} />
      )}
      {allowing !== null && (
        <AllowDialog fingerprint={allowing} onClose={() => setAllowing(null)} />
      )}
    </Card>
  );
}

// ---- allowlist ----

function AllowlistCard() {
  const { tenant, tree, canManage } = useTreeContext();
  const api = useComplianceApi();
  const invalidate = useInvalidate();
  const toast = useToast();
  const [removing, setRemoving] = useState<AllowEntry | null>(null);
  const allowlist = useQuery({
    queryKey: complianceKeys.allowlist(tenant, tree),
    queryFn: api.allowlist,
    retry: retryV06,
  });
  const remove = useMutation({
    mutationFn: (e: AllowEntry) => api.disallow(e.fingerprint),
    onSuccess: async (_r, e) => {
      toast.success("Removed from the allowlist", e.fingerprint);
      await invalidate();
    },
  });
  return (
    <Card className="gap-0 overflow-hidden py-0">
      <div className="border-b p-3">
        <h3 className="flex items-center gap-2 font-semibold">
          <ShieldCheck aria-hidden="true" className="size-4" />
          Allowlist
        </h3>
        <p className="text-xs text-muted-foreground">
          Known false positives, by fingerprint.
        </p>
      </div>
      {allowlist.isPending ? (
        <TableSkeleton label="Loading the allowlist" rows={2} columns={3} />
      ) : allowlist.isError ? (
        <QueryError
          error={allowlist.error}
          onRetry={() => void allowlist.refetch()}
          title="Could not load the allowlist"
        />
      ) : allowlist.data.entries.length === 0 ? (
        <p className="p-4 text-sm text-muted-foreground">
          The allowlist is empty.
        </p>
      ) : (
        <Table aria-label="Allowlist">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Fingerprint</TableHead>
              <TableHead>Reason</TableHead>
              <TableHead className="hidden sm:table-cell">Added</TableHead>
              <TableHead>
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {allowlist.data.entries.map((e) => (
              <TableRow key={e.fingerprint}>
                <TableCell className="max-w-48">
                  <Fingerprint value={e.fingerprint} />
                </TableCell>
                <TableCell className="text-sm">{e.reason}</TableCell>
                <TableCell className="hidden text-xs text-muted-foreground sm:table-cell">
                  {e.created_by} · {relativeTime(e.created_at)}
                </TableCell>
                <TableCell className="w-px text-right">
                  {canManage && (
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label={`Remove ${e.fingerprint}`}
                      onClick={() => setRemoving(e)}
                    >
                      <Trash2 aria-hidden="true" />
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      <ConfirmAction
        open={removing !== null}
        onOpenChange={(o) => !o && setRemoving(null)}
        title="Remove from the allowlist?"
        description={
          <>
            Pushes containing{" "}
            <span className="font-mono break-all">{removing?.fingerprint}</span>{" "}
            are reported (and blocked in block mode) again.
          </>
        }
        confirmLabel="Remove"
        onConfirm={() =>
          removing ? remove.mutateAsync(removing) : Promise.resolve()
        }
      />
    </Card>
  );
}

// ---- license audit ----

function Stat({
  label,
  value,
  emphasis,
}: {
  label: string;
  value: number;
  emphasis?: boolean;
}) {
  return (
    <div className="rounded-md border p-3">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd
        className={
          emphasis && value > 0 ? "text-xl font-bold" : "text-xl font-semibold"
        }
      >
        {formatNumber(value)}
      </dd>
    </div>
  );
}

function AuditBody({ audit }: { audit: LicenseAudit }) {
  return (
    <div className="grid gap-5">
      <dl
        aria-label="License audit summary"
        className="grid grid-cols-2 gap-2 sm:grid-cols-4"
      >
        <Stat label="Files" value={audit.files} />
        <Stat
          label="Without a license rule"
          value={audit.unlicensed_count}
          emphasis
        />
        <Stat
          label="Exposed proprietary paths"
          value={audit.exposed_count}
          emphasis
        />
        <Stat
          label="Rules failing the SPDX check"
          value={audit.invalid_rules}
          emphasis
        />
      </dl>
      <section aria-label="Files per license" className="grid gap-2">
        <h4 className="text-sm font-semibold">Files per license</h4>
        {audit.licenses.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No file is covered by a license rule.
          </p>
        ) : (
          <ul className="flex flex-wrap gap-2">
            {audit.licenses.map((u) => (
              <li key={u.spdx}>
                <Badge
                  variant={u.proprietary ? "default" : "outline"}
                  className="font-mono"
                  title={u.globs.join(", ")}
                >
                  {u.spdx} · {formatNumber(u.files)}
                </Badge>
              </li>
            ))}
          </ul>
        )}
      </section>
      {audit.exposed_count > 0 && (
        <section aria-label="Exposed proprietary paths" className="grid gap-2">
          <h4 className="text-sm font-semibold">
            Proprietary paths readable by other tenants
          </h4>
          <Table aria-label="Exposed paths">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>Path</TableHead>
                <TableHead>Readable by</TableHead>
                <TableHead className="hidden sm:table-cell">Rule</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {audit.exposed.map((e) => (
                <TableRow key={`${e.path}\u0000${e.grantee}`}>
                  <TableCell className="font-mono text-xs break-all">
                    {e.path}
                  </TableCell>
                  <TableCell className="text-sm">
                    {e.grantee || "every other tenant"}{" "}
                    <span className="text-muted-foreground">({e.grant})</span>
                  </TableCell>
                  <TableCell className="hidden font-mono text-xs sm:table-cell">
                    {e.glob}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </section>
      )}
      {audit.unlicensed_count > 0 && (
        <section
          aria-label="Paths without a license rule"
          className="grid gap-2"
        >
          <h4 className="text-sm font-semibold">
            Paths without a license rule
          </h4>
          <ul className="grid gap-0.5 font-mono text-xs">
            {audit.unlicensed.slice(0, 50).map((p) => (
              <li key={p} className="break-all">
                {p}
              </li>
            ))}
          </ul>
          {audit.unlicensed_count > 50 && (
            <p className="text-xs text-muted-foreground">
              and {formatNumber(audit.unlicensed_count - 50)} more
            </p>
          )}
        </section>
      )}
      {audit.rules.length > 0 && (
        <section aria-label="License rule checks" className="grid gap-2">
          <h4 className="text-sm font-semibold">
            Rules (SPDX License List {audit.spdx_list_version})
          </h4>
          <Table aria-label="Rule checks">
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>Glob</TableHead>
                <TableHead>License</TableHead>
                <TableHead className="hidden sm:table-cell">Files</TableHead>
                <TableHead>Check</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {audit.rules.map((r) => (
                <TableRow
                  key={`${r.path_glob}\u0000${r.grantee}`}
                  className={r.expired ? "text-muted-foreground" : undefined}
                >
                  <TableCell className="font-mono text-xs">
                    {r.path_glob}
                    {r.grantee && (
                      <span className="text-muted-foreground">
                        {" "}
                        → {r.grantee}
                      </span>
                    )}
                  </TableCell>
                  <TableCell className="font-mono text-xs">{r.spdx}</TableCell>
                  <TableCell className="hidden text-xs sm:table-cell">
                    {formatNumber(r.files)}
                  </TableCell>
                  <TableCell className="text-xs">
                    {r.valid ? (
                      <span>{r.expired ? "valid (expired)" : "valid"}</span>
                    ) : (
                      <ul className="font-semibold">
                        {r.problems.map((p) => (
                          <li key={p}>{p}</li>
                        ))}
                      </ul>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </section>
      )}
    </div>
  );
}

function LicenseAuditCard() {
  const { tenant, tree, canManage } = useTreeContext();
  const api = useComplianceApi();
  const audit = useQuery({
    queryKey: complianceKeys.licenses(tenant, tree),
    queryFn: () => api.licenses(),
    retry: retryV06,
    enabled: canManage,
  });
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Scale aria-hidden="true" className="size-4" />
          License audit
        </CardTitle>
        <CardDescription>
          {audit.data ? (
            <>
              Default branch <span className="font-mono">{audit.data.ref}</span>{" "}
              at{" "}
              <span className="font-mono">
                {shortId(audit.data.snapshot_id)}
              </span>
              .
            </>
          ) : (
            "Licenses of the default branch, uncovered paths, proprietary paths reachable through grants and a strict SPDX check of every rule."
          )}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {!canManage ? (
          <p className="text-sm text-muted-foreground">
            Tree administrators can run the license audit.
          </p>
        ) : audit.isPending ? (
          <TableSkeleton
            label="Loading the license audit"
            rows={3}
            columns={3}
          />
        ) : audit.isError ? (
          <QueryError
            error={audit.error}
            onRetry={() => void audit.refetch()}
            title="Could not run the license audit"
          />
        ) : (
          <AuditBody audit={audit.data} />
        )}
      </CardContent>
    </Card>
  );
}

// ---- tab ----

/** Secret-scan findings and license audit of this tree (§16.7). */
export function ComplianceTab() {
  const { tenant, tree } = useTreeContext();
  const api = useComplianceApi();
  const settings = useQuery({
    queryKey: complianceKeys.settings(tenant, tree),
    queryFn: api.settings,
    retry: retryV06,
  });
  const unavailable = settings.isError && isUnavailable(settings.error);

  return (
    <div className="grid gap-4">
      <div>
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <ShieldAlert aria-hidden="true" className="size-5" />
          Compliance
        </h2>
        <p className="text-sm text-muted-foreground">
          Server-side secret scanning of pushes and the license audit of this
          tree.
        </p>
      </div>
      {settings.isPending ? (
        <TableSkeleton
          label="Loading compliance settings"
          rows={2}
          columns={3}
        />
      ) : unavailable ? (
        <UnavailableNotice feature="Secret scanning and license compliance" />
      ) : settings.isError ? (
        <QueryError
          error={settings.error}
          onRetry={() => void settings.refetch()}
          title="Could not load the scanning settings"
        />
      ) : (
        <>
          <SettingsCard
            key={`${settings.data.updated_at ?? ""}|${settings.data.mode}|${settings.data.exclude.join(",")}|${settings.data.max_file_bytes}`}
            settings={settings.data}
          />
          <FindingsCard />
          <AllowlistCard />
          <LicenseAuditCard />
        </>
      )}
    </div>
  );
}
