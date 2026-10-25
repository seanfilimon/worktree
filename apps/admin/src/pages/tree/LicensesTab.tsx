import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Check, ChevronsUpDown, Info, Pencil, Plus, Scale } from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { keys, useBranches, useLicenses, useSnapshot } from "@/api/queries";
import { LICENSE_GRANTS, type LicenseGrant, type LicenseRule } from "@/api/types";
import { ConfirmDelete } from "@/components/files/ConfirmDelete";
import { COMMON_SPDX, licenseGlobError, licenseGlobMatch, spdxError } from "@/components/files/license";
import { MatchPreview } from "@/components/files/MatchPreview";
import { EmptyState, ErrorState, LoadingRows } from "@/components/files/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";

const GRANTS: Record<LicenseGrant, { title: string; description: string }> = {
  "read-only": { title: "Read only", description: "Other tenants may view and clone these paths, but not push changes or export them." },
  modify: { title: "Modify", description: "Other tenants may also push and stage changes, but not export, fork or mirror." },
  redistribute: { title: "Redistribute", description: "Other tenants may read, modify and export — copy, fork, archive or mirror." },
};

/** Files of the default branch tip, used to preview what a glob covers. */
function useTipFiles() {
  const { tenant, tree, summary } = useTreeContext();
  const branches = useBranches(tenant, tree);
  const tip = branches.data?.find((b) => b.name === summary.tree.default_branch)?.tip ?? "";
  const detail = useSnapshot(tenant, tree, tip);
  const paths = useMemo(() => (detail.data?.files ?? []).map((f) => f.path), [detail.data]);
  return { paths, pending: branches.isPending || (tip !== "" && detail.isPending) };
}

