/** Path owners of a tree (§15.7): server configuration or the OWNERS file, with a rules editor. */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArrowDown, ArrowUp, FileText, Info, LoaderCircle, Plus, Save, Server, Trash2, TriangleAlert, UserCheck } from "lucide-react";
import { useId, useState } from "react";
import { isUnavailable } from "@/api/v05tree";
import { splitPrincipals, useTreeOwners, useV07ReviewApi, v07Keys, type OwnersRule, type OwnersView } from "@/api/v07review";
import { EmptyState, ErrorState, LoadingRows, RelativeTime } from "@/components/files/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";
import { FeatureUnavailable } from "./v05/shared";

interface Row {
  key: number;
  pattern: string;
  /** Owners as typed: tokens separated by spaces or commas. */
  owners: string;
}

let nextKey = 1;
const toRows = (rules: readonly OwnersRule[]): Row[] => rules.map((r) => ({ key: nextKey++, pattern: r.pattern, owners: r.owners.join(" ") }));

function patternError(p: string): string | undefined {
  const s = p.trim();
  if (!s) return "Pattern required.";
  if (/\s/.test(s)) return "No spaces.";
  return undefined;
}

const SOURCE: Record<OwnersView["source"], { label: string; icon: typeof Server; variant: "default" | "outline" }> = {
  config: { label: "Server config", icon: Server, variant: "default" },
  file: { label: "OWNERS file", icon: FileText, variant: "outline" },
  none: { label: "None", icon: Info, variant: "outline" },
};

function Explanation() {
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="flex items-center gap-2 text-sm">
          <Info aria-hidden="true" className="size-4" />
          How owners work
        </CardTitle>
      </CardHeader>
      <CardContent className="grid gap-2 px-4 text-sm text-muted-foreground">
        <p>
          Each rule maps a path pattern to owners. Patterns are anchored at the tree root:{" "}
          <code className="font-mono text-foreground">docs/**</code> covers everything below docs,{" "}
          <code className="font-mono text-foreground">**/*.go</code> Go files at any depth,{" "}
          <code className="font-mono text-foreground">*.go</code> only top-level ones.
        </p>
        <p>
          <span className="font-semibold text-foreground">The last matching rule wins.</span> A rule without owners un-owns the paths it
          matches.
        </p>
        <p>
          Owners are users (<code className="font-mono text-foreground">alice</code>, <code className="font-mono text-foreground">@alice</code>,{" "}
          <code className="font-mono text-foreground">user:alice</code>) or teams of this tenant (
          <code className="font-mono text-foreground">@tenant/core</code>, <code className="font-mono text-foreground">team:core</code>).
          Opening a merge request requests its owners as reviewers; a protection can require their approval.
        </p>
        <p>
          Without a server configuration, an <code className="font-mono text-foreground">OWNERS</code> file at the root of the default branch
          applies (one <code className="font-mono text-foreground">pattern @owner…</code> rule per line, # comments).
        </p>
      </CardContent>
    </Card>
  );
}

