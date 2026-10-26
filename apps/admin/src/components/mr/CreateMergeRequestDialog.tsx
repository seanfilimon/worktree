import { useMutation, useQueryClient } from "@tanstack/react-query";
import { GitBranch, LoaderCircle } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { useApi } from "@/api/context";
import { useBranches, useCompare } from "@/api/queries";
import type { Branch } from "@/api/types";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { describeMergeError } from "@/lib/mergeErrors";
import { mergeRequestPath } from "@/lib/paths";
import { ChangeSummary } from "./ChangeSummary";
import { MergeErrorAlert } from "./MergeErrorAlert";
import { findBranch, suggestTitle } from "./model";
// v0.7 review (§15.7): draft and requested reviewers on create.
import { splitPrincipals, type CreateMergeRequestV07 } from "@/api/v07review";
import { Checkbox } from "@/components/ui/checkbox";

interface Props {
  tenant: string;
  tree: string;
  defaultBranch: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** "New merge request" dialog with a live preview of the change summary. */
export function CreateMergeRequestDialog({ open, onOpenChange, ...rest }: Props) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>New merge request</DialogTitle>
          <DialogDescription>Propose fast-forwarding a target branch to the tip of a source branch.</DialogDescription>
        </DialogHeader>
        {/* Mounted only while open, so every opening starts from a fresh form. */}
        <CreateForm {...rest} onDone={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  );
}

/** Most recently updated branch other than `target`. */
function defaultSource(branches: readonly Branch[], target: string): string {
  const candidates = branches.filter((b) => b.name !== target);
  candidates.sort((a, b) => b.updated_at.localeCompare(a.updated_at));
  return candidates[0]?.name ?? "";
}

