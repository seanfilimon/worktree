/**
 * Config-as-code (docs/ENGINEERING_PLAN.md §16.3): the configuration the
 * server applied from this tree's `.wt-tree/` files — source snapshot,
 * files, declared branches and protections, access document, changes and
 * warnings — the located errors of the last rejected push, and a form
 * that validates files without pushing them. Endpoints in ./treeConfigApi.
 */
import { useMutation, useQuery } from "@tanstack/react-query";
import { CircleAlert, FileCog, FileText, GitBranch, Lock, ShieldCheck, TriangleAlert } from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { Link } from "react-router";
import { useApi } from "@/api/context";
import { isUnavailable, retryV06 } from "@/api/v06";
import { FormError } from "@/components/people/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { formatBytes } from "@/lib/format";
import { filesPath, treePath } from "@/lib/paths";
import { useTreeContext } from "./context";
import { EmptyState, LoadingRows, QueryError, RelativeTime } from "./history/shared";
import {
  CONFIG_FILES,
  createTreeConfigApi,
  diagnosticLocation,
  protectionSummary,
  treeConfigKey,
  type ConfigDiagnostic,
  type ConfigValidation,
  type TreeConfigApplied,
  type TreeConfigAttempt,
} from "./treeConfigApi";

function useTreeConfigApi() {
  const { client } = useApi();
  const { tenant, tree } = useTreeContext();
  return useMemo(() => createTreeConfigApi(client, tenant, tree), [client, tenant, tree]);
}

// ---- pieces ----

