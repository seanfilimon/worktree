import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Building2, Globe, Lock, ShieldAlert, Trash2, Webhook } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { useApi } from "@/api/context";
import { keys } from "@/api/queries";
import { VISIBILITIES, type Visibility } from "@/api/types";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useToast } from "@/toast/store";
import { WebhooksPanel } from "@/pages/webhooks/WebhooksPanel";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";

const VISIBILITY: Record<Visibility, { icon: typeof Lock; description: string }> = {
  private: { icon: Lock, description: "Only tenant members" },
  internal: { icon: Building2, description: "Any signed-in account on this server" },
  public: { icon: Globe, description: "Anyone, including anonymous readers" },
};

function DeleteTreeDialog() {
  const api = useApi();
  const { tenant, tree } = useTreeContext();
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const id = useId();
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const confirmed = typed === tree;

  const remove = useMutation({
    mutationFn: () => api.trees.remove(tenant, tree),
    onSuccess: async () => {
      toast.success("Tree deleted", `${tenant}/${tree}`);
      setOpen(false);
      queryClient.removeQueries({ queryKey: ["tree", tenant, tree] });
      await queryClient.invalidateQueries({ queryKey: keys.tenantTrees(tenant) });
      void navigate("/trees", { replace: true });
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (confirmed) remove.mutate();
  };

  return (
    <AlertDialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setTyped("");
      }}
    >
      <AlertDialogTrigger asChild>
        <Button>
          <Trash2 aria-hidden="true" />
          Delete tree
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <form onSubmit={onSubmit} className="grid gap-4">
          <AlertDialogHeader>
            <AlertDialogTitle>
              Delete{" "}
              <span className="font-mono">
                {tenant}/{tree}
              </span>
              ?
            </AlertDialogTitle>
            <AlertDialogDescription>
              This permanently removes every branch, snapshot, tag, staged snapshot and merge request of the tree, for everyone. It cannot
              be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-confirm`}>
              Type <span className="font-mono font-bold select-all">{tree}</span> to confirm
            </Label>
            <Input
              id={`${id}-confirm`}
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
            />
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel type="button">Cancel</AlertDialogCancel>
            <Button type="submit" disabled={!confirmed || remove.isPending}>
              <Trash2 aria-hidden="true" />
              Delete this tree
            </Button>
          </AlertDialogFooter>
        </form>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function TreeSettingsTab() {
  const api = useApi();
  const { tenant, tree, summary, canManage } = useTreeContext();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const saved = summary.tree;
  const [description, setDescription] = useState(saved.description);
  const [visibility, setVisibility] = useState<Visibility>(saved.visibility);
  const [defaultBranch, setDefaultBranch] = useState(saved.default_branch);

  const changes = {
    ...(description !== saved.description && { description }),
    ...(visibility !== saved.visibility && { visibility }),
    ...(defaultBranch !== saved.default_branch && { default_branch: defaultBranch }),
  };
  const dirty = Object.keys(changes).length > 0;

  const save = useMutation({
    mutationFn: () => api.trees.update(tenant, tree, changes),
    onSuccess: async () => {
      toast.success("Tree settings saved");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.tree(tenant, tree) }),
        queryClient.invalidateQueries({ queryKey: keys.tenantTrees(tenant) }),
      ]);
    },
  });

  const reset = () => {
    setDescription(saved.description);
    setVisibility(saved.visibility);
    setDefaultBranch(saved.default_branch);
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (dirty) save.mutate();
  };

  return (
    <div className="flex max-w-3xl flex-col gap-6">
      {!canManage && (
        <Alert>
          <Lock aria-hidden="true" />
          <AlertTitle>Read-only</AlertTitle>
          <AlertDescription>Only tenant owners, admins and maintainers can change tree settings.</AlertDescription>
        </Alert>
      )}
      <form onSubmit={onSubmit} aria-label="Tree settings">
        <Card>
          <CardHeader>
            <CardTitle>General</CardTitle>
            <CardDescription>How the tree is described and who can see it.</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-5">
            <div className="grid gap-2">
              <Label htmlFor={`${id}-description`}>Description</Label>
              <Textarea
                id={`${id}-description`}
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                disabled={!canManage}
                rows={3}
                maxLength={500}
                placeholder="What lives in this tree?"
              />
            </div>
            <div className="grid gap-5 sm:grid-cols-2">
              <div className="grid content-start gap-2">
                <Label htmlFor={`${id}-visibility`}>Visibility</Label>
                <Select value={visibility} onValueChange={(v) => setVisibility(v as Visibility)} disabled={!canManage}>
                  <SelectTrigger id={`${id}-visibility`} className="w-full capitalize">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {VISIBILITIES.map((v) => {
                      const Icon = VISIBILITY[v].icon;
                      return (
                        <SelectItem key={v} value={v}>
                          <Icon aria-hidden="true" />
                          <span className="capitalize">{v}</span>
                        </SelectItem>
                      );
                    })}
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">{VISIBILITY[visibility].description}.</p>
              </div>
              <div className="grid content-start gap-2">
                <span className="text-sm leading-none font-medium">Default branch</span>
                {canManage ? (
                  <BranchSelect
                    tenant={tenant}
                    tree={tree}
                    value={defaultBranch}
                    onChange={setDefaultBranch}
                    label="Default branch"
                    className="w-full"
                  />
                ) : (
                  <p className="font-mono text-sm">{defaultBranch}</p>
                )}
                <p className="text-xs text-muted-foreground">Shown first, and the target of new merge requests.</p>
              </div>
            </div>
          </CardContent>
        </Card>
        {canManage && dirty && (
          <div
            role="region"
            aria-label="Unsaved changes"
            className="sticky bottom-4 z-10 mt-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border-2 border-foreground bg-background px-4 py-3 shadow-lg"
          >
            <p className="text-sm font-semibold">You have unsaved changes</p>
            <div className="flex gap-2">
              <Button type="button" variant="outline" size="sm" onClick={reset} disabled={save.isPending}>
                Discard
              </Button>
              <Button type="submit" size="sm" disabled={save.isPending}>
                Save changes
              </Button>
            </div>
          </div>
        )}
      </form>
      <section aria-labelledby={`${id}-webhooks`} className="flex flex-col gap-3">
        <div>
          <h2 id={`${id}-webhooks`} className="flex items-center gap-2 font-semibold">
            <Webhook aria-hidden="true" className="size-4" />
            Webhooks
          </h2>
          <p className="text-sm text-muted-foreground">Events of this tree only. Tenant-wide webhooks are managed on the tenant page.</p>
        </div>
        <WebhooksPanel
          scope={{ tenant, tree }}
          editable={canManage}
          description={
            <>
              POST signed JSON events from <span className="font-mono text-foreground">{`${tenant}/${tree}`}</span> to your services.
            </>
          }
        />
      </section>
      {canManage && (
        <Card className="border-2 border-foreground">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <ShieldAlert aria-hidden="true" className="size-4" />
              Danger zone
            </CardTitle>
            <CardDescription>Irreversible actions. Proceed with care.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-wrap items-center justify-between gap-4">
            <div className="grid gap-1">
              <p className="text-sm font-semibold">Delete this tree</p>
              <p className="text-sm text-muted-foreground">Removes its branches, history and merge requests for everyone.</p>
            </div>
            <DeleteTreeDialog />
          </CardContent>
        </Card>
      )}
    </div>
  );
}
