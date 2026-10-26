/** Composer anchored to one diff line: a line comment or a suggested change (§15.7). */
import { MessageSquare, RotateCcw, WandSparkles } from "lucide-react";
import { useId, useMemo, useState, type FormEvent, type KeyboardEvent } from "react";
import { errorMessage } from "@/api/client";
import { useBlob } from "@/api/queries";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { CommentMutations } from "../v05/threads";
import { splitLines } from "./lines";

type Mode = "comment" | "suggest";

export interface LineComposerProps {
  tenant: string;
  tree: string;
  path: string;
  line: number;
  /** Source tip: the comment and the suggestion are written against it. */
  sourceTip?: string;
  /** Source-tip line texts visible in the diff (prefill fallback). */
  diffLines?: ReadonlyMap<number, string>;
  m: CommentMutations;
  onClose: () => void;
}

export function LineComposer({ tenant, tree, path, line, sourceTip, diffLines, m, onClose }: LineComposerProps) {
  const id = useId();
  const [mode, setMode] = useState<Mode>("comment");
  const [body, setBody] = useState("");
  const [endText, setEndText] = useState(String(line));
  const [typed, setTyped] = useState<string | null>(null);
  const [touched, setTouched] = useState(false);

  // The whole file at the source tip, for prefilling ranges beyond the diff's context lines.
  const blob = useBlob(tenant, tree, sourceTip ?? "", path, mode === "suggest" && !!sourceTip);
  const fileLines = useMemo(() => (blob.data ? splitLines(new TextDecoder().decode(blob.data)) : null), [blob.data]);

  const end = Number(endText);
  const endError =
    mode !== "suggest"
      ? null
      : !Number.isInteger(end) || end < line
        ? `An end line from ${line}.`
        : fileLines && end > fileLines.length
          ? `The file has ${fileLines.length} lines.`
          : null;

  const current = useMemo(() => {
    if (endError) return "";
    const out: string[] = [];
    for (let i = line; i <= end; i++) out.push(fileLines?.[i - 1] ?? diffLines?.get(i) ?? "");
    return out.join("\n");
  }, [line, end, endError, fileLines, diffLines]);
  // Until edited, the replacement follows the selected range.
  const replacement = typed ?? current;

  const bodyError = touched && mode === "comment" && !body.trim() ? "Write a comment." : null;
  const range = mode === "suggest" && !endError && end > line ? `lines ${line}–${end}` : `line ${line}`;

  const submit = (e?: FormEvent) => {
    e?.preventDefault();
    setTouched(true);
    if (mode === "comment") {
      if (!body.trim()) return;
      m.add.mutate({ body: body.trim(), path, line, snapshot_id: sourceTip }, { onSuccess: onClose });
      return;
    }
    if (endError) return;
    m.add.mutate(
      {
        body: body.trim(),
        path,
        line,
        snapshot_id: sourceTip,
        suggestion: { path, start_line: line, end_line: end, replacement, ...(sourceTip ? { snapshot_id: sourceTip } : {}) },
      },
      { onSuccess: onClose },
    );
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <form
      aria-label={`Comment on ${path} line ${line}`}
      onSubmit={submit}
      onKeyDown={onKeyDown}
      noValidate
      className="flex flex-col gap-2 rounded-md border bg-card p-2.5"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          aria-label="Composer mode"
          value={mode}
          onValueChange={(v) => {
            if (v === "comment" || v === "suggest") setMode(v);
          }}
        >
          <ToggleGroupItem value="comment" aria-label="Comment">
            <MessageSquare aria-hidden="true" />
            Comment
          </ToggleGroupItem>
          <ToggleGroupItem value="suggest" aria-label="Suggest change">
            <WandSparkles aria-hidden="true" />
            Suggest change
          </ToggleGroupItem>
        </ToggleGroup>
        <span className="font-mono text-xs text-muted-foreground">
          {path} · {range}
        </span>
      </div>

      {mode === "suggest" && (
        <div className="grid gap-2">
          <div className="flex flex-wrap items-end gap-3">
            <div className="grid gap-1">
              <Label htmlFor={`${id}-start`} className="text-xs text-muted-foreground">
                Start line
              </Label>
              <Input id={`${id}-start`} value={line} readOnly className="h-8 w-20 tabular-nums" />
            </div>
            <div className="grid gap-1">
              <Label htmlFor={`${id}-end`} className="text-xs text-muted-foreground">
                End line
              </Label>
              <Input
                id={`${id}-end`}
                type="number"
                inputMode="numeric"
                min={line}
                max={fileLines?.length}
                value={endText}
                onChange={(e) => setEndText(e.target.value)}
                aria-invalid={endError ? true : undefined}
                aria-describedby={endError ? `${id}-end-err` : undefined}
                className="h-8 w-20 tabular-nums"
              />
            </div>
            {typed !== null && (
              <Button type="button" variant="ghost" size="xs" onClick={() => setTyped(null)}>
                <RotateCcw aria-hidden="true" />
                Reset to current lines
              </Button>
            )}
          </div>
          {endError && (
            <p id={`${id}-end-err`} className="text-xs font-medium">
              {endError}
            </p>
          )}
          <Label htmlFor={`${id}-replacement`} className="text-xs">
            Replacement
          </Label>
          <Textarea
            id={`${id}-replacement`}
            rows={Math.min(12, Math.max(2, replacement.split("\n").length + 1))}
            value={replacement}
            spellCheck={false}
            className="font-mono text-xs"
            onChange={(e) => setTyped(e.target.value)}
            aria-describedby={`${id}-replacement-hint`}
          />
          <p id={`${id}-replacement-hint`} className="text-xs text-muted-foreground">
            Replaces {range} of the source tip. Empty deletes the lines.
          </p>
        </div>
      )}

      <Label htmlFor={`${id}-body`} className={mode === "comment" ? "sr-only" : "text-xs"}>
        {mode === "comment" ? "Line comment" : "Comment (optional)"}
      </Label>
      <Textarea
        id={`${id}-body`}
        rows={2}
        autoFocus
        value={body}
        placeholder={mode === "comment" ? "Comment on this line" : "Why this change"}
        onChange={(e) => setBody(e.target.value)}
        aria-invalid={bodyError ? true : undefined}
      />
      {bodyError && <p className="text-xs font-medium">{bodyError}</p>}
      {m.add.isError && (
        <p role="alert" className="text-xs font-medium">
          {errorMessage(m.add.error)}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <Button type="submit" size="xs" disabled={m.add.isPending}>
          {mode === "comment" ? "Add comment" : "Add suggestion"}
        </Button>
        <Button type="button" variant="ghost" size="xs" onClick={onClose}>
          Cancel
        </Button>
        <span className="ml-auto hidden text-[11px] text-muted-foreground sm:inline">Ctrl+Enter to send · Esc to cancel</span>
      </div>
    </form>
  );
}
