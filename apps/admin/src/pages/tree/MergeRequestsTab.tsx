import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useApi } from "@/api/context";
import { useBranches, useMergeRequests } from "@/api/queries";
import { MR_STATES, type MergeRequestState } from "@/api/types";
import { StateBadge } from "@/components/legacy/Badge";
import { Button } from "@/components/legacy/Button";
import { Section } from "@/components/legacy/Card";
import { SelectField, TextAreaField, TextField } from "@/components/legacy/Field";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/legacy/States";
import { Table, Td, Th, Tr } from "@/components/legacy/Table";
import { RelativeTime } from "@/components/legacy/Time";
import { cn } from "@/lib/cn";
import { mergeRequestPath } from "@/lib/paths";
import { useTreeContext } from "./context";

const FILTERS: { value: MergeRequestState | ""; label: string }[] = [
  ...MR_STATES.map((s) => ({ value: s, label: s[0]!.toUpperCase() + s.slice(1) })),
  { value: "", label: "All" },
];

function CreateMergeRequestForm({ onDone }: { onDone: () => void }) {
  const api = useApi();
  const { tenant, tree, summary } = useTreeContext();
  const branches = useBranches(tenant, tree);
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const names = (branches.data ?? []).map((b) => b.name);
  const defaultBranch = summary.tree.default_branch;
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [source, setSource] = useState("");
  const [target, setTarget] = useState(defaultBranch);
  const [touched, setTouched] = useState(false);
  const sourceValue = source || names.find((n) => n !== target) || "";
  const errors = {
    title: title.trim() ? undefined : "Required.",
    source: !sourceValue ? "Choose a source branch." : sourceValue === target ? "Source and target must differ." : undefined,
  };

  const create = useMutation({
    mutationFn: () =>
      api.mergeRequests.create(tenant, tree, { title: title.trim(), description: description.trim(), source_branch: sourceValue, target_branch: target }),
    onSuccess: async (mr) => {
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree] });
      onDone();
      void navigate(mergeRequestPath(tenant, tree, mr.number));
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (!errors.title && !errors.source) create.mutate();
  };

  const options = names.map((n) => (
    <option key={n} value={n}>
      {n}
    </option>
  ));

  return (
    <form onSubmit={onSubmit} aria-label="Create merge request" className="flex flex-col gap-3" noValidate>
      <div className="grid gap-3 sm:grid-cols-2">
        <SelectField label="Source branch" value={sourceValue} onChange={(e) => setSource(e.target.value)} error={touched ? errors.source : undefined}>
          {!sourceValue && <option value="">Select a branch</option>}
          {options}
        </SelectField>
        <SelectField label="Target branch" value={target} onChange={(e) => setTarget(e.target.value)}>
          {!names.includes(target) && <option value={target}>{target}</option>}
          {options}
        </SelectField>
      </div>
      <TextField label="Title" value={title} onChange={(e) => setTitle(e.target.value)} error={touched ? errors.title : undefined} autoFocus />
      <TextAreaField label="Description" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="What changes and why (optional)" />
      <div className="flex gap-2">
        <Button type="submit" variant="primary" loading={create.isPending}>
          Open merge request
        </Button>
        <Button onClick={onDone}>Cancel</Button>
      </div>
    </form>
  );
}

export function MergeRequestsTab() {
  const { tenant, tree } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const raw = params.get("state");
  const state: MergeRequestState | "" = raw === "" || raw === "all" ? "" : MR_STATES.find((s) => s === raw) ?? "open";
  const [creating, setCreating] = useState(false);
  const mrs = useMergeRequests(tenant, tree, state);

  return (
    <div className="flex flex-col gap-5">
      {creating && (
        <Section title="New merge request">
          <CreateMergeRequestForm onDone={() => setCreating(false)} />
        </Section>
      )}
      <Section
        title="Merge requests"
        flush
        actions={
          <>
            <div role="group" aria-label="Filter by state" className="flex rounded-md border border-border-strong p-0.5">
              {FILTERS.map((f) => (
                <button
                  key={f.label}
                  type="button"
                  aria-pressed={state === f.value}
                  onClick={() => setParams({ state: f.value || "all" }, { replace: true })}
                  className={cn(
                    "rounded px-2 py-0.5 text-xs font-medium",
                    state === f.value ? "bg-surface-2 text-fg" : "text-muted-foreground hover:text-fg",
                  )}
                >
                  {f.label}
                </button>
              ))}
            </div>
            {!creating && (
              <Button variant="primary" size="sm" onClick={() => setCreating(true)}>
                New merge request
              </Button>
            )}
          </>
        }
      >
        {mrs.isPending ? (
          <SkeletonRows label="Loading merge requests" />
        ) : mrs.isError ? (
          <ErrorState error={mrs.error} onRetry={() => void mrs.refetch()} />
        ) : mrs.data.length === 0 ? (
          <EmptyState title={state ? `No ${state} merge requests` : "No merge requests"} />
        ) : (
          <Table label="Merge requests">
            <thead>
              <tr>
                <Th>Merge request</Th>
                <Th>Branches</Th>
                <Th>State</Th>
                <Th className="text-right">Approvals</Th>
                <Th>Updated</Th>
              </tr>
            </thead>
            <tbody>
              {mrs.data.map((mr) => (
                <Tr key={mr.id}>
                  <Td>
                    <Link to={mergeRequestPath(tenant, tree, mr.number)} className="font-medium hover:underline">
                      <span className="mr-1.5 font-mono text-muted-foreground">!{mr.number}</span>
                      {mr.title}
                    </Link>
                    <p className="text-xs text-muted-foreground">
                      by {mr.author} · opened <RelativeTime value={mr.created_at} />
                    </p>
                  </Td>
                  <Td className="font-mono text-xs">
                    {mr.source_branch} <span className="text-subtle">→</span> {mr.target_branch}
                  </Td>
                  <Td>
                    <StateBadge value={mr.state} />
                  </Td>
                  <Td className="text-right tabular-nums">{mr.approvals?.length ?? 0}</Td>
                  <Td>
                    <RelativeTime value={mr.updated_at} className="text-muted-foreground" />
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
