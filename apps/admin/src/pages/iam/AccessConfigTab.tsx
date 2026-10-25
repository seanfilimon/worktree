import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, CircleX, ClipboardPaste, FileCode2, ListChecks, Play } from "lucide-react";
import { useState } from "react";
import { iamKeys } from "@/api/iam";
import {
  decodeBase64Text,
  isNotImplemented,
  opsKeys,
  parseAccessDocument,
  planItemText,
  prettyJson,
  useAccessConfig,
  useOpsApi,
  type AccessPlanResult,
} from "@/api/v05ops";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { absoluteTime, relativeTime } from "@/lib/format";
import { NotImplementedNotice } from "@/pages/tenants/opsShared";
import { useToast } from "@/toast/store";

const EXAMPLE = `{
  "roles": [
    { "name": "release-manager", "description": "Cut releases", "permissions": ["tree:read", "release:*", "mr:merge"] }
  ],
  "policies": [
    {
      "name": "protect-main",
      "enabled": true,
      "statements": [
        { "sid": "no-direct-main", "effect": "deny", "principals": ["role:developer"], "actions": ["branch:push"], "resources": ["wt:branch/acme/web/main"] }
      ]
    }
  ]
}`;

function LastApplied({ slug, onLoad }: { slug: string; onLoad: (text: string) => void }) {
  const doc = useAccessConfig(slug);
  const text = doc.data ? prettyJson(decodeBase64Text(doc.data.content)) : "";

  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle>Last applied document</CardTitle>
        <CardDescription>
          Roles and policies applied with <span className="font-mono">wt access apply</span> or below are managed by config and
          read-only elsewhere.
        </CardDescription>
        {doc.data && (
          <CardAction>
            <Button size="sm" variant="outline" onClick={() => onLoad(text)}>
              <ClipboardPaste aria-hidden="true" />
              Load into editor
            </Button>
          </CardAction>
        )}
      </CardHeader>
      {isNotImplemented(doc.error) ? (
        <div className="p-4">
          <NotImplementedNotice feature="Declarative access config" />
        </div>
      ) : doc.isPending ? (
        <TableSkeleton label="Loading access document" rows={2} columns={2} />
      ) : doc.isError ? (
        <QueryError error={doc.error} onRetry={() => void doc.refetch()} title="Could not load the access document" />
      ) : doc.data === null ? (
        <EmptyState icon={FileCode2} title="No document applied yet">
          Plan and apply a canonical JSON document, or run <span className="font-mono">wt access apply</span>.
        </EmptyState>
      ) : (
        <div className="flex flex-col">
          <dl className="grid gap-x-6 gap-y-2 border-b px-6 py-4 text-sm sm:grid-cols-[auto_1fr]">
            <dt className="text-muted-foreground">Kind</dt>
            <dd>
              <Badge variant="outline">{doc.data.kind || "combined"}</Badge>
            </dd>
            <dt className="text-muted-foreground">Hash</dt>
            <dd className="truncate font-mono text-xs" title={doc.data.hash}>
              {doc.data.hash}
            </dd>
            <dt className="text-muted-foreground">Applied</dt>
            <dd>
              <time dateTime={doc.data.updated_at} title={absoluteTime(doc.data.updated_at)}>
                {relativeTime(doc.data.updated_at)}
              </time>
              {doc.data.updated_by && (
                <>
                  {" "}
                  by <span className="font-medium">{doc.data.updated_by}</span>
                </>
              )}
            </dd>
          </dl>
          <pre
            aria-label="Applied document"
            className="max-h-96 overflow-auto bg-muted/40 px-6 py-4 font-mono text-xs leading-relaxed"
          >
            {text}
          </pre>
        </div>
      )}
    </Card>
  );
}