function RulesEditor({ view, canManage }: { view: OwnersView; canManage: boolean }) {
  const api = useV07ReviewApi();
  const { tenant, tree } = useTreeContext();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [rows, setRows] = useState<Row[]>(() => toRows(view.rules));
  const [touched, setTouched] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const readOnly = !canManage;

  const save = useMutation({
    mutationFn: (rules: OwnersRule[]) => api.owners.put(tenant, tree, rules),
    meta: { inlineError: true },
    onSuccess: async (v, rules) => {
      queryClient.setQueryData(v07Keys.owners(tenant, tree), v);
      toast.success(rules.length ? "Owners saved" : "Server owners configuration removed", rules.length ? `${rules.length} rules` : undefined);
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree, "mr-checks"] });
    },
    onError: (err) => toast.error(err, "Could not save owners"),
  });

  const update = (key: number, patch: Partial<Row>) => setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const move = (i: number, d: -1 | 1) =>
    setRows((rs) => {
      const j = i + d;
      if (j < 0 || j >= rs.length) return rs;
      const next = [...rs];
      [next[i], next[j]] = [next[j]!, next[i]!];
      return next;
    });
  const add = () => setRows((rs) => [...rs, { key: nextKey++, pattern: "", owners: "" }]);
  const remove = (key: number) => setRows((rs) => rs.filter((r) => r.key !== key));

  // Fully blank rows are ignored on save.
  const filled = rows.filter((r) => r.pattern.trim() || r.owners.trim());
  const errors = new Map(filled.map((r) => [r.key, patternError(r.pattern)] as const).filter(([, e]) => e));

  const submit = () => {
    setTouched(true);
    if (errors.size > 0) return;
    const rules = filled.map((r) => ({ pattern: r.pattern.trim(), owners: splitPrincipals(r.owners) }));
    if (rules.length === 0 && view.source !== "config") return;
    save.mutate(rules);
  };

  return (
    <div className="flex flex-col">
      {rows.length === 0 ? (
        <EmptyState icon={UserCheck} title="No owner rules">
          {canManage ? "Add a rule to map paths to their owners." : "No rules are configured."}
        </EmptyState>
      ) : (
        <Table aria-label="Owner rules">
          <TableHeader className="sticky top-0 z-10 bg-card">
            <TableRow>
              <TableHead className="w-10 pl-4">#</TableHead>
              <TableHead className="w-[38%]">Pattern</TableHead>
              <TableHead>Owners</TableHead>
              {canManage && (
                <TableHead className="pr-4">
                  <span className="sr-only">Actions</span>
                </TableHead>
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((r, i) => {
              const err = touched ? errors.get(r.key) : undefined;
              const n = i + 1;
              return (
                <TableRow key={r.key} data-rule={n}>
                  <TableCell className="pl-4 align-top font-mono text-muted-foreground tabular-nums">{n}</TableCell>
                  <TableCell className="align-top">
                    <Input
                      aria-label={`Pattern of rule ${n}`}
                      value={r.pattern}
                      readOnly={readOnly}
                      onChange={(e) => update(r.key, { pattern: e.target.value })}
                      placeholder="docs/**"
                      autoComplete="off"
                      spellCheck={false}
                      className="h-8 min-w-32 font-mono text-xs"
                      aria-invalid={err ? true : undefined}
                      aria-describedby={err ? `${id}-err-${r.key}` : undefined}
                    />
                    {err && (
                      <p id={`${id}-err-${r.key}`} className="mt-1 text-xs font-semibold">
                        {err}
                      </p>
                    )}
                  </TableCell>
                  <TableCell className="align-top">
                    <Input
                      aria-label={`Owners of rule ${n}`}
                      value={r.owners}
                      readOnly={readOnly}
                      onChange={(e) => update(r.key, { owners: e.target.value })}
                      placeholder={`@alice @${tenant}/core`}
                      autoComplete="off"
                      spellCheck={false}
                      className={cn("h-8 min-w-40 font-mono text-xs", !r.owners.trim() && r.pattern.trim() && "border-dashed")}
                    />
                    {!r.owners.trim() && r.pattern.trim() && <p className="mt-1 text-xs text-muted-foreground">No owners: un-owns matching paths.</p>}
                  </TableCell>
                  {canManage && (
                    <TableCell className="pr-4 text-right align-top whitespace-nowrap">
                      <Button variant="ghost" size="icon-sm" aria-label={`Move rule ${n} up`} disabled={i === 0} onClick={() => move(i, -1)}>
                        <ArrowUp aria-hidden="true" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Move rule ${n} down`}
                        disabled={i === rows.length - 1}
                        onClick={() => move(i, 1)}
                      >
                        <ArrowDown aria-hidden="true" />
                      </Button>
                      <Button variant="ghost" size="icon-sm" aria-label={`Delete rule ${n}`} onClick={() => remove(r.key)}>
                        <Trash2 aria-hidden="true" />
                      </Button>
                    </TableCell>
                  )}
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
      {canManage && (
        <div className="flex flex-wrap items-center gap-2 border-t px-4 py-3">
          <Button variant="outline" size="sm" onClick={add}>
            <Plus aria-hidden="true" />
            Add rule
          </Button>
          <Button size="sm" onClick={submit} disabled={save.isPending || (filled.length === 0 && view.source !== "config")}>
            {save.isPending && save.variables?.length !== 0 ? <LoaderCircle aria-hidden="true" className="animate-spin" /> : <Save aria-hidden="true" />}
            Save
          </Button>
          <span className="text-xs text-muted-foreground">The last matching rule wins.</span>
          {view.source === "config" && (
            <AlertDialog open={confirmRemove} onOpenChange={setConfirmRemove}>
              <AlertDialogTrigger asChild>
                <Button variant="ghost" size="sm" className="ml-auto" disabled={save.isPending}>
                  <Trash2 aria-hidden="true" />
                  Remove server config
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Remove the server owners configuration?</AlertDialogTitle>
                  <AlertDialogDescription>
                    The rules stored on the server are deleted. An OWNERS file at the root of the default branch applies again, if there is one.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction onClick={() => save.mutate([])}>Remove configuration</AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          )}
        </div>
      )}
      {canManage && view.source === "file" && (
        <p className="border-t px-4 py-2 text-xs text-muted-foreground">
          These rules come from the OWNERS file. Saving stores them as a server configuration, which then takes precedence over the file.
        </p>
      )}
    </div>
  );
}

export function OwnersTab() {
  const { tenant, tree, canManage } = useTreeContext();
  const owners = useTreeOwners(tenant, tree);
  const v = owners.data;
  const src = v ? SOURCE[v.source] : null;
  const SrcIcon = src?.icon;

  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem] lg:items-start">
      <section aria-labelledby="owners-title" className="overflow-hidden rounded-xl border bg-card">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
          <div>
            <h2 id="owners-title" className="font-semibold">
              Path owners
            </h2>
            <p className="text-sm text-muted-foreground">Who reviews which paths. Owners are requested on new merge requests.</p>
          </div>
          {v && src && SrcIcon && (
            <div className="flex flex-col items-end gap-1 text-xs text-muted-foreground">
              <Badge variant={src.variant} data-source={v.source}>
                <SrcIcon aria-hidden="true" />
                <span className="sr-only">Source: </span>
                {src.label}
              </Badge>
              {v.source === "config" && v.updated_at && (
                <span>
                  updated {v.updated_by ? `by ${v.updated_by} ` : ""}
                  <RelativeTime value={v.updated_at} />
                </span>
              )}
            </div>
          )}
        </div>
        {owners.isPending ? (
          <LoadingRows rows={3} label="Loading owners" />
        ) : owners.isError ? (
          isUnavailable(owners.error) ? (
            <FeatureUnavailable feature="Path owners" />
          ) : (
            <ErrorState error={owners.error} onRetry={() => void owners.refetch()} />
          )
        ) : (
          <>
            {owners.data.errors.length > 0 && (
              <div className="border-b p-4">
                <Alert>
                  <TriangleAlert aria-hidden="true" />
                  <AlertTitle>OWNERS file problems</AlertTitle>
                  <AlertDescription>
                    <p>These lines could not be parsed and are ignored:</p>
                    <ul aria-label="OWNERS file errors" className="mt-1 flex flex-col gap-0.5">
                      {owners.data.errors.map((e) => (
                        <li key={e} className="font-mono text-xs text-foreground">
                          {e}
                        </li>
                      ))}
                    </ul>
                  </AlertDescription>
                </Alert>
              </div>
            )}
            {/* Remount when the stored configuration changes so the editor starts from it. */}
            <RulesEditor
              key={`${owners.data.source}:${owners.data.updated_at ?? ""}`}
              view={owners.data}
              canManage={canManage}
            />
          </>
        )}
      </section>
      <Explanation />
    </div>
  );
}
