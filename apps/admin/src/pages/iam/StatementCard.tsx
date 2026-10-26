import { Ban, Check, ChevronDown, Trash2 } from "lucide-react";
import { useId, useState, type ReactNode } from "react";
import type { IamAction, PolicyConditions, PolicyStatement, PolicyValidationError, TimeWindow } from "@/api/iam";
import { ChipsInput } from "@/components/files/ChipsInput";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { ActionPicker } from "./ActionPicker";
import { AttributeConditionsSection } from "./AttributeConditionsSection";
import { FieldErrors } from "./shared";
import { fieldErrors, fromLocalInput, PRINCIPAL_EXAMPLES, RESOURCE_EXAMPLES, toLocalInput, WEEKDAYS } from "./util";

interface StatementCardProps {
  index: number;
  tenant: string;
  statement: PolicyStatement;
  actions: readonly IamAction[];
  errors: readonly PolicyValidationError[];
  onChange: (s: PolicyStatement) => void;
  onRemove?: () => void;
}

function Field({
  id,
  label,
  hint,
  errors,
  children,
}: {
  id: string;
  label: string;
  hint?: ReactNode;
  errors: readonly string[];
  children: ReactNode;
}) {
  return (
    <div className="grid min-w-0 gap-2">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {hint && (
        <div id={`${id}-hint`} className="text-xs text-muted-foreground">
          {hint}
        </div>
      )}
      <FieldErrors id={`${id}-err`} messages={errors} />
    </div>
  );
}

function Examples({ label, values, current, onAdd }: { label: string; values: readonly string[]; current: readonly string[]; onAdd: (v: string) => void }) {
  return (
    <span className="flex flex-wrap items-center gap-1">
      <span>{label}</span>
      {values.map((v) => (
        <Button
          key={v}
          type="button"
          variant="ghost"
          size="xs"
          className="h-5 px-1.5 font-mono text-[11px] text-muted-foreground"
          disabled={current.includes(v)}
          onClick={() => onAdd(v)}
          aria-label={`Add ${v}`}
        >
          + {v}
        </Button>
      ))}
    </span>
  );
}

