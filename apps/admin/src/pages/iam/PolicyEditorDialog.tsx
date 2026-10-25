import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Braces, CircleAlert, CircleCheck, LoaderCircle, Plus, SlidersHorizontal } from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { errorMessage } from "@/api/client";
import { iamKeys, useIamActions, useIamApi, usePolicyValidation, type Policy, type PolicyStatement } from "@/api/iam";
import { FormError } from "@/components/people/states";
import { useDebouncedValue } from "@/components/shell/useDebouncedValue";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { useToast } from "@/toast/store";
import { FieldErrors } from "./shared";
import { StatementCard } from "./StatementCard";
import { cleanStatement, emptyStatement, otherErrors, parseStatementsJson } from "./util";

interface PolicyEditorDialogProps {
  tenant: string;
  /** null = new policy. */
  policy: Policy | null;
  onClose: () => void;
}

type Mode = "visual" | "json";

const toJson = (statements: PolicyStatement[]) => JSON.stringify(statements.map(cleanStatement), null, 2);

/** Create / edit a policy with a visual statement builder and a JSON mode, validated live by the server. */
export function PolicyEditorDialog({ tenant, policy, onClose }: PolicyEditorDialogProps) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const actions = useIamActions();
  const id = useId();
  const [name, setName] = useState(policy?.name ?? "");
  const [description, setDescription] = useState(policy?.description ?? "");
  const [enabled, setEnabled] = useState(policy?.enabled ?? true);
  const [statements, setStatements] = useState<PolicyStatement[]>(policy?.statements.length ? policy.statements : [emptyStatement()]);
  const [mode, setMode] = useState<Mode>("visual");
  const [jsonText, setJsonText] = useState("");
  const [jsonError, setJsonError] = useState("");
  const [touched, setTouched] = useState(false);

  const canonical = useMemo(() => JSON.stringify(statements.map(cleanStatement)), [statements]);
  const debounced = useDebouncedValue(canonical, 350);
  const validation = usePolicyValidation(tenant, debounced, !jsonError);
  const checking = validation.isFetching || debounced !== canonical;
  const vErrors = validation.data?.errors ?? [];
  const invalid = validation.data?.valid === false;

  const save = useMutation({
    mutationFn: () => {
      const body = { name: name.trim(), description: description.trim(), enabled, statements: statements.map(cleanStatement) };
      return policy ? iam.policies.replace(tenant, policy.id, body) : iam.policies.create(tenant, body);
    },
    meta: { inlineError: true },
    onSuccess: async (p) => {
      toast.success(policy ? "Policy saved" : "Policy created", `${p.name} · v${p.version}`);
      await queryClient.invalidateQueries({ queryKey: iamKeys.policies(tenant) });
      onClose();
    },
  });

  const switchMode = (next: string) => {
    if (next === "json") {
      setJsonText(toJson(statements));
      setJsonError("");
      setMode("json");
    } else if (!jsonError) {
      setMode("visual");
    }
  };

  const editJson = (text: string) => {
    setJsonText(text);
    const parsed = parseStatementsJson(text);
    if (parsed.ok) {
      setJsonError("");
      setStatements(parsed.statements);
    } else {
      setJsonError(parsed.error);
    }
  };

  const update = (i: number, s: PolicyStatement) => setStatements((list) => list.map((x, j) => (j === i ? s : x)));
  const nameErrors = name.trim() ? [] : ["Required."];

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (nameErrors.length || jsonError) return;
    save.mutate();
  };

  const general = otherErrors(vErrors, statements.length);

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[92vh] flex-col gap-4 sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>{policy ? `Edit policy ${policy.name}` : "New policy"}</DialogTitle>
          <DialogDescription>
            A deny in any enabled policy overrides every allow. Allows extend what roles grant.
            {policy && ` Saving creates version ${policy.version + 1}.`}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col gap-4" noValidate>
          <div className="grid gap-4 sm:grid-cols-[1fr_1fr_auto]">
            <div className="grid content-start gap-2">
              <Label htmlFor={`${id}-name`}>Name</Label>
              <Input
                id={`${id}-name`}
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="office-hours-deploys"
                aria-invalid={touched && nameErrors.length > 0}
              />
              {touched && <FieldErrors messages={nameErrors} />}
            </div>
            <div className="grid content-start gap-2">
              <Label htmlFor={`${id}-desc`}>Description</Label>
              <Input id={`${id}-desc`} value={description} onChange={(e) => setDescription(e.target.value)} />
            </div>
            <div className="grid content-start gap-2">
              <Label htmlFor={`${id}-enabled`}>Enabled</Label>
              <Switch id={`${id}-enabled`} checked={enabled} onCheckedChange={setEnabled} className="mt-2" />
            </div>
          </div>

          <Tabs value={mode} onValueChange={switchMode} className="min-h-0 flex-1 gap-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <TabsList>
                <TabsTrigger value="visual">
                  <SlidersHorizontal aria-hidden="true" />
                  Visual
                </TabsTrigger>
                <TabsTrigger value="json">
                  <Braces aria-hidden="true" />
                  JSON
                </TabsTrigger>
              </TabsList>
              <ValidationStatus
                checking={checking}
                error={validation.error}
                jsonError={jsonError}
                count={vErrors.length}
                valid={validation.data?.valid === true}
              />
            </div>
            <TabsContent value="visual" className="-mx-1 min-h-0 overflow-y-auto px-1">
              <div className="flex flex-col gap-3">
                {general.length > 0 && <ErrorList errors={general} />}
                {statements.map((s, i) => (
                  <StatementCard
                    key={i}
                    index={i}
                    tenant={tenant}
                    statement={s}
                    actions={actions.data ?? []}
                    errors={vErrors}
                    onChange={(next) => update(i, next)}
                    onRemove={statements.length > 1 ? () => setStatements((l) => l.filter((_, j) => j !== i)) : undefined}
                  />
                ))}
                <div>
                  <Button type="button" variant="outline" size="sm" onClick={() => setStatements((l) => [...l, emptyStatement()])}>
                    <Plus aria-hidden="true" />
                    Add statement
                  </Button>
                </div>
              </div>
            </TabsContent>
            <TabsContent value="json" className="flex min-h-0 flex-col gap-2">
              <Label htmlFor={`${id}-json`}>Statements (JSON array)</Label>
              <Textarea
                id={`${id}-json`}
                value={jsonText}
                onChange={(e) => editJson(e.target.value)}
                spellCheck={false}
                aria-invalid={jsonError !== "" || invalid}
                aria-describedby={`${id}-json-status`}
                className="min-h-72 flex-1 font-mono text-xs"
              />
              <div id={`${id}-json-status`} aria-live="polite">
                {jsonError ? (
                  <Alert>
                    <CircleAlert aria-hidden="true" />
                    <AlertTitle>Cannot parse</AlertTitle>
                    <AlertDescription>{jsonError}</AlertDescription>
                  </Alert>
                ) : (
                  vErrors.length > 0 && <ErrorList errors={vErrors} />
                )}
              </div>
            </TabsContent>
          </Tabs>

          <FormError error={save.error} />
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={save.isPending || jsonError !== ""}>
              {save.isPending ? "Saving…" : policy ? "Save policy" : "Create policy"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ValidationStatus({
  checking,
  error,
  jsonError,
  count,
  valid,
}: {
  checking: boolean;
  error: unknown;
  jsonError: string;
  count: number;
  valid: boolean;
}) {
  let content;
  if (jsonError) {
    content = (
      <>
        <CircleAlert aria-hidden="true" className="size-4" />
        JSON has errors
      </>
    );
  } else if (checking) {
    content = (
      <>
        <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
        Validating…
      </>
    );
  } else if (error) {
    content = (
      <>
        <CircleAlert aria-hidden="true" className="size-4" />
        Could not validate: {errorMessage(error)}
      </>
    );
  } else if (count > 0) {
    content = (
      <>
        <CircleAlert aria-hidden="true" className="size-4" />
        {count} problem{count === 1 ? "" : "s"} found
      </>
    );
  } else if (valid) {
    content = (
      <>
        <CircleCheck aria-hidden="true" className="size-4" />
        Statements are valid
      </>
    );
  }
  return (
    <p role="status" className="flex items-center gap-1.5 text-sm font-medium">
      {content}
    </p>
  );
}

function ErrorList({ errors }: { errors: readonly { statement: number; field: string; message: string }[] }) {
  return (
    <Alert>
      <CircleAlert aria-hidden="true" />
      <AlertTitle>Validation errors</AlertTitle>
      <AlertDescription>
        <ul aria-label="Validation errors" className="grid gap-0.5">
          {errors.map((e, i) => (
            <li key={i}>
              <span className="font-medium text-foreground">
                Statement {e.statement + 1}
                {e.field && <span className="font-mono"> · {e.field}</span>}
              </span>
              : {e.message}
            </li>
          ))}
        </ul>
      </AlertDescription>
    </Alert>
  );
}
