import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus, Tags, Trash2 } from "lucide-react";
import { useId, useState, type FormEvent, type ReactNode } from "react";
import { abacKeys, useAbacApi, useAttributes, type Attributes, type AttributeTarget } from "@/api/abac";
import { EmptyState, FormError, QueryError } from "@/components/people/states";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { FieldErrors } from "@/pages/iam/shared";
import { useToast } from "@/toast/store";
import { MAX_ATTRIBUTES, rowsFromAttributes, rowsToAttributes, validateRows, type AttributeRow } from "./abac";

interface AttributesEditorProps {
  attributes: Attributes;
  editable: boolean;
  /** Accessible name of the form / list, e.g. "Tenant attributes". */
  label: string;
  saving?: boolean;
  /** Error of the last save (server message shown inline). */
  error?: unknown;
  onSave: (attributes: Attributes) => void;
}

const sameRows = (a: readonly AttributeRow[], b: readonly AttributeRow[]) =>
  a.length === b.length && a.every((r, i) => r.key === b[i]?.key && r.value === b[i]?.value);

/** Key/value editor for one object's attributes; Save replaces the full set. */
export function AttributesEditor({ attributes, editable, label, saving = false, error, onSave }: AttributesEditorProps) {
  const id = useId();
  const [initial] = useState(() => rowsFromAttributes(attributes));
  const [rows, setRows] = useState<AttributeRow[]>(initial);
  const [submitted, setSubmitted] = useState(false);

  if (!editable) {
    if (initial.length === 0) return <p className="text-sm text-muted-foreground">No attributes.</p>;
    return (
      <dl aria-label={label} className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-sm">
        {initial.map((r) => (
          <div key={r.key} className="contents">
            <dt className="font-mono text-muted-foreground">{r.key}</dt>
            <dd className="font-mono break-all">{r.value}</dd>
          </div>
        ))}
      </dl>
    );
  }

  const validation = validateRows(rows);
  const dirty = !sameRows(rows, initial);
  const set = (i: number, patch: Partial<AttributeRow>) => setRows((list) => list.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!dirty || !validation.valid) return;
    onSave(rowsToAttributes(rows));
  };

  const reset = () => {
    setRows(initial);
    setSubmitted(false);
  };

  return (
    <form aria-label={label} onSubmit={submit} noValidate className="flex flex-col gap-4">
      {rows.length === 0 ? (
        <EmptyState icon={Tags} title="No attributes" className="py-6">
          Key/value pairs that policy conditions can match, e.g. department = eng.
        </EmptyState>
      ) : (
        <div className="grid gap-2">
          <div aria-hidden="true" className="hidden gap-2 text-xs font-medium text-muted-foreground sm:grid sm:grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)_2.25rem]">
            <span>Key</span>
            <span>Value</span>
          </div>
          <ul aria-label="Attributes" className="grid gap-2">
            {rows.map((r, i) => {
              const errs = validation.rows[i] ?? {};
              const keyErr = errs.key && (submitted || r.key !== "") ? [errs.key] : [];
              const valueErr = errs.value && (submitted || r.value !== "") ? [errs.value] : [];
              return (
                <li key={i} className="grid items-start gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)_2.25rem]">
                  <div className="grid gap-1">
                    <Input
                      aria-label={`Attribute key ${i + 1}`}
                      value={r.key}
                      onChange={(e) => set(i, { key: e.target.value })}
                      placeholder="department"
                      className="font-mono"
                      autoComplete="off"
                      spellCheck={false}
                      aria-invalid={keyErr.length > 0 || undefined}
                      aria-describedby={keyErr.length ? `${id}-k${i}` : undefined}
                    />
                    <FieldErrors id={`${id}-k${i}`} messages={keyErr} />
                  </div>
                  <div className="grid gap-1">
                    <Input
                      aria-label={`Attribute value ${i + 1}`}
                      value={r.value}
                      onChange={(e) => set(i, { value: e.target.value })}
                      placeholder="eng"
                      className="font-mono"
                      autoComplete="off"
                      spellCheck={false}
                      aria-invalid={valueErr.length > 0 || undefined}
                      aria-describedby={valueErr.length ? `${id}-v${i}` : undefined}
                    />
                    <FieldErrors id={`${id}-v${i}`} messages={valueErr} />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={`Remove attribute ${r.key.trim() || i + 1}`}
                    onClick={() => setRows((list) => list.filter((_, j) => j !== i))}
                  >
                    <Trash2 aria-hidden="true" />
                  </Button>
                </li>
              );
            })}
          </ul>
        </div>
      )}
      <FieldErrors messages={validation.general} />
      <FormError error={error} />
      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={rows.length >= MAX_ATTRIBUTES}
          onClick={() => setRows((list) => [...list, { key: "", value: "" }])}
        >
          <Plus aria-hidden="true" />
          Add attribute
        </Button>
        <span className="text-xs text-muted-foreground tabular-nums">
          {rows.length} / {MAX_ATTRIBUTES}
        </span>
        <div className="ml-auto flex gap-2">
          <Button type="button" variant="outline" size="sm" disabled={!dirty || saving} onClick={reset}>
            Reset attributes
          </Button>
          <Button type="submit" size="sm" disabled={!dirty || saving}>
            {saving ? "Saving…" : "Save attributes"}
          </Button>
        </div>
      </div>
    </form>
  );
}

interface AttributesCardProps {
  target: AttributeTarget;
  editable: boolean;
  /** Accessible name, e.g. "Tenant attributes". */
  label: string;
  title?: string;
  description?: ReactNode;
  /** "card" (default) or a plain section for sheets. */
  variant?: "card" | "section";
}

/** Loads, edits and saves the attributes of `target`. */
export function AttributesCard({ target, editable, label, title = "Attributes", description, variant = "card" }: AttributesCardProps) {
  const abac = useAbacApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const query = useAttributes(target);
  const save = useMutation({
    mutationFn: (attributes: Attributes) => abac.put(target, attributes),
    meta: { inlineError: true },
    onSuccess: (saved) => {
      queryClient.setQueryData(abacKeys.attributes(target), saved);
      const n = Object.keys(saved).length;
      toast.success("Attributes saved", `${n} attribute${n === 1 ? "" : "s"}`);
    },
  });

  const body = query.isPending ? (
    <div role="status" aria-label={`Loading ${label.toLowerCase()}`} className="grid gap-2">
      <Skeleton className="h-9 w-full" />
      <Skeleton className="h-9 w-full" />
    </div>
  ) : query.isError ? (
    <QueryError error={query.error} onRetry={() => void query.refetch()} title={`Could not load ${label.toLowerCase()}`} />
  ) : (
    <AttributesEditor
      key={JSON.stringify(query.data)}
      attributes={query.data}
      editable={editable}
      label={label}
      saving={save.isPending}
      error={save.error}
      onSave={(a) => save.mutate(a)}
    />
  );

  if (variant === "section") {
    return (
      <section aria-labelledby={`${id}-title`} className="flex flex-col gap-2">
        <h3 id={`${id}-title`} className="text-sm font-semibold">
          {title}
        </h3>
        {description && <p className="text-sm text-muted-foreground">{description}</p>}
        {body}
      </section>
    );
  }
  return (
    <Card role="region" aria-label={label}>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        {description && <CardDescription>{description}</CardDescription>}
      </CardHeader>
      <CardContent>{body}</CardContent>
    </Card>
  );
}