function PlanResult({ result, applied }: { result: AccessPlanResult; applied: boolean }) {
  const ok = result.accepted && result.errors.length === 0;
  return (
    <section aria-label={applied ? "Apply result" : "Plan result"} className="flex flex-col gap-3 rounded-md border p-4">
      <div className="flex flex-wrap items-center gap-2">
        {ok ? (
          <Badge>
            <CircleCheck aria-hidden="true" />
            {applied ? "Applied" : "Accepted"}
          </Badge>
        ) : (
          <Badge variant="outline">
            <CircleX aria-hidden="true" />
            <span className="line-through">Rejected</span>
          </Badge>
        )}
        <span className="text-sm text-muted-foreground">
          {result.changes.length === 0
            ? "No changes"
            : `${result.changes.length} ${result.changes.length === 1 ? "change" : "changes"}`}
          {result.errors.length > 0 && ` · ${result.errors.length} ${result.errors.length === 1 ? "error" : "errors"}`}
        </span>
      </div>
      {result.errors.length > 0 && (
        <div className="flex flex-col gap-1">
          <h4 className="text-sm font-semibold">Errors</h4>
          <ul aria-label="Errors" className="flex flex-col gap-1 text-sm">
            {result.errors.map((e, i) => (
              <li key={i} className="flex gap-2 font-mono text-xs">
                <CircleX aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
                <span className="font-semibold">{planItemText(e)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {result.changes.length > 0 && (
        <div className="flex flex-col gap-1">
          <h4 className="text-sm font-semibold">Changes</h4>
          <ul aria-label="Changes" className="flex flex-col gap-1">
            {result.changes.map((c, i) => (
              <li key={i} className="flex gap-2 font-mono text-xs">
                <ListChecks aria-hidden="true" className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                <span>{planItemText(c)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

/** Tenant IAM: the declarative access document (plan / apply from pasted canonical JSON). */
export function AccessConfigTab({ slug, editable }: { slug: string; editable: boolean }) {
  const ops = useOpsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [text, setText] = useState("");
  const [touched, setTouched] = useState(false);
  const [planned, setPlanned] = useState<{ text: string; result: AccessPlanResult; applied: boolean } | null>(null);
  const [confirming, setConfirming] = useState(false);
  const parsed = parseAccessDocument(text);
  const showParseError = touched && parsed.error;

  const plan = useMutation({
    mutationFn: (content: Record<string, unknown>) => ops.accessConfig.plan(slug, content),
    meta: { inlineError: true },
    onSuccess: (result) => setPlanned({ text, result, applied: false }),
  });

  const apply = useMutation({
    mutationFn: (content: Record<string, unknown>) => ops.accessConfig.apply(slug, content),
    meta: { inlineError: true },
    onSuccess: async (result) => {
      setPlanned({ text, result, applied: true });
      if (result.accepted) {
        toast.success("Access config applied", `${result.changes.length} ${result.changes.length === 1 ? "change" : "changes"}`);
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: opsKeys.accessConfig(slug) }),
          queryClient.invalidateQueries({ queryKey: iamKeys.roles(slug) }),
          queryClient.invalidateQueries({ queryKey: iamKeys.policies(slug) }),
        ]);
      }
    },
  });

  const current = planned && planned.text === text ? planned : null;
  const canApply = editable && current !== null && !current.applied && current.result.accepted && current.result.errors.length === 0;
  const mutationError = apply.error ?? plan.error;
  const busy = plan.isPending || apply.isPending;

  const onPlan = () => {
    setTouched(true);
    if (parsed.content) {
      apply.reset();
      plan.mutate(parsed.content);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <LastApplied
        slug={slug}
        onLoad={(t) => {
          setText(t);
          setTouched(false);
        }}
      />
      <Card>
        <CardHeader>
          <CardTitle>Plan and apply</CardTitle>
          <CardDescription>
            Paste the canonical JSON compiled from <span className="font-mono">.wt/access/*.toml</span> (
            <span className="font-mono">{'{"roles":[…],"policies":[…]}'}</span>). Plan is a dry run; apply replaces every
            config-managed role and policy atomically.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="grid gap-2">
            <Label htmlFor="access-config-json">Access document (JSON)</Label>
            <Textarea
              id="access-config-json"
              value={text}
              rows={12}
              spellCheck={false}
              placeholder={EXAMPLE}
              aria-invalid={showParseError ? true : undefined}
              aria-describedby={showParseError ? "access-config-json-error" : undefined}
              onChange={(e) => setText(e.target.value)}
              onBlur={() => setTouched(text.trim() !== "")}
              className="max-h-[32rem] font-mono text-xs"
            />
            {showParseError && (
              <p id="access-config-json-error" className="text-sm font-medium">
                {parsed.error}
              </p>
            )}
          </div>
          {isNotImplemented(mutationError) ? (
            <NotImplementedNotice feature="Declarative access config" />
          ) : (
            <FormError error={mutationError} />
          )}
          {current && <PlanResult result={current.result} applied={current.applied} />}
          {planned && !current && (
            <p className="text-xs text-muted-foreground" role="status">
              The document changed since the last plan. Plan again before applying.
            </p>
          )}
        </CardContent>
        <CardFooter className="flex-wrap justify-between gap-2 border-t pt-6">
          <span className="text-xs text-muted-foreground">
            {editable ? "Apply is enabled after an accepted plan of the same document." : "Tenant owners and admins can apply."}
          </span>
          <div className="flex gap-2">
            <Button variant="outline" disabled={busy || text.trim() === ""} onClick={onPlan}>
              <ListChecks aria-hidden="true" />
              {plan.isPending ? "Planning…" : "Plan"}
            </Button>
            {editable && (
              <Button disabled={!canApply || busy} onClick={() => setConfirming(true)}>
                <Play aria-hidden="true" />
                {apply.isPending ? "Applying…" : "Apply"}
              </Button>
            )}
          </div>
        </CardFooter>
      </Card>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title="Apply this access document?"
        description={`${current?.result.changes.length ?? 0} planned ${current?.result.changes.length === 1 ? "change" : "changes"} to ${slug}. Config-managed roles and policies not in the document are deleted.`}
        confirmLabel="Apply"
        pending={apply.isPending}
        onConfirm={async () => {
          if (parsed.content) await apply.mutateAsync(parsed.content);
        }}
      />
    </div>
  );
}
