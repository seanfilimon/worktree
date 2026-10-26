import { Plus, Trash2 } from "lucide-react";
import { validateOverrides, type OverrideRow } from "@/components/abac/abac";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FieldErrors } from "./shared";

interface AttributeOverridesInputProps {
  id: string;
  rows: readonly OverrideRow[];
  onChange: (rows: OverrideRow[]) => void;
  /** Show errors of empty fields too (after a submit attempt). */
  showErrors: boolean;
}

/** v0.8 ABAC (§16.5): simulator `context.attributes` overrides (scoped key = value, or unset). */
export function AttributeOverridesInput({ id, rows, onChange, showErrors }: AttributeOverridesInputProps) {
  const errors = validateOverrides(rows);
  const set = (i: number, patch: Partial<OverrideRow>) => onChange(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  return (
    <div className="grid gap-2" role="group" aria-labelledby={`${id}-ov-title`}>
      <div className="grid gap-1">
        <span id={`${id}-ov-title`} className="text-sm leading-none font-medium">
          Attribute overrides
        </span>
        <p className="text-xs text-muted-foreground">
          Replace stored attributes for this simulation, e.g. <span className="font-mono">principal.department</span> = eng. Unset simulates a
          missing attribute.
        </p>
      </div>
      {rows.length > 0 && (
        <ul aria-label="Attribute overrides" className="grid gap-2">
          {rows.map((r, i) => {
            const e = errors[i] ?? {};
            const messages = [
              ...(e.key && (showErrors || r.key !== "") ? [e.key] : []),
              ...(e.value && (showErrors || r.value !== "") ? [e.value] : []),
            ];
            return (
              <li key={i} className="grid gap-1.5">
                <div className="grid grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_auto_auto] items-center gap-2">
                  <Input
                    aria-label={`Override ${i + 1} key`}
                    value={r.key}
                    onChange={(ev) => set(i, { key: ev.target.value })}
                    placeholder="principal.department"
                    className="font-mono"
                    autoComplete="off"
                    spellCheck={false}
                    aria-invalid={Boolean(e.key && (showErrors || r.key !== "")) || undefined}
                  />
                  <Input
                    aria-label={`Override ${i + 1} value`}
                    value={r.unset ? "" : r.value}
                    onChange={(ev) => set(i, { value: ev.target.value })}
                    placeholder={r.unset ? "unset" : "eng"}
                    disabled={r.unset}
                    className="font-mono"
                    autoComplete="off"
                    spellCheck={false}
                    aria-invalid={Boolean(e.value && (showErrors || r.value !== "")) || undefined}
                  />
                  <div className="flex items-center gap-1.5">
                    <Checkbox id={`${id}-ov${i}-unset`} aria-label={`Override ${i + 1} unset`} checked={r.unset} onCheckedChange={(v) => set(i, { unset: v === true })} />
                    <Label htmlFor={`${id}-ov${i}-unset`} className="text-xs font-normal">
                      Unset
                    </Label>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={`Remove override ${i + 1}`}
                    onClick={() => onChange(rows.filter((_, j) => j !== i))}
                  >
                    <Trash2 aria-hidden="true" />
                  </Button>
                </div>
                <FieldErrors messages={messages} />
              </li>
            );
          })}
        </ul>
      )}
      <div>
        <Button type="button" variant="outline" size="sm" onClick={() => onChange([...rows, { key: "", value: "", unset: false }])}>
          <Plus aria-hidden="true" />
          Add override
        </Button>
      </div>
    </div>
  );
}