function CreateForm({ tenant, tree, defaultBranch, onDone }: Omit<Props, "open" | "onOpenChange"> & { onDone: () => void }) {
  const api = useApi();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const id = useId();
  const branches = useBranches(tenant, tree);
  const list = branches.data ?? [];

  const [target, setTarget] = useState(defaultBranch);
  const [chosenSource, setSource] = useState<string | null>(null);
  const [typedTitle, setTitle] = useState<string | null>(null);
  const [description, setDescription] = useState("");
  const [submitted, setSubmitted] = useState(false);
  // v0.7 review (§15.7)
  const [draft, setDraft] = useState(false);
  const [reviewers, setReviewers] = useState("");

  const source = chosenSource ?? defaultSource(list, target);
  const title = typedTitle ?? suggestTitle(source);
  const sourceTip = findBranch(list, source)?.tip ?? "";
  const targetTip = findBranch(list, target)?.tip ?? "";
  // An empty target has nothing to compare against: everything is new.
  const comparable = source !== target && sourceTip !== "" && targetTip !== "";
  const compare = useCompare(tenant, tree, comparable ? sourceTip : "", targetTip);

  const errors = {
    source: !source ? "Choose a source branch." : source === target ? "Source and target must differ." : undefined,
    title: title.trim() ? undefined : "A title is required.",
  };

  const create = useMutation({
    mutationFn: () =>
      api.mergeRequests.create(tenant, tree, {
        title: title.trim(),
        description: description.trim(),
        source_branch: source,
        target_branch: target,
        // v0.7 review (§15.7): omitted unless set.
        ...(draft ? { draft: true } : {}),
        ...(splitPrincipals(reviewers).length ? { reviewers: splitPrincipals(reviewers) } : {}),
      } satisfies CreateMergeRequestV07),
    meta: { inlineError: true },
    onSuccess: async (mr) => {
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree] });
      onDone();
      void navigate(mergeRequestPath(tenant, tree, mr.number));
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!errors.source && !errors.title) create.mutate();
  };

  const names = list.map((b) => b.name);
  const targetOptions = names.includes(target) ? names : [target, ...names];
  const showSourceError = submitted && errors.source;
  const showTitleError = submitted && errors.title;

  return (
    <form onSubmit={onSubmit} aria-label="Create merge request" noValidate className="flex flex-col gap-4">
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="flex min-w-0 flex-col gap-2">
          <Label htmlFor={`${id}-source`}>Source branch</Label>
          <Select value={source} onValueChange={setSource} disabled={branches.isPending}>
            <SelectTrigger
              id={`${id}-source`}
              className="w-full font-mono"
              aria-invalid={showSourceError ? true : undefined}
              aria-describedby={showSourceError ? `${id}-source-error` : undefined}
            >
              <SelectValue placeholder={branches.isPending ? "Loading branches…" : "Select a branch"} />
            </SelectTrigger>
            <SelectContent>
              {names.map((n) => (
                <SelectItem key={n} value={n} className="font-mono">
                  <GitBranch aria-hidden />
                  {n}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {showSourceError && (
            <p id={`${id}-source-error`} className="text-xs font-medium">
              {errors.source}
            </p>
          )}
        </div>
        <div className="flex min-w-0 flex-col gap-2">
          <Label htmlFor={`${id}-target`}>Target branch</Label>
          <Select value={target} onValueChange={setTarget}>
            <SelectTrigger id={`${id}-target`} className="w-full font-mono">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {targetOptions.map((n) => (
                <SelectItem key={n} value={n} className="font-mono">
                  <GitBranch aria-hidden />
                  {n}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      <section aria-label="Change summary" aria-live="polite" className="rounded-md border bg-muted/40 p-3">
        {!source || source === target ? (
          <p className="text-sm text-muted-foreground">Choose two different branches to preview the changes.</p>
        ) : !sourceTip ? (
          <p className="text-sm text-muted-foreground">The source branch has no snapshots yet.</p>
        ) : !targetTip ? (
          <p className="text-sm text-muted-foreground">
            The target branch is empty: merging will set it to the source tip.
          </p>
        ) : (
          <ChangeSummary
            compare={compare.data}
            pending={compare.isPending}
            error={compare.error}
            stale={compare.isPlaceholderData}
          />
        )}
      </section>

      <div className="flex flex-col gap-2">
        <Label htmlFor={`${id}-title`}>Title</Label>
        <Input
          id={`${id}-title`}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          aria-invalid={showTitleError ? true : undefined}
          aria-describedby={showTitleError ? `${id}-title-error` : undefined}
          autoComplete="off"
        />
        {showTitleError && (
          <p id={`${id}-title-error`} className="text-xs font-medium">
            {errors.title}
          </p>
        )}
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor={`${id}-description`}>Description</Label>
        <Textarea
          id={`${id}-description`}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="What changes and why (optional)"
          rows={4}
        />
      </div>

      {/* v0.7 review (§15.7): requested reviewers and draft. */}
      <div className="flex flex-col gap-2">
        <Label htmlFor={`${id}-reviewers`}>Reviewers</Label>
        <Input
          id={`${id}-reviewers`}
          value={reviewers}
          onChange={(e) => setReviewers(e.target.value)}
          placeholder={`alice, @${tenant}/core`}
          autoComplete="off"
          spellCheck={false}
          className="font-mono text-sm"
          aria-describedby={`${id}-reviewers-hint`}
        />
        <p id={`${id}-reviewers-hint`} className="text-xs text-muted-foreground">
          Optional. Users or teams, separated by spaces or commas. Owners of the changed paths are requested automatically.
        </p>
      </div>
      <div className="flex items-start gap-2">
        <Checkbox id={`${id}-draft`} checked={draft} onCheckedChange={(v) => setDraft(v === true)} aria-describedby={`${id}-draft-hint`} />
        <div className="grid gap-1">
          <Label htmlFor={`${id}-draft`}>Create as draft</Label>
          <p id={`${id}-draft-hint`} className="text-xs text-muted-foreground">
            Drafts cannot be merged and request owners only once marked ready for review.
          </p>
        </div>
      </div>

      {create.isError && <MergeErrorAlert error={describeMergeError(create.error)} />}

      <DialogFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button type="submit" disabled={create.isPending}>
          {create.isPending && <LoaderCircle aria-hidden className="animate-spin" />}
          Open merge request
        </Button>
      </DialogFooter>
    </form>
  );
}
