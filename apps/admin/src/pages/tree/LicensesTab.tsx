import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { keys, useLicenses } from "@/api/queries";
import { LICENSE_GRANTS, type LicenseGrant, type LicenseRule } from "@/api/types";
import { Badge, type BadgeTone } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Section } from "@/components/ui/Card";
import { ConfirmButton } from "@/components/ui/ConfirmButton";
import { SelectField, TextField } from "@/components/ui/Field";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { Table, Td, Th, Tr } from "@/components/ui/Table";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";

const GRANT_TONES: Record<LicenseGrant, BadgeTone> = { "read-only": "warning", modify: "info", redistribute: "accent" };
const GRANT_HINTS: Record<LicenseGrant, string> = {
  "read-only": "Other tenants may read but not change these paths.",
  modify: "Other tenants may modify, but not redistribute.",
  redistribute: "Other tenants may modify and redistribute.",
};

function LicenseForm({ initial, editing, onDone }: { initial: LicenseRule | null; editing: boolean; onDone: () => void }) {
  const api = useApi();
  const { tenant, tree, summary } = useTreeContext();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [pathGlob, setPathGlob] = useState(initial?.path_glob ?? "");
  const [spdx, setSpdx] = useState(initial?.spdx ?? "");
  const [grant, setGrant] = useState<LicenseGrant>(initial?.grant ?? "read-only");
  const [touched, setTouched] = useState(false);
  const errors = {
    path: pathGlob.trim() ? undefined : "Required, e.g. vendor/** or **.",
    spdx: /^[A-Za-z0-9.+-]+(?:\s+(?:AND|OR|WITH)\s+[A-Za-z0-9.+-]+)*$/.test(spdx.trim()) ? undefined : "An SPDX identifier, e.g. MIT or Apache-2.0.",
  };

  const save = useMutation({
    mutationFn: (body: LicenseRule) => api.trees.putLicense(tenant, tree, body),
    onSuccess: async (r) => {
      toast.success(editing ? "License rule updated" : "License rule added", `${r.path_glob} → ${r.spdx}`);
      await queryClient.invalidateQueries({ queryKey: keys.licenses(tenant, tree) });
      onDone();
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.path || errors.spdx) return;
    save.mutate({ tree_id: summary.tree.id, path_glob: pathGlob.trim(), spdx: spdx.trim(), grant });
  };

  return (
    <form onSubmit={onSubmit} aria-label={editing ? "Edit license rule" : "Add license rule"} className="grid gap-3 sm:grid-cols-[2fr_1fr_1fr_auto] sm:items-start" noValidate>
      <TextField
        label="Path glob"
        value={pathGlob}
        onChange={(e) => setPathGlob(e.target.value)}
        readOnly={editing}
        placeholder="vendor/**"
        error={touched ? errors.path : undefined}
      />
      <TextField label="SPDX license" value={spdx} onChange={(e) => setSpdx(e.target.value)} placeholder="MIT" error={touched ? errors.spdx : undefined} />
      <SelectField label="Grant" value={grant} onChange={(e) => setGrant(e.target.value as LicenseGrant)} hint={GRANT_HINTS[grant]}>
        {LICENSE_GRANTS.map((g) => (
          <option key={g} value={g}>
            {g}
          </option>
        ))}
      </SelectField>
      <div className="flex gap-2 sm:pt-5">
        <Button type="submit" variant="primary" loading={save.isPending}>
          {editing ? "Save" : "Add rule"}
        </Button>
        <Button onClick={onDone}>Cancel</Button>
      </div>
    </form>
  );
}

export function LicensesTab() {
  const api = useApi();
  const { tenant, tree, canManage } = useTreeContext();
  const licenses = useLicenses(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const [editing, setEditing] = useState<{ rule: LicenseRule | null } | null>(null);

  const remove = useMutation({
    mutationFn: (glob: string) => api.trees.deleteLicense(tenant, tree, glob),
    onSuccess: async (_r, glob) => {
      toast.success("License rule removed", glob);
      await queryClient.invalidateQueries({ queryKey: keys.licenses(tenant, tree) });
    },
  });

  return (
    <div className="flex flex-col gap-5">
      {editing && (
        <Section title={editing.rule ? `Edit rule: ${editing.rule.path_glob}` : "Add license rule"}>
          <LicenseForm key={editing.rule?.path_glob ?? "new"} initial={editing.rule} editing={editing.rule !== null} onDone={() => setEditing(null)} />
        </Section>
      )}
      <Section
        title="License rules"
        description="SPDX licenses per path. Pushes and exports by other tenants are checked against the grant."
        flush
        actions={
          canManage &&
          !editing && (
            <Button variant="primary" size="sm" onClick={() => setEditing({ rule: null })}>
              Add rule
            </Button>
          )
        }
      >
        {licenses.isPending ? (
          <SkeletonRows rows={3} label="Loading license rules" />
        ) : licenses.isError ? (
          <ErrorState error={licenses.error} onRetry={() => void licenses.refetch()} />
        ) : licenses.data.length === 0 ? (
          <EmptyState title="No license rules">Without rules, only tree members can write and access follows visibility.</EmptyState>
        ) : (
          <Table label="License rules">
            <thead>
              <tr>
                <Th>Path glob</Th>
                <Th>License</Th>
                <Th>Grant</Th>
                <Th>
                  <span className="sr-only">Actions</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {licenses.data.map((r) => (
                <Tr key={r.path_glob}>
                  <Td className="font-mono font-medium">{r.path_glob}</Td>
                  <Td className="font-mono">{r.spdx}</Td>
                  <Td>
                    <Badge tone={GRANT_TONES[r.grant] ?? "neutral"}>{r.grant}</Badge>
                  </Td>
                  <Td className="text-right whitespace-nowrap">
                    {canManage && (
                      <>
                        <Button size="sm" variant="ghost" onClick={() => setEditing({ rule: r })} aria-label={`Edit ${r.path_glob}`}>
                          Edit
                        </Button>
                        <ConfirmButton confirmLabel="Delete" onConfirm={() => remove.mutate(r.path_glob)} aria-label={`Delete ${r.path_glob}`}>
                          Delete
                        </ConfirmButton>
                      </>
                    )}
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </Section>
    </div>
  );
}
