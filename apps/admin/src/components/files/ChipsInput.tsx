import { X } from "lucide-react";
import { useState, type KeyboardEvent } from "react";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

interface ChipsInputProps {
  id: string;
  values: readonly string[];
  onChange: (values: string[]) => void;
  placeholder?: string;
  /** Id of the element describing the field. */
  describedBy?: string;
  /** Normalises a typed value; return "" to reject it. */
  normalize?: (raw: string) => string;
  className?: string;
}

/** Free-form multi-value input: Enter, comma or space adds a chip, Backspace on empty removes the last. */
export function ChipsInput({ id, values, onChange, placeholder, describedBy, normalize = (s) => s.trim(), className }: ChipsInputProps) {
  const [draft, setDraft] = useState("");

  const commit = (raw: string) => {
    const added = raw
      .split(/[\s,]+/)
      .map(normalize)
      .filter((v) => v && !values.includes(v));
    if (added.length) onChange([...values, ...new Set(added)]);
    setDraft("");
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if ((e.key === "Enter" || e.key === "," || e.key === " ") && draft.trim()) {
      e.preventDefault();
      commit(draft);
    } else if (e.key === "Backspace" && !draft && values.length) {
      e.preventDefault();
      onChange(values.slice(0, -1));
    }
  };

  return (
    <div
      className={cn(
        "flex min-h-9 w-full flex-wrap items-center gap-1.5 rounded-md border border-input bg-transparent px-2 py-1.5 shadow-xs focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50 dark:bg-input/30",
        className,
      )}
    >
      {values.length > 0 && (
        <ul aria-label="Selected" className="contents">
          {values.map((v) => (
            <li key={v}>
              <Badge variant="secondary" className="gap-1 pr-1 font-mono">
                {v}
                <button
                  type="button"
                  aria-label={`Remove ${v}`}
                  onClick={() => onChange(values.filter((x) => x !== v))}
                  className="rounded-full p-0.5 hover:bg-foreground hover:text-background focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
                >
                  <X aria-hidden="true" className="size-3" />
                </button>
              </Badge>
            </li>
          ))}
        </ul>
      )}
      <Input
        id={id}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={onKeyDown}
        onBlur={() => draft.trim() && commit(draft)}
        placeholder={values.length ? undefined : placeholder}
        aria-describedby={describedBy}
        autoComplete="off"
        className="h-6 min-w-24 flex-1 border-0 bg-transparent px-1 py-0 shadow-none focus-visible:ring-0 dark:bg-transparent"
      />
    </div>
  );
}
