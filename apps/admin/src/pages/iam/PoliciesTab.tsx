import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArchiveX, Ban, FileText, History, Pencil, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { iamKeys, useIamApi, usePolicies, type Policy } from "@/api/iam";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { PolicyEditorDialog } from "./PolicyEditorDialog";
import { DeletedPoliciesSheet, PolicyHistorySheet } from "./PolicyHistorySheet";
import { ConfirmAction, TableCard } from "./shared";
import { cleanStatement } from "./util";

interface PoliciesTabProps {
  slug: string;
  editable: boolean;
}

export function PoliciesTab({ slug, editable }: PoliciesTabProps) {
  const policies = usePolicies(slug);
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [editing, setEditing] = useState<Policy | "new" | null>(null);
  const [deleting, setDeleting] = useState<Policy | null>(null);
  const [history, setHistory] = useState<{ policy: Policy; deleted: boolean } | null>(null);
  const [showDeleted, setShowDeleted] = useState(false);

  const invalidate = () => queryClient.invalidateQueries({ queryKey: iamKeys.policies(slug) });
  const toggle = useMutation({
    mutationFn: (p: Policy) =>
      iam.policies.replace(slug, p.id, {
        name: p.name,
        description: p.description,
        enabled: !p.enabled,
        statements: p.statements.map(cleanStatement),
      }),
    onSuccess: async (p) => {
      toast.success(p.enabled ? "Policy enabled" : "Policy disabled", p.name);
      await invalidate();
    },
  });
  const remove = useMutation({
    mutationFn: (p: Policy) => iam.policies.remove(slug, p.id),
    meta: { inlineError: true },
    onSuccess: async (_r, p) => {
      toast.success("Policy deleted", p.name);
      await invalidate();
    },
  });

  return (
    <>
      <TableCard
        description="Policies add fine-grained allow and deny statements on top of roles. Disabled policies are ignored."
        actions={
          <>
            <Button size="sm" variant="outline" onClick={() => setShowDeleted(true)}>
              <ArchiveX aria-hidden="true" />
              Deleted policies
            </Button>
            {editable && (
              <Button size="sm" onClick={() => setEditing("new")}>
                <Plus aria-hidden="true" />
                New policy
              </Button>
            )}
          </>
        }
      >
        {policies.isPending ? (
          <TableSkeleton label="Loading policies" rows={3} />
        ) : policies.isError ? (
          <QueryError error={policies.error} onRetry={() => void policies.refetch()} title="Could not load policies" />
        ) : policies.data.length === 0 ? (
          <EmptyState icon={FileText} title="No policies">
            Without policies, access is decided by roles and grants alone.
          </EmptyState>
        ) : (
          <Table aria-label="Policies">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Policy</TableHead>
                <TableHead>Enabled</TableHead>
                <TableHead className="hidden sm:table-cell">Version</TableHead>
                <TableHead className="hidden sm:table-cell">Statements</TableHead>
                <TableHead className="hidden md:table-cell">Updated</TableHead>
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {policies.data.map((p) => {
                const denies = p.statements.filter((s) => s.effect === "deny").length;
                return (
                  <TableRow key={p.id} className={p.enabled ? undefined : "text-muted-foreground"}>
                    <TableCell>
                      <p className={p.enabled ? "font-medium" : "font-medium line-through"}>{p.name}</p>
                      {p.description && <p className="max-w-80 truncate text-xs text-muted-foreground">{p.description}</p>}
                    </TableCell>
                    <TableCell>
                      <Switch
                        checked={p.enabled}
                        disabled={!editable || (toggle.isPending && toggle.variables.id === p.id)}
                        onCheckedChange={() => toggle.mutate(p)}
                        aria-label={`Enable ${p.name}`}
                      />
                    </TableCell>
                    <TableCell className="hidden font-mono sm:table-cell">v{p.version}</TableCell>
                    <TableCell className="hidden sm:table-cell">
                      <span className="flex items-center gap-1.5">
                        <span className="tabular-nums">{p.statements.length}</span>
                        {denies > 0 && (
                          <Badge variant="outline">
                            <Ban aria-hidden="true" />
                            {denies} deny
                          </Badge>
                        )}
                      </span>
                    </TableCell>
                    <TableCell className="hidden md:table-cell">
                      <time dateTime={p.updated_at} title={absoluteTime(p.updated_at)}>
                        {relativeTime(p.updated_at)}
                      </time>
                    </TableCell>
                    <TableCell className="w-px text-right">
                      <div className="flex justify-end gap-1">
                        {/* v0.6 (§14.7): version history, diff and rollback. */}
                        <Button variant="ghost" size="icon-sm" aria-label={`History of ${p.name}`} onClick={() => setHistory({ policy: p, deleted: false })}>
                          <History aria-hidden="true" />
                        </Button>
                        {editable && (
                          <>
                            <Button variant="ghost" size="icon-sm" aria-label={`Edit ${p.name}`} onClick={() => setEditing(p)}>
                              <Pencil aria-hidden="true" />
                            </Button>
                            <Button variant="ghost" size="icon-sm" aria-label={`Delete ${p.name}`} onClick={() => setDeleting(p)}>
                              <Trash2 aria-hidden="true" />
                            </Button>
                          </>
                        )}
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </TableCard>
      {editable && editing !== null && (
        <PolicyEditorDialog tenant={slug} policy={editing === "new" ? null : editing} onClose={() => setEditing(null)} />
      )}
      <PolicyHistorySheet
        tenant={slug}
        policy={history?.policy ?? null}
        deleted={history?.deleted}
        editable={editable}
        onClose={() => setHistory(null)}
      />
      <DeletedPoliciesSheet
        tenant={slug}
        open={showDeleted}
        onClose={() => setShowDeleted(false)}
        onHistory={(p) => {
          setShowDeleted(false);
          setHistory({ policy: p, deleted: true });
        }}
      />
      <ConfirmAction
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        title={`Delete policy ${deleting?.name ?? ""}?`}
        description="Its statements stop applying immediately. This cannot be undone."
        confirmLabel="Delete policy"
        onConfirm={() => (deleting ? remove.mutateAsync(deleting) : Promise.resolve())}
      />
    </>
  );
}
