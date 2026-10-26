import { Plus, Trash2 } from "lucide-react";
import { ATTRIBUTE_OPS, ATTRIBUTE_SCOPES, type AttributeCondition, type AttributeScope } from "@/api/abac";
import type { PolicyValidationError } from "@/api/iam";
import {
  conditionText,
  isAttributeOp,
  joinConditionKey,
  MAX_CONDITIONS,
  OP_LABEL,
  opArity,
  splitConditionKey,
  validateCondition,
  valuesForOp,
} from "@/components/abac/abac";
import { ChipsInput } from "@/components/files/ChipsInput";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { FieldErrors } from "./shared";
import { fieldErrors } from "./util";

interface AttributeConditionsSectionProps {
  id: string;
  /** Statement index (for server validation errors). */
  index: number;
  conditions: readonly AttributeCondition[];
  errors: readonly PolicyValidationError[];
  /** undefined = no attribute conditions (the key is dropped). */
  onChange: (conditions: AttributeCondition[] | undefined) => void;
}

const NEW_CONDITION: AttributeCondition = { key: "principal.", op: "equals", values: [] };

/** v0.8 ABAC (§16.5): `conditions.attributes` builder of one statement; all conditions must hold. */
export function AttributeConditionsSection({ id, index, conditions, errors, onChange }: AttributeConditionsSectionProps) {
  const set = (i: number, c: AttributeCondition) => onChange(conditions.map((x, j) => (j === i ? c : x)));
  const remove = (i: number) => {
    const next = conditions.filter((_, j) => j !== i);
    onChange(next.length ? next : undefined);
  };
  const general = errors.filter((e) => e.statement === index && e.field === "conditions.attributes").map((e) => e.message);

  return (
    <div className="grid gap-3 rounded-md border p-3" role="group" aria-labelledby={`${id}-attr-title`}>
      <div className="grid gap-1">
        <span id={`${id}-attr-title`} className="text-sm leading-none font-medium">
          Attribute conditions
        </span>
        <p className="text-xs text-muted-foreground">
          All must hold. An unset attribute fails equals, in, starts with and exists, and satisfies the negated operators. Service accounts
          and anonymous callers have no principal attributes.
        </p>
      </div>
      <FieldErrors messages={general} />
      {conditions.length > 0 && (
        <ul aria-label="Attribute conditions" className="grid gap-3">
          {conditions.map((c, i) => (
            <ConditionRow
              key={i}
              id={`${id}-attr${i}`}
              n={i + 1}
              condition={c}
              serverErrors={fieldErrors(errors, index, `conditions.attributes[${i}]`)}
              onChange={(next) => set(i, next)}
              onRemove={() => remove(i)}
            />
          ))}
        </ul>
      )}
      <div className="flex items-center gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={conditions.length >= MAX_CONDITIONS}
          onClick={() => onChange([...conditions, { ...NEW_CONDITION, values: [] }])}
        >
          <Plus aria-hidden="true" />
          Add attribute condition
        </Button>
        {conditions.length > 0 && (
          <span className="text-xs text-muted-foreground tabular-nums">
            {conditions.length} / {MAX_CONDITIONS}
          </span>
        )}
      </div>
    </div>
  );
}

function ConditionRow({
  id,
  n,
  condition: c,
  serverErrors,
  onChange,
  onRemove,
}: {
  id: string;
  n: number;
  condition: AttributeCondition;
  serverErrors: string[];
  onChange: (c: AttributeCondition) => void;
  onRemove: () => void;
}) {
  const { scope, name } = splitConditionKey(c.key);
  const arity = opArity(c.op);
  const values = c.values ?? [];
  const client = validateCondition(c);
  // Only complain about what was typed; empty fields are reported by the server validator.
  const shown = [
    ...(client.key && name !== "" ? [client.key] : []),
    ...(client.values && values.some((v) => v !== "") ? [client.values] : []),
  ];
  const messages = shown.length ? shown : serverErrors;

  const setScope = (s: string) => onChange({ ...c, key: joinConditionKey(s as AttributeScope, name) });
  const setName = (v: string) => onChange({ ...c, key: scope ? joinConditionKey(scope, v) : v });
  const setOp = (op: string) => {
    const nextValues = valuesForOp(op, values);
    onChange(nextValues ? { key: c.key, op, values: nextValues } : { key: c.key, op });
  };

  return (
    <li className="grid gap-2 rounded-md border bg-muted/30 p-2.5" aria-label={`Condition ${n}`}>
      <div className="flex items-center justify-between gap-2">
        <Badge variant="outline" className="max-w-full truncate font-mono">
          {conditionText(c)}
        </Badge>
        <Button type="button" variant="ghost" size="icon" aria-label={`Remove condition ${n}`} onClick={onRemove}>
          <Trash2 aria-hidden="true" />
        </Button>
      </div>
      <div className="grid gap-2 sm:grid-cols-[8rem_minmax(0,1fr)_10rem]">
        <Select value={scope} onValueChange={setScope}>
          <SelectTrigger aria-label={`Condition ${n} scope`} className="w-full font-mono">
            <SelectValue placeholder="Scope" />
          </SelectTrigger>
          <SelectContent>
            {ATTRIBUTE_SCOPES.map((s) => (
              <SelectItem key={s} value={s} className="font-mono">
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input
          aria-label={`Condition ${n} attribute`}
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="department"
          className="font-mono"
          autoComplete="off"
          spellCheck={false}
          aria-invalid={Boolean(client.key && name !== "") || undefined}
        />
        <Select value={isAttributeOp(c.op) ? c.op : ""} onValueChange={setOp}>
          <SelectTrigger aria-label={`Condition ${n} operator`} className="w-full">
            <SelectValue placeholder="Operator" />
          </SelectTrigger>
          <SelectContent>
            {ATTRIBUTE_OPS.map((op) => (
              <SelectItem key={op} value={op}>
                {OP_LABEL[op]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {arity === "one" && (
        <Input
          aria-label={`Condition ${n} value`}
          value={values[0] ?? ""}
          onChange={(e) => onChange({ ...c, values: e.target.value === "" ? [] : [e.target.value] })}
          placeholder="eng"
          className="font-mono"
          autoComplete="off"
          spellCheck={false}
        />
      )}
      {arity === "many" && (
        <div className="grid gap-1">
          <Label htmlFor={`${id}-values`} className="sr-only">
            Condition {n} values
          </Label>
          <ChipsInput
            id={`${id}-values`}
            values={values}
            onChange={(v) => onChange({ ...c, values: v })}
            placeholder={c.op === "starts_with" ? "prefix (Enter to add)" : "value (Enter to add)"}
          />
        </div>
      )}
      <FieldErrors messages={messages} />
    </li>
  );
}
