import { Check, ChevronsUpDown, GitBranch, Lock } from "lucide-react";
import { useId, useState } from "react";
import { useBranches, useTree } from "@/api/queries";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

interface BranchSelectProps {
  tenant: string;
  tree: string;
  value: string;
  onChange: (branch: string) => void;
  /** Accessible name of the trigger. */
  label?: string;
  /** Adds an "All branches" option with value "". */
  allowAll?: boolean;
  className?: string;
}

/** Searchable branch picker (combobox) that scales to many branches. */
export function BranchSelect({ tenant, tree, value, onChange, label = "Branch", allowAll = false, className = "w-56" }: BranchSelectProps) {
  const branches = useBranches(tenant, tree);
  const summary = useTree(tenant, tree);
  const [open, setOpen] = useState(false);
  const listId = useId();
  const list = branches.data ?? [];
  const known = list.some((b) => b.name === value);
  const defaultBranch = summary.data?.tree.default_branch;

  const select = (name: string) => {
    setOpen(false);
    if (name !== value) onChange(name);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          role="combobox"
          aria-label={label}
          aria-expanded={open}
          aria-controls={open ? listId : undefined}
          disabled={branches.isPending}
          className={cn("justify-between font-mono font-normal", className)}
        >
          <span className="flex min-w-0 items-center gap-2">
            <GitBranch aria-hidden="true" className="text-muted-foreground" />
            <span className="truncate">{value || (allowAll ? "All branches" : "Select branch")}</span>
          </span>
          <ChevronsUpDown aria-hidden="true" className="opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-72 p-0" align="start">
        <Command label="Find a branch">
          <CommandInput placeholder="Find a branch…" />
          <CommandList id={listId}>
            <CommandEmpty>No branch found.</CommandEmpty>
            <CommandGroup heading={`${list.length} ${list.length === 1 ? "branch" : "branches"}`}>
              {allowAll && (
                <CommandItem value="__all__" keywords={["all branches"]} onSelect={() => select("")}>
                  <Check aria-hidden="true" className={cn(value === "" ? "opacity-100" : "opacity-0")} />
                  All branches
                </CommandItem>
              )}
              {!known && value && (
                <CommandItem value={value} onSelect={() => select(value)} className="font-mono">
                  <Check aria-hidden="true" />
                  {value}
                </CommandItem>
              )}
              {list.map((b) => (
                <CommandItem key={b.name} value={b.name} onSelect={() => select(b.name)} className="font-mono">
                  <Check aria-hidden="true" className={cn(b.name === value ? "opacity-100" : "opacity-0")} />
                  <span className="min-w-0 flex-1 truncate">{b.name}</span>
                  {b.name === defaultBranch && <span className="font-sans text-[11px] font-semibold">default</span>}
                  {b.protected && (
                    <>
                      <Lock aria-hidden="true" className="size-3.5" />
                      <span className="sr-only">protected</span>
                    </>
                  )}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