/** Diagnostics as a table: location, code (errors filled, warnings outlined), message. */
export function DiagnosticsTable({ items, label }: { items: ConfigDiagnostic[]; label: string }) {
  return (
    <Table aria-label={label}>
      <TableHeader>
        <TableRow>
          <TableHead>Location</TableHead>
          <TableHead>Code</TableHead>
          <TableHead>Message</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {items.map((d, i) => (
          <TableRow key={`${diagnosticLocation(d)}-${d.code}-${i}`}>
            <TableCell className="font-mono text-xs whitespace-nowrap">{diagnosticLocation(d)}</TableCell>
            <TableCell>
              <Badge variant={d.severity === "error" ? "default" : "outline"} title={d.severity}>
                {d.code}
              </Badge>
            </TableCell>
            <TableCell className="whitespace-normal">{d.message}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}

function RejectedAttempt({ attempt }: { attempt: TreeConfigAttempt }) {
  const { tenant, tree } = useTreeContext();
  return (
    <Alert>
      <TriangleAlert aria-hidden="true" />
      <AlertTitle>The last configuration push was rejected</AlertTitle>
      <AlertDescription className="grid w-full gap-3">
        <p>
          Snapshot{" "}
          <Link className="font-mono underline underline-offset-4" to={treePath(tenant, tree, `snapshots/${encodeURIComponent(attempt.snapshot_id)}`)}>
            {attempt.snapshot_id.slice(0, 12)}
          </Link>{" "}
          by {attempt.by || "unknown"}, <RelativeTime value={attempt.at} />. Fix the files below and push again.
        </p>
        <div className="rounded-md border bg-background text-foreground">
          <DiagnosticsTable items={attempt.errors} label="Errors of the last attempt" />
        </div>
      </AlertDescription>
    </Alert>
  );
}

function AppliedCard({ applied }: { applied: TreeConfigApplied }) {
  const { tenant, tree } = useTreeContext();
  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ShieldCheck aria-hidden="true" className="size-4" />
          Applied configuration
        </CardTitle>
        <CardDescription>
          From snapshot{" "}
          <Link className="font-mono text-foreground underline underline-offset-4" to={treePath(tenant, tree, `snapshots/${encodeURIComponent(applied.snapshot_id)}`)}>
            {applied.snapshot_id.slice(0, 12)}
          </Link>{" "}
          on <span className="font-mono">{applied.branch}</span>, applied by {applied.applied_by || "unknown"} <RelativeTime value={applied.applied_at} />.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6">
        <section aria-labelledby="config-files" className="grid gap-2">
          <h3 id="config-files" className="text-sm font-medium">
            Files
          </h3>
          {applied.files.length === 0 ? (
            <p className="text-sm text-muted-foreground">No config-as-code files remain on the default branch.</p>
          ) : (
            <ul className="grid gap-1 text-sm">
              {applied.files.map((f) => (
                <li key={f.path} className="flex items-center gap-2">
                  <FileText aria-hidden="true" className="size-3.5 text-muted-foreground" />
                  <Link className="font-mono hover:underline" to={filesPath(tenant, tree, { snapshot: applied.snapshot_id, file: f.path })}>
                    {f.path}
                  </Link>
                  <span className="text-muted-foreground">{formatBytes(f.size)}</span>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section aria-labelledby="config-branches" className="grid gap-2">
          <h3 id="config-branches" className="flex items-center gap-2 text-sm font-medium">
            <GitBranch aria-hidden="true" className="size-3.5" />
            Branches <span className="font-normal text-muted-foreground">(.wt-tree/branches.toml)</span>
          </h3>
          {applied.branches.length === 0 ? (
            <p className="text-sm text-muted-foreground">No branch metadata declared.</p>
          ) : (
            <Table aria-label="Declared branches">
              <TableHeader>
                <TableRow>
                  <TableHead>Branch</TableHead>
                  <TableHead>Owner</TableHead>
                  <TableHead>Description</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {applied.branches.map((b) => (
                  <TableRow key={b.name}>
                    <TableCell className="font-mono">{b.name}</TableCell>
                    <TableCell className="font-mono text-xs">{b.owner || <span className="text-muted-foreground">—</span>}</TableCell>
                    <TableCell className="whitespace-normal">{b.description || <span className="text-muted-foreground">—</span>}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </section>

        <section aria-labelledby="config-protections" className="grid gap-2">
          <h3 id="config-protections" className="flex items-center gap-2 text-sm font-medium">
            <Lock aria-hidden="true" className="size-3.5" />
            Protections <span className="font-normal text-muted-foreground">(.wt-tree/protections.toml)</span>
          </h3>
          {applied.protections.length === 0 ? (
            <p className="text-sm text-muted-foreground">No protections declared.</p>
          ) : (
            <Table aria-label="Declared protections">
              <TableHeader>
                <TableRow>
                  <TableHead>Pattern</TableHead>
                  <TableHead>Rules</TableHead>
                  <TableHead>Managed by</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {applied.protections.map((p) => {
                  const managed = applied.managed_patterns.includes(p.pattern);
                  return (
                    <TableRow key={p.pattern}>
                      <TableCell className="font-mono">{p.pattern}</TableCell>
                      <TableCell className="whitespace-normal">{protectionSummary(p)}</TableCell>
                      <TableCell>
                        {managed ? (
                          <Badge variant="secondary">config</Badge>
                        ) : (
                          <Badge variant="outline" title="A rule with this pattern was set through the API; the entry is ignored.">
                            API (ignored)
                          </Badge>
                        )}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          )}
        </section>

        <section aria-labelledby="config-access" className="grid gap-1">
          <h3 id="config-access" className="text-sm font-medium">
            Access policies <span className="font-normal text-muted-foreground">(.wt-tree/access/)</span>
          </h3>
          <p className="text-sm text-muted-foreground">
            {applied.access_hash ? (
              <>
                Tree access document applied (<span className="font-mono">{applied.access_hash.slice(0, 12)}</span>). Its deny policies are listed under{" "}
                <span className="font-medium text-foreground">IAM → Policies</span> as <span className="font-mono">{tree}/…</span>.
              </>
            ) : (
              "No tree access document applied."
            )}
          </p>
        </section>

        {applied.changes.length > 0 && (
          <section aria-labelledby="config-changes" className="grid gap-1">
            <h3 id="config-changes" className="text-sm font-medium">
              Changes of the last apply
            </h3>
            <ul className="list-inside list-disc text-sm">
              {applied.changes.map((c) => (
                <li key={c} className="font-mono text-xs">
                  {c}
                </li>
              ))}
            </ul>
          </section>
        )}

        {applied.warnings.length > 0 && (
          <section aria-labelledby="config-warnings" className="grid gap-2">
            <h3 id="config-warnings" className="text-sm font-medium">
              Warnings
            </h3>
            <DiagnosticsTable items={applied.warnings} label="Warnings" />
          </section>
        )}
      </CardContent>
    </Card>
  );
}

function ValidateCard() {
  const api = useTreeConfigApi();
  const contentId = useId();
  const [path, setPath] = useState<string>(CONFIG_FILES[0].path);
  const [texts, setTexts] = useState<Record<string, string>>({});
  const [result, setResult] = useState<ConfigValidation | null>(null);
  const validate = useMutation({
    mutationFn: (files: Record<string, string>) => api.validate(files),
    meta: { inlineError: true },
    onSuccess: setResult,
  });
  const files = Object.fromEntries(Object.entries(texts).filter(([, v]) => v.trim() !== ""));
  const count = Object.keys(files).length;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (count === 0) return;
    validate.mutate(files);
  };
  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle>Validate before pushing</CardTitle>
        <CardDescription>
          Paste one or more files; the server checks them as if they were pushed to the default branch. Nothing is changed. Locally,{" "}
          <code className="font-mono">wt trees config validate</code> does the same.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <form onSubmit={submit} aria-label="Validate configuration" className="grid gap-3" noValidate>
          <ToggleGroup
            type="single"
            variant="outline"
            value={path}
            onValueChange={(v) => v && setPath(v)}
            aria-label="File"
            className="flex-wrap"
          >
            {CONFIG_FILES.map((f) => (
              <ToggleGroupItem key={f.path} value={f.path} className="font-mono text-xs">
                {f.label}
                {texts[f.path]?.trim() ? " •" : ""}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <div className="grid gap-2">
            <Label htmlFor={contentId} className="font-mono">
              {path}
            </Label>
            <Textarea
              id={contentId}
              rows={10}
              spellCheck={false}
              className="font-mono text-xs"
              placeholder={'[[branch]]\nname = "main"\nowner = "team:platform"'}
              value={texts[path] ?? ""}
              onChange={(e) => setTexts((t) => ({ ...t, [path]: e.target.value }))}
            />
          </div>
          <FormError error={validate.error} />
          <div className="flex items-center gap-3">
            <Button type="submit" disabled={validate.isPending || count === 0}>
              {validate.isPending ? "Validating…" : count === 0 ? "Validate" : `Validate ${count} file${count === 1 ? "" : "s"}`}
            </Button>
            {result && (
              <span role="status" className="flex items-center gap-1.5 text-sm font-medium">
                {result.valid ? <ShieldCheck aria-hidden="true" className="size-4" /> : <CircleAlert aria-hidden="true" className="size-4" />}
                {result.valid ? "Valid" : `${result.diagnostics.filter((d) => d.severity === "error").length} error(s)`}
              </span>
            )}
          </div>
          {result && result.diagnostics.length > 0 && <DiagnosticsTable items={result.diagnostics} label="Validation results" />}
          {result && result.access_changes.length > 0 && (
            <ul aria-label="Access plan" className="list-inside list-disc text-sm">
              {result.access_changes.map((c) => (
                <li key={c} className="font-mono text-xs">
                  {c}
                </li>
              ))}
            </ul>
          )}
        </form>
      </CardContent>
    </Card>
  );
}

// ---- page ----

/** The applied .wt-tree configuration of this tree (§16.3). */
export function TreeConfigTab() {
  const { tenant, tree, summary } = useTreeContext();
  const api = useTreeConfigApi();
  const config = useQuery({ queryKey: treeConfigKey(tenant, tree), queryFn: api.get, retry: retryV06 });
  const unavailable = config.isError && isUnavailable(config.error);

  return (
    <div className="grid gap-4">
      <div>
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <FileCog aria-hidden="true" className="size-5" />
          Configuration as code
        </h2>
        <p className="text-sm text-muted-foreground">
          <span className="font-mono">.wt-tree/branches.toml</span>, <span className="font-mono">protections.toml</span>,{" "}
          <span className="font-mono">config.toml</span> and <span className="font-mono">access/</span> are validated when pushed to{" "}
          <span className="font-mono">{summary.tree.default_branch}</span> and applied after the push.
        </p>
      </div>
      {unavailable ? (
        <UnavailableNotice feature="Configuration as code" />
      ) : config.isPending ? (
        <LoadingRows label="Loading configuration" />
      ) : config.isError ? (
        <QueryError error={config.error} onRetry={() => void config.refetch()} />
      ) : (
        <>
          {config.data.last_attempt && !config.data.last_attempt.accepted && <RejectedAttempt attempt={config.data.last_attempt} />}
          {config.data.applied ? (
            <AppliedCard applied={config.data.applied} />
          ) : (
            <Card>
              <CardContent>
                <EmptyState title="No configuration applied yet" icon={<FileCog aria-hidden="true" />}>
                  Add <span className="font-mono">.wt-tree/branches.toml</span> or <span className="font-mono">.wt-tree/protections.toml</span> to the tree and push it to{" "}
                  <span className="font-mono">{summary.tree.default_branch}</span>.
                </EmptyState>
              </CardContent>
            </Card>
          )}
        </>
      )}
      {!unavailable && <ValidateCard />}
    </div>
  );
}
