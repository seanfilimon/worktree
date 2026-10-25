import { GitBranch, GitCommitHorizontal, Tag } from "lucide-react";
import { useId } from "react";
import { useBranches, useSnapshots, useTags } from "@/api/queries";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue } from "@/components/ui/select";
import { firstLine, shortId } from "@/lib/format";
import { cn } from "@/lib/utils";

/** Radix Select items can't have an empty value; this stands in for "". */
const NONE = "\u0000none";

interface BranchPickerProps {
  tenant: string;
  tree: string;
  value: string;
  onChange: (branch: string) => void;
}

/** Branch selector (shadcn Select) labelled "Branch". */
export function BranchPicker({ tenant, tree, value, onChange }: BranchPickerProps) {
  const id = useId();
  const branches = useBranches(tenant, tree);
  const names = (branches.data ?? []).map((b) => b.name);
  if (value && !names.includes(value)) names.unshift(value);
  return (
    <div className="flex items-center gap-2">
      <Label htmlFor={id} className="text-xs text-muted-foreground">
        Branch
      </Label>
      <Select value={value} onValueChange={onChange} disabled={branches.isPending && names.length === 0}>
        <SelectTrigger id={id} size="sm" className="w-48 font-mono">
          <SelectValue placeholder="Select a branch" />
        </SelectTrigger>
        <SelectContent>
          {names.map((n) => (
            <SelectItem key={n} value={n} className="font-mono">
              {n}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

interface RevisionSelectProps {
  tenant: string;
  tree: string;
  /** Branch whose recent snapshots are offered. */
  branch: string;
  value: string;
  onChange: (rev: string) => void;
  label: string;
  /** Label of the "" choice; omit to require a revision. */
  emptyLabel?: string;
  className?: string;
}

/** Picks a revision: a branch, a tag or one of the branch's recent snapshots. */
export function RevisionSelect({ tenant, tree, branch, value, onChange, label, emptyLabel, className }: RevisionSelectProps) {
  const id = useId();
  const branches = useBranches(tenant, tree);
  const tags = useTags(tenant, tree);
  const snapshots = useSnapshots(tenant, tree, branch);

  const branchNames = (branches.data ?? []).map((b) => b.name);
  const tagNames = (tags.data ?? []).map((t) => t.name);
  const recent = (snapshots.data ?? []).slice(0, 20);
  const known = value === "" || branchNames.includes(value) || tagNames.includes(value) || recent.some((s) => s.id === value);

  return (
    <div className={cn("flex min-w-0 flex-col gap-1.5", className)}>
      <Label htmlFor={id} className="text-xs text-muted-foreground">
        {label}
      </Label>
      <Select value={value === "" ? (emptyLabel ? NONE : "") : value} onValueChange={(v) => onChange(v === NONE ? "" : v)}>
        <SelectTrigger id={id} className="w-full min-w-0 font-mono sm:w-64">
          <SelectValue placeholder="Select a revision" />
        </SelectTrigger>
        <SelectContent>
          {emptyLabel && (
            <>
              <SelectItem value={NONE} className="font-sans">
                {emptyLabel}
              </SelectItem>
              <SelectSeparator />
            </>
          )}
          {!known && (
            <SelectGroup>
              <SelectLabel>Current</SelectLabel>
              <SelectItem value={value} className="font-mono">
                {value}
              </SelectItem>
            </SelectGroup>
          )}
          {branchNames.length > 0 && (
            <SelectGroup>
              <SelectLabel>Branches</SelectLabel>
              {branchNames.map((n) => (
                <SelectItem key={`b:${n}`} value={n} className="font-mono">
                  <GitBranch aria-hidden="true" />
                  {n}
                </SelectItem>
              ))}
            </SelectGroup>
          )}
          {tagNames.length > 0 && (
            <SelectGroup>
              <SelectLabel>Tags</SelectLabel>
              {tagNames.map((n) => (
                <SelectItem key={`t:${n}`} value={n} className="font-mono">
                  <Tag aria-hidden="true" />
                  {n}
                </SelectItem>
              ))}
            </SelectGroup>
          )}
          {recent.length > 0 && (
            <SelectGroup>
              <SelectLabel>Recent snapshots on {branch}</SelectLabel>
              {recent.map((s) => (
                <SelectItem key={`s:${s.id}`} value={s.id}>
                  <GitCommitHorizontal aria-hidden="true" />
                  <span className="font-mono">{shortId(s.id)}</span>
                  <span className="max-w-48 truncate font-sans text-muted-foreground">{firstLine(s.message)}</span>
                </SelectItem>
              ))}
            </SelectGroup>
          )}
        </SelectContent>
      </Select>
    </div>
  );
}
