import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { keys, useProtections } from "@/api/queries";
import type { BranchProtection } from "@/api/types";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Section } from "@/components/ui/Card";
import { ConfirmButton } from "@/components/ui/ConfirmButton";
import { Checkbox, TextField } from "@/components/ui/Field";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { Table, Td, Th, Tr } from "@/components/ui/Table";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";

interface FormState {
  pattern: string;
  no_direct_push: boolean;
  required_approvals: string;
  allowed_pushers: string;
  no_delete: boolean;
}

const EMPTY: FormState = { pattern: "", no_direct_push: true, required_approvals: "1", allowed_pushers: "", no_delete: true };

function toForm(p: BranchProtection): FormState {
  return {
    pattern: p.pattern,
    no_direct_push: p.no_direct_push,
    required_approvals: String(p.required_approvals),
    allowed_pushers: (p.allowed_pushers ?? []).join(", "),
    no_delete: p.no_delete,
  };
}

function ProtectionForm({ initial, editing, onDone }: { initial: FormState; editing: boolean; onDone: () => void }) {
  const api = useApi();
  const { tenant, tree, summary } = useTreeContext();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [form, setForm] = useState(initial);
  const [touched, setTouched] = useState(false);
  const approvals = Number(form.required_approvals);
  const errors = {
    pattern: form.pattern.trim() ? undefined : "Required, e.g. main or release/*.",
    approvals: Number.isInteger(approvals) && approvals >= 0 && approvals <= 20 ? undefined : "A whole number from 0 to 20.",
  };

  const save = useMutation({
    mutationFn: (body: BranchProtection) => api.trees.putProtection(tenant, tree, body),
    onSuccess: async (p) => {
      toast.success(editing ? "Protection updated" : "Protection added", p.pattern);
      await queryClient.invalidateQueries({ queryKey: keys.protections(tenant, tree) });
      await queryClient.invalidateQueries({ queryKey: keys.branches(tenant, tree) });
      onDone();
    },
  });

  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setForm((f) => ({ ...f, [k]: v }));

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.pattern || errors.approvals) return;
    save.mutate({
      tree_id: summary.tree.id,
      pattern: form.pattern.trim(),
      no_direct_push: form.no_direct_push,
      required_approvals: approvals,
      allowed_pushers: form.allowed_pushers
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
      no_delete: form.no_delete,
    });
  };

  return (
    <form onSubmit={onSubmit} aria-label={editing ? "Edit protection" : "Add protection"} className="flex flex-col gap-4" noValidate>
      <div className="grid gap-3 sm:grid-cols-3">
        <TextField
          label="Branch pattern"
          value={form.pattern}
          onChange={(e) => set("pattern", e.target.value)}
          readOnly={editing}
          placeholder="main"
          hint="Glob: * matches within a path segment."
          error={touched ? errors.pattern : undefined}
        />
        <TextField
          label="Required approvals"
          type="number"
          min={0}
          max={20}
          value={form.required_approvals}
          onChange={(e) => set("required_approvals", e.target.value)}
          error={touched ? errors.approvals : undefined}
        />
        <TextField
          label="Allowed pushers"
          value={form.allowed_pushers}
          onChange={(e) => set("allowed_pushers", e.target.value)}
          placeholder="alice, bob"
          hint="Comma-separated usernames; empty = role-based."
        />
      </div>
      <div className="flex flex-wrap gap-6">
        <Checkbox
          label="No direct push"
          hint="Changes only via merge requests."
          checked={form.no_direct_push}
          onChange={(e) => set("no_direct_push", e.target.checked)}
        />
        <Checkbox label="Prevent deletion" checked={form.no_delete} onChange={(e) => set("no_delete", e.target.checked)} />
      </div>
      <div className="flex gap-2">
        <Button type="submit" variant="primary" loading={save.isPending}>
          {editing ? "Save changes" : "Add protection"}
        </Button>
        <Button onClick={onDone}>Cancel</Button>
      </div>
    </form>
  );
}

export function ProtectionsTab() {
  const api = useApi();
  const { tenant, tree, canManage } = useTreeContext();
  const protections = useProtections(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const [editing, setEditing] = useState<{ initial: FormState; existing: boolean } | null>(null);

  const remove = useMutation({
    mutationFn: (pattern: string) => api.trees.deleteProtection(tenant, tree, pattern),
    onSuccess: async (_r, pattern) => {
      toast.success("Protection removed", pattern);
      await queryClient.invalidateQueries({ queryKey: keys.protections(tenant, tree) });
      await queryClient.invalidateQueries({ queryKey: keys.branches(tenant, tree) });
    },
  });

  return (
    <div className="flex flex-col gap-5">
      {editing && (
        <Section title={editing.existing ? `Edit protection: ${editing.initial.pattern}` : "Add branch protection"}>
          <ProtectionForm key={editing.initial.pattern} initial={editing.initial} editing={editing.existing} onDone={() => setEditing(null)} />
        </Section>
      )}
      <Section
        title="Branch protections"
        description="Rules applied to every branch whose name matches the pattern."
        flush
        actions={
          canManage &&
          !editing && (
            <Button variant="primary" size="sm" onClick={() => setEditing({ initial: EMPTY, existing: false })}>
              Add protection
            </Button>
          )
        }
      >
        {protections.isPending ? (
          <SkeletonRows rows={3} label="Loading protections" />
        ) : protections.isError ? (
          <ErrorState error={protections.error} onRetry={() => void protections.refetch()} />
        ) : protections.data.length === 0 ? (
          <EmptyState title="No protected branches">Protect your default branch to require reviews before merging.</EmptyState>
        ) : (
          <Table label="Branch protections">
            <thead>
              <tr>
                <Th>Pattern</Th>
                <Th>Rules</Th>
                <Th className="text-right">Approvals</Th>
                <Th>Allowed pushers</Th>
                <Th>
                  <span className="sr-only">Actions</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {protections.data.map((p) => (
                <Tr key={p.pattern}>
                  <Td className="font-mono font-medium">{p.pattern}</Td>
                  <Td>
                    <span className="flex flex-wrap gap-1">
                      {p.no_direct_push && <Badge tone="warning">no direct push</Badge>}
                      {p.no_delete && <Badge tone="danger">no delete</Badge>}
                      {!p.no_direct_push && !p.no_delete && <span className="text-subtle">—</span>}
                    </span>
                  </Td>
                  <Td className="text-right tabular-nums">{p.required_approvals}</Td>
                  <Td className="text-muted">{(p.allowed_pushers ?? []).join(", ") || "role-based"}</Td>
                  <Td className="text-right whitespace-nowrap">
                    {canManage && (
                      <>
                        <Button size="sm" variant="ghost" onClick={() => setEditing({ initial: toForm(p), existing: true })} aria-label={`Edit ${p.pattern}`}>
                          Edit
                        </Button>
                        <ConfirmButton confirmLabel="Delete" onConfirm={() => remove.mutate(p.pattern)} aria-label={`Delete ${p.pattern}`}>
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
