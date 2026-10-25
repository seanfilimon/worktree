/** v0.6 server-side merge UI pieces (docs/ENGINEERING_PLAN.md §14.1, §14.9). */
import { FileWarning } from "lucide-react";
import { useId } from "react";
import type { MergeRequest } from "@/api/types";
import { defaultMergeMessage, MERGE_METHOD_LABEL, MERGE_METHODS, type MergeConflict, type Mergeability, type MergeMethod } from "@/api/v06";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Textarea } from "@/components/ui/textarea";

const METHOD_HINT: Record<MergeMethod, string> = {
  "fast-forward": "Move the target to the source tip. No new snapshot.",
  merge: "Create a merge snapshot with both tips as parents.",
  squash: "Create one snapshot with all changes, on top of the target.",
};

export function ConflictList({ conflicts }: { conflicts: readonly MergeConflict[] }) {
  if (conflicts.length === 0) return null;
  return (
    <ul aria-label="Conflicts" className="mt-1 flex flex-col gap-1">
      {conflicts.map((c) => (
        <li key={`${c.path}:${c.kind}`} className="flex flex-wrap items-center gap-2 text-foreground">
          <FileWarning aria-hidden="true" className="size-3.5 shrink-0" />
          <span className="font-mono text-xs font-semibold break-all">{c.path}</span>
          {c.kind && (
            <Badge variant="outline" className="font-mono">
              {c.kind}
            </Badge>
          )}
        </li>
      ))}
    </ul>
  );
}

interface MergeOptionsFieldsProps {
  mr: MergeRequest;
  mergeability: Mergeability;
  method: MergeMethod;
  onMethod: (m: MergeMethod) => void;
  message: string;
  onMessage: (s: string) => void;
}

/** Method picker (allowed methods enabled) and the optional message for merge / squash. */
export function MergeOptionsFields({ mr, mergeability, method, onMethod, message, onMessage }: MergeOptionsFieldsProps) {
  const id = useId();
  return (
    <div className="grid gap-4">
      <div className="grid gap-2">
        <span id={`${id}-method`} className="text-sm font-medium">
          Merge method
        </span>
        <RadioGroup
          aria-labelledby={`${id}-method`}
          value={method}
          onValueChange={(v) => {
            const next = MERGE_METHODS.find((x) => x === v);
            if (next) onMethod(next);
          }}
          className="grid gap-2"
        >
          {MERGE_METHODS.map((mm) => {
            const allowed = mergeability.methods.includes(mm);
            return (
              <Label
                key={mm}
                className="flex items-start gap-2 rounded-md border p-3 font-normal has-[[data-state=checked]]:border-foreground has-[:disabled]:opacity-60"
              >
                <RadioGroupItem value={mm} disabled={!allowed} className="mt-0.5" aria-label={MERGE_METHOD_LABEL[mm]} />
                <span className="grid gap-0.5">
                  <span className={allowed ? "font-medium" : "font-medium line-through"}>{MERGE_METHOD_LABEL[mm]}</span>
                  <span className="text-xs text-muted-foreground">
                    {allowed
                      ? METHOD_HINT[mm]
                      : mm === "fast-forward" && !mergeability.fast_forward
                        ? "Not possible: the branches have diverged."
                        : "Not allowed by the target's protection."}
                  </span>
                </span>
              </Label>
            );
          })}
        </RadioGroup>
      </div>
      {method !== "fast-forward" && (
        <div className="grid gap-2">
          <Label htmlFor={`${id}-message`}>{method === "squash" ? "Squash message" : "Merge message"}</Label>
          <Textarea
            id={`${id}-message`}
            value={message}
            onChange={(e) => onMessage(e.target.value)}
            rows={3}
            placeholder={defaultMergeMessage(method, mr)}
            className="font-mono text-xs"
          />
          <p className="text-xs text-muted-foreground">Empty = the default shown. The snapshot is authored by you.</p>
        </div>
      )}
    </div>
  );
}
