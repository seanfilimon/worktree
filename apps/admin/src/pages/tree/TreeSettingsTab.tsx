import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { useApi } from "@/api/context";
import { keys, useBranches } from "@/api/queries";
import { VISIBILITIES, type Visibility } from "@/api/types";
import { Button } from "@/components/ui/Button";
import { Section } from "@/components/ui/Card";
import { ConfirmButton } from "@/components/ui/ConfirmButton";
import { SelectField, TextField } from "@/components/ui/Field";
import { Alert } from "@/components/ui/States";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";

export function TreeSettingsTab() {
  const api = useApi();
  const { tenant, tree, summary, canManage } = useTreeContext();
  const branches = useBranches(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const [description, setDescription] = useState(summary.tree.description);
  const [visibility, setVisibility] = useState<Visibility>(summary.tree.visibility);
  const [defaultBranch, setDefaultBranch] = useState(summary.tree.default_branch);
  const names = (branches.data ?? []).map((b) => b.name);
  if (!names.includes(defaultBranch)) names.unshift(defaultBranch);

  const dirty =
    description !== summary.tree.description || visibility !== summary.tree.visibility || defaultBranch !== summary.tree.default_branch;

  const save = useMutation({
    mutationFn: () => api.trees.update(tenant, tree, { description, visibility, default_branch: defaultBranch }),
    onSuccess: async () => {
      toast.success("Tree settings saved");
      await queryClient.invalidateQueries({ queryKey: keys.tree(tenant, tree) });
      await queryClient.invalidateQueries({ queryKey: keys.tenantTrees(tenant) });
    },
  });

  const remove = useMutation({
    mutationFn: () => api.trees.remove(tenant, tree),
    onSuccess: async () => {
      toast.success("Tree deleted", `${tenant}/${tree}`);
      await queryClient.invalidateQueries({ queryKey: keys.tenantTrees(tenant) });
      queryClient.removeQueries({ queryKey: ["tree", tenant, tree] });
      void navigate("/trees", { replace: true });
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    save.mutate();
  };

  return (
    <div className="flex flex-col gap-5">
      {!canManage && (
        <Alert tone="info" title="Read-only">
          Only tenant owners, admins and maintainers can change tree settings.
        </Alert>
      )}
      <Section title="General">
        <form onSubmit={onSubmit} aria-label="Tree settings" className="grid max-w-2xl gap-3">
          <TextField label="Description" value={description} onChange={(e) => setDescription(e.target.value)} disabled={!canManage} />
          <div className="grid gap-3 sm:grid-cols-2">
            <SelectField label="Visibility" value={visibility} onChange={(e) => setVisibility(e.target.value as Visibility)} disabled={!canManage}>
              {VISIBILITIES.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </SelectField>
            <SelectField label="Default branch" value={defaultBranch} onChange={(e) => setDefaultBranch(e.target.value)} disabled={!canManage}>
              {names.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </SelectField>
          </div>
          <div>
            <Button type="submit" variant="primary" loading={save.isPending} disabled={!canManage || !dirty}>
              Save changes
            </Button>
          </div>
        </form>
      </Section>
      {canManage && (
        <Section title="Danger zone" description="Deleting a tree removes its branches, history and merge requests for everyone.">
          <ConfirmButton
            variant="danger"
            size="md"
            confirmLabel={`Delete ${tenant}/${tree} permanently`}
            onConfirm={() => remove.mutate()}
            loading={remove.isPending}
          >
            Delete tree
          </ConfirmButton>
        </Section>
      )}
    </div>
  );
}