function SpdxCombobox({
  id,
  value,
  onChange,
  invalid,
  describedBy,
}: {
  id: string;
  value: string;
  onChange: (v: string) => void;
  invalid: boolean;
  describedBy: string;
}) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const custom = search.trim();
  const choose = (v: string) => {
    onChange(v);
    setSearch("");
    setOpen(false);
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          id={id}
          type="button"
          variant="outline"
          role="combobox"
          aria-expanded={open}
          aria-invalid={invalid || undefined}
          aria-describedby={describedBy}
          className={cn("w-full justify-between font-mono font-normal", !value && "text-muted-foreground")}
        >
          {value || "Choose a license…"}
          <ChevronsUpDown aria-hidden="true" className="opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-(--radix-popover-trigger-width) min-w-72 p-0" align="start">
        <Command label="Search licenses">
          <CommandInput placeholder="Search or type an SPDX expression…" value={search} onValueChange={setSearch} />
          <CommandList>
            <CommandEmpty>No common license matches.</CommandEmpty>
            {custom && !COMMON_SPDX.some((l) => l.id.toLowerCase() === custom.toLowerCase()) && (
              <CommandGroup heading="Custom">
                <CommandItem value={`custom:${custom}`} onSelect={() => choose(custom)} className="font-mono">
                  Use “{custom}”
                </CommandItem>
              </CommandGroup>
            )}
            <CommandGroup heading="Common licenses">
              {COMMON_SPDX.map((l) => (
                <CommandItem key={l.id} value={l.id} keywords={[l.name]} onSelect={() => choose(l.id)}>
                  <Check aria-hidden="true" className={cn(value === l.id ? "opacity-100" : "opacity-0")} />
                  <span className="font-mono">{l.id}</span>
                  <span className="ml-auto truncate text-xs text-muted-foreground">{l.name}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function LicenseDialog({ initial, existing, onClose }: { initial: LicenseRule | null; existing: readonly string[]; onClose: () => void }) {
  const api = useApi();
  const { tenant, tree, summary } = useTreeContext();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const tipFiles = useTipFiles();
  const editing = initial !== null;
  const [pathGlob, setPathGlob] = useState(initial?.path_glob ?? "");
  const [spdx, setSpdx] = useState(initial?.spdx ?? "");
  const [grant, setGrant] = useState<LicenseGrant>(initial?.grant ?? "read-only");
  const [touched, setTouched] = useState(false);
  const errors = { path: licenseGlobError(pathGlob), spdx: spdxError(spdx) };
  const glob = pathGlob.trim();
  const matches = useMemo(
    () => (errors.path ? [] : tipFiles.paths.filter((p) => licenseGlobMatch(glob, p))),
    [tipFiles.paths, glob, errors.path],
  );
  const replaces = !editing && existing.includes(glob);

  const save = useMutation({
    mutationFn: (body: LicenseRule) => api.trees.putLicense(tenant, tree, body),
    onSuccess: async (r) => {
      toast.success(editing ? "License rule updated" : "License rule added", `${r.path_glob} → ${r.spdx}`);
      onClose();
      await queryClient.invalidateQueries({ queryKey: keys.licenses(tenant, tree) });
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.path || errors.spdx) return;
    save.mutate({ tree_id: summary.tree.id, path_glob: glob, spdx: spdx.trim(), grant });
  };

  const showPath = touched && errors.path;
  const showSpdx = touched && errors.spdx;

  return (
    <DialogContent className="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{editing ? `Edit license rule: ${initial.path_glob}` : "Add license rule"}</DialogTitle>
        <DialogDescription>When several rules match a file, the most specific pattern wins.</DialogDescription>
      </DialogHeader>
      <form
        id={`${id}-form`}
        onSubmit={onSubmit}
        aria-label={editing ? "Edit license rule" : "Add license rule"}
        className="grid gap-4"
        noValidate
      >
        <div className="grid gap-2">
          <Label htmlFor={`${id}-glob`}>Path glob</Label>
          <Input
            id={`${id}-glob`}
            value={pathGlob}
            onChange={(e) => setPathGlob(e.target.value)}
            readOnly={editing}
            placeholder="vendor/**"
            autoComplete="off"
            className="font-mono"
            aria-invalid={showPath ? true : undefined}
            aria-describedby={`${id}-glob-hint`}
          />
          <p id={`${id}-glob-hint`} className={showPath ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
            {showPath ||
              (replaces
                ? "A rule for this glob exists — saving replaces it."
                : "Anchored at the root. ** spans directories; a directory covers everything beneath it.")}
          </p>
          <MatchPreview
            noun={["file", "files"]}
            matches={matches}
            total={tipFiles.paths.length}
            pending={tipFiles.pending}
            empty={glob ? `No file on ${summary.tree.default_branch} matches yet.` : "Type a glob to preview the files it covers."}
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor={`${id}-spdx`}>SPDX license</Label>
          <SpdxCombobox id={`${id}-spdx`} value={spdx} onChange={setSpdx} invalid={Boolean(showSpdx)} describedBy={`${id}-spdx-hint`} />
          <p id={`${id}-spdx-hint`} className={showSpdx ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
            {showSpdx || "An identifier or expression, e.g. MIT OR Apache-2.0. Proprietary = all rights reserved."}
          </p>
        </div>
        <fieldset className="grid gap-2">
          <legend className="mb-2 text-sm font-medium">Grant to other tenants</legend>
          <RadioGroup value={grant} onValueChange={(v) => setGrant(v as LicenseGrant)} className="gap-2">
            {LICENSE_GRANTS.map((g) => (
              <Label
                key={g}
                htmlFor={`${id}-grant-${g}`}
                className="flex cursor-pointer items-start gap-3 rounded-md border p-3 font-normal has-[[data-state=checked]]:border-foreground has-[[data-state=checked]]:bg-accent"
              >
                <RadioGroupItem id={`${id}-grant-${g}`} value={g} aria-describedby={`${id}-grant-${g}-desc`} className="mt-0.5" />
                <span className="grid gap-0.5">
                  <span className="font-semibold">{GRANTS[g].title}</span>
                  <span id={`${id}-grant-${g}-desc`} className="text-xs text-muted-foreground">
                    {GRANTS[g].description}
                  </span>
                </span>
              </Label>
            ))}
          </RadioGroup>
        </fieldset>
      </form>
      <DialogFooter>
        <Button variant="outline" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" form={`${id}-form`} disabled={save.isPending}>
          {editing ? "Save changes" : "Add rule"}
        </Button>
      </DialogFooter>
    </DialogContent>
  );
}

function GrantBadge({ grant }: { grant: LicenseGrant }) {
  // Filled = most permissive, outline = intermediate, dashed = most restrictive.
  return (
    <Badge variant={grant === "redistribute" ? "default" : "outline"} className={cn("font-mono", grant === "read-only" && "border-dashed")}>
      {grant}
    </Badge>
  );
}

export function LicensesTab() {
  const api = useApi();
  const { tenant, tree, canManage } = useTreeContext();
  const licenses = useLicenses(tenant, tree);
  const tipFiles = useTipFiles();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [dialog, setDialog] = useState<{ rule: LicenseRule | null } | null>(null);

  const remove = useMutation({
    mutationFn: (glob: string) => api.trees.deleteLicense(tenant, tree, glob),
    onSuccess: async (_r, glob) => {
      toast.success("License rule removed", glob);
      await queryClient.invalidateQueries({ queryKey: keys.licenses(tenant, tree) });
    },
  });

  const rules = licenses.data ?? [];

  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem] lg:items-start">
      <section aria-labelledby="licenses-title" className="overflow-hidden rounded-xl border bg-card">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
          <div>
            <h2 id="licenses-title" className="font-semibold">
              License rules
            </h2>
            <p className="text-sm text-muted-foreground">SPDX licenses per path, enforced on reads, pushes and exports by other tenants.</p>
          </div>
          {canManage && (
            <Button size="sm" onClick={() => setDialog({ rule: null })}>
              <Plus aria-hidden="true" />
              Add rule
            </Button>
          )}
        </div>
        {licenses.isPending ? (
          <LoadingRows rows={3} label="Loading license rules" />
        ) : licenses.isError ? (
          <ErrorState error={licenses.error} onRetry={() => void licenses.refetch()} />
        ) : rules.length === 0 ? (
          <EmptyState icon={Scale} title="No license rules">
            Without rules, only tenant members can write and access follows the tree&apos;s visibility.
          </EmptyState>
        ) : (
          <Table aria-label="License rules">
            <TableHeader>
              <TableRow>
                <TableHead>Path glob</TableHead>
                <TableHead>License</TableHead>
                <TableHead>Grant</TableHead>
                {canManage && (
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {rules.map((r) => {
                const covered = tipFiles.paths.filter((p) => licenseGlobMatch(r.path_glob, p)).length;
                return (
                  <TableRow key={r.path_glob}>
                    <TableCell>
                      <span className="block font-mono font-semibold">{r.path_glob}</span>
                      {!tipFiles.pending && (
                        <span className="text-xs text-muted-foreground">
                          {covered} {covered === 1 ? "file" : "files"}
                        </span>
                      )}
                    </TableCell>
                    <TableCell className="font-mono">{r.spdx}</TableCell>
                    <TableCell>
                      <GrantBadge grant={r.grant} />
                    </TableCell>
                    {canManage && (
                      <TableCell className="text-right whitespace-nowrap">
                        <Button variant="ghost" size="icon-sm" aria-label={`Edit ${r.path_glob}`} onClick={() => setDialog({ rule: r })}>
                          <Pencil aria-hidden="true" />
                        </Button>
                        <ConfirmDelete
                          label={`Delete ${r.path_glob}`}
                          title={`Delete license rule “${r.path_glob}”?`}
                          onConfirm={() => remove.mutate(r.path_glob)}
                          pending={remove.isPending && remove.variables === r.path_glob}
                        >
                          Matching paths fall back to the next most specific rule, or to membership-only access.
                        </ConfirmDelete>
                      </TableCell>
                    )}
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </section>
      <Card className="gap-3 py-4">
        <CardHeader className="px-4">
          <CardTitle className="flex items-center gap-2 text-sm">
            <Info aria-hidden="true" className="size-4" />
            Grants
          </CardTitle>
        </CardHeader>
        <CardContent className="px-4">
          <dl className="grid gap-3 text-sm">
            {LICENSE_GRANTS.map((g) => (
              <div key={g}>
                <dt>
                  <GrantBadge grant={g} />
                </dt>
                <dd className="mt-1 text-muted-foreground">{GRANTS[g].description}</dd>
              </div>
            ))}
          </dl>
          <p className="mt-3 text-xs text-muted-foreground">Tenant members are never restricted by license rules.</p>
        </CardContent>
      </Card>
      <Dialog open={dialog !== null} onOpenChange={(open) => !open && setDialog(null)}>
        {dialog && (
          <LicenseDialog
            key={dialog.rule?.path_glob ?? "new"}
            initial={dialog.rule}
            existing={rules.map((r) => r.path_glob)}
            onClose={() => setDialog(null)}
          />
        )}
      </Dialog>
    </div>
  );
}