/** Visual editor for one policy statement. */
export function StatementCard({ index, tenant, statement: s, actions, errors, onChange, onRemove }: StatementCardProps) {
  const id = useId();
  const c = s.conditions;
  const condCount =
    (c.ip_ranges?.length ? 1 : 0) + (c.require_mfa ? 1 : 0) + (c.not_before ? 1 : 0) + (c.not_after ? 1 : 0) + (c.time_window ? 1 : 0) + (c.attributes?.length ?? 0);
  const [condOpen, setCondOpen] = useState(condCount > 0);
  const errs = (f: string) => fieldErrors(errors, index, f);
  const setCond = (patch: Partial<PolicyConditions>) => onChange({ ...s, conditions: { ...c, ...patch } });
  const setWindow = (patch: Partial<TimeWindow>) =>
    setCond({ time_window: { days: [], start: "09:00", end: "18:00", timezone: "UTC", ...c.time_window, ...patch } });
  const title = `Statement ${index + 1}`;
  const condErrors = errs("conditions");

  return (
    <Card className="gap-4 p-4" role="group" aria-label={title}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <h4 className="text-sm font-semibold">{title}</h4>
          <Badge variant={s.effect === "deny" ? "default" : "outline"} className="uppercase">
            {s.effect === "deny" ? <Ban aria-hidden="true" /> : <Check aria-hidden="true" />}
            {s.effect}
          </Badge>
        </div>
        {onRemove && (
          <Button type="button" variant="ghost" size="sm" onClick={onRemove} aria-label={`Remove ${title.toLowerCase()}`}>
            <Trash2 aria-hidden="true" />
            Remove
          </Button>
        )}
      </div>

      <div className="grid gap-4 sm:grid-cols-[auto_1fr]">
        <div className="grid content-start gap-2">
          <span id={`${id}-effect`} className="text-sm leading-none font-medium">
            Effect
          </span>
          <ToggleGroup
            type="single"
            variant="outline"
            value={s.effect}
            onValueChange={(v) => v && onChange({ ...s, effect: v === "deny" ? "deny" : "allow" })}
            aria-labelledby={`${id}-effect`}
          >
            <ToggleGroupItem value="allow" aria-label="Allow" className="data-[state=on]:bg-foreground data-[state=on]:text-background">
              <Check aria-hidden="true" />
              Allow
            </ToggleGroupItem>
            <ToggleGroupItem value="deny" aria-label="Deny" className="data-[state=on]:bg-foreground data-[state=on]:text-background">
              <Ban aria-hidden="true" />
              Deny
            </ToggleGroupItem>
          </ToggleGroup>
          <FieldErrors messages={errs("effect")} />
        </div>
        <Field id={`${id}-sid`} label="Statement id (optional)" errors={errs("sid")}>
          <Input id={`${id}-sid`} value={s.sid ?? ""} onChange={(e) => onChange({ ...s, sid: e.target.value || undefined })} placeholder="DenyFridayDeploys" />
        </Field>
      </div>

      <Field id={`${id}-actions`} label="Actions" errors={errs("actions")} hint="Concrete actions, area wildcards such as tree:* or * for everything.">
        <ActionPicker
          id={`${id}-actions`}
          actions={actions}
          value={s.actions}
          onChange={(actions) => onChange({ ...s, actions })}
          describedBy={`${id}-actions-hint`}
          invalid={errs("actions").length > 0}
        />
      </Field>

      <div className="grid gap-4 lg:grid-cols-2">
        <Field
          id={`${id}-resources`}
          label="Resources"
          errors={errs("resources")}
          hint={
            <Examples
              label="Examples:"
              values={RESOURCE_EXAMPLES(tenant)}
              current={s.resources}
              onAdd={(v) => onChange({ ...s, resources: [...s.resources, v] })}
            />
          }
        >
          <ChipsInput
            id={`${id}-resources`}
            values={s.resources}
            onChange={(resources) => onChange({ ...s, resources })}
            placeholder={`wt:tree/${tenant}/*`}
            describedBy={`${id}-resources-hint`}
          />
        </Field>
        <Field
          id={`${id}-principals`}
          label="Principals"
          errors={errs("principals")}
          hint={
            <Examples
              label="Examples:"
              values={PRINCIPAL_EXAMPLES}
              current={s.principals}
              onAdd={(v) => onChange({ ...s, principals: [...s.principals, v] })}
            />
          }
        >
          <ChipsInput
            id={`${id}-principals`}
            values={s.principals}
            onChange={(principals) => onChange({ ...s, principals })}
            placeholder="team:platform"
            describedBy={`${id}-principals-hint`}
          />
        </Field>
      </div>

      <Collapsible open={condOpen} onOpenChange={setCondOpen} className="rounded-md border">
        <CollapsibleTrigger asChild>
          <Button type="button" variant="ghost" className="w-full justify-between rounded-md">
            <span className="flex items-center gap-2">
              Conditions
              {condCount > 0 && <Badge variant="secondary">{condCount}</Badge>}
            </span>
            <ChevronDown aria-hidden="true" className={cn("transition-transform", condOpen && "rotate-180")} />
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent className="grid gap-4 border-t p-3">
          <FieldErrors messages={condErrors.filter((m) => !errs("conditions.ip_ranges").includes(m) && !errs("conditions.attributes").includes(m))} />
          <Field id={`${id}-ip`} label="IP ranges" errors={errs("conditions.ip_ranges")} hint="CIDR ranges; the request IP must be in one, e.g. 10.0.0.0/8.">
            <ChipsInput
              id={`${id}-ip`}
              values={c.ip_ranges ?? []}
              onChange={(ip_ranges) => setCond({ ip_ranges: ip_ranges.length ? ip_ranges : undefined })}
              placeholder="203.0.113.0/24"
              describedBy={`${id}-ip-hint`}
            />
          </Field>
          <div className="flex items-start justify-between gap-4 rounded-md border p-3">
            <div className="grid gap-1">
              <Label htmlFor={`${id}-mfa`}>Require MFA</Label>
              <p id={`${id}-mfa-desc`} className="text-xs text-muted-foreground">
                Only applies when the session signed in with a second factor.
              </p>
            </div>
            <Switch
              id={`${id}-mfa`}
              checked={c.require_mfa ?? false}
              onCheckedChange={(v) => setCond({ require_mfa: v || undefined })}
              aria-describedby={`${id}-mfa-desc`}
            />
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field id={`${id}-nb`} label="Not before" errors={[]}>
              <Input
                id={`${id}-nb`}
                type="datetime-local"
                value={toLocalInput(c.not_before)}
                onChange={(e) => setCond({ not_before: fromLocalInput(e.target.value) })}
              />
            </Field>
            <Field id={`${id}-na`} label="Not after" errors={[]}>
              <Input
                id={`${id}-na`}
                type="datetime-local"
                value={toLocalInput(c.not_after)}
                onChange={(e) => setCond({ not_after: fromLocalInput(e.target.value) })}
              />
            </Field>
          </div>
          <div className="grid gap-3 rounded-md border p-3">
            <div className="flex items-start justify-between gap-4">
              <div className="grid gap-1">
                <Label htmlFor={`${id}-tw`}>Time window</Label>
                <p className="text-xs text-muted-foreground">A recurring weekly window, e.g. office hours. The end may wrap midnight.</p>
              </div>
              <Switch
                id={`${id}-tw`}
                checked={c.time_window !== undefined}
                onCheckedChange={(v) => (v ? setWindow({}) : setCond({ time_window: undefined }))}
              />
            </div>
            {c.time_window && (
              <>
                <div className="grid gap-2">
                  <span id={`${id}-days`} className="text-xs font-medium">
                    Days (none = every day)
                  </span>
                  <ToggleGroup
                    type="multiple"
                    variant="outline"
                    size="sm"
                    value={c.time_window.days}
                    onValueChange={(days) => setWindow({ days: WEEKDAYS.filter((d) => days.includes(d)) })}
                    aria-labelledby={`${id}-days`}
                    className="flex-wrap"
                  >
                    {WEEKDAYS.map((d) => (
                      <ToggleGroupItem key={d} value={d} className="capitalize data-[state=on]:bg-foreground data-[state=on]:text-background">
                        {d}
                      </ToggleGroupItem>
                    ))}
                  </ToggleGroup>
                </div>
                <div className="grid gap-3 sm:grid-cols-3">
                  <Field id={`${id}-start`} label="Start" errors={[]}>
                    <Input id={`${id}-start`} type="time" value={c.time_window.start} onChange={(e) => setWindow({ start: e.target.value })} />
                  </Field>
                  <Field id={`${id}-end`} label="End" errors={[]}>
                    <Input id={`${id}-end`} type="time" value={c.time_window.end} onChange={(e) => setWindow({ end: e.target.value })} />
                  </Field>
                  <Field id={`${id}-tz`} label="Timezone" errors={[]}>
                    <Input
                      id={`${id}-tz`}
                      value={c.time_window.timezone}
                      onChange={(e) => setWindow({ timezone: e.target.value })}
                      placeholder="Europe/Berlin"
                      spellCheck={false}
                    />
                  </Field>
                </div>
              </>
            )}
          </div>
          <AttributeConditionsSection
            id={id}
            index={index}
            conditions={c.attributes ?? []}
            errors={errors}
            onChange={(attributes) => setCond({ attributes })}
          />
        </CollapsibleContent>
      </Collapsible>
    </Card>
  );
}
