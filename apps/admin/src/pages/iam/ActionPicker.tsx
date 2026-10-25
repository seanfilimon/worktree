import { Check, ChevronsUpDown, X } from "lucide-react";
import { useMemo, useState } from "react";
import { actionArea, groupByArea, type IamAction } from "@/api/iam";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

interface ActionPickerProps {
  id: string;
  actions: readonly IamAction[];
  value: readonly string[];
  onChange: (value: string[]) => void;
  describedBy?: string;
  invalid?: boolean;
}

/** Multi-select of actions (with `area:*` and `*` wildcards) in a searchable popover. */
export function ActionPicker({ id, actions, value, onChange, describedBy, invalid }: ActionPickerProps) {
  const [open, setOpen] = useState(false);
  const groups = useMemo(() => groupByArea(actions, (a) => a.area || actionArea(a.action)), [actions]);
  const toggle = (a: string) => onChange(value.includes(a) ? value.filter((x) => x !== a) : [...value, a]);

  const item = (action: string, description: string) => {
    const selected = value.includes(action);
    return (
      <CommandItem key={action} value={action} keywords={[description]} onSelect={() => toggle(action)} aria-checked={selected}>
        <span
          aria-hidden="true"
          className={cn("flex size-4 items-center justify-center rounded-[4px] border", selected && "border-foreground bg-foreground text-background")}
        >
          {selected && <Check className="size-3 text-background" />}
        </span>
        <span className="font-mono text-xs">{action}</span>
        <span className="ml-auto truncate text-xs text-muted-foreground">{description}</span>
      </CommandItem>
    );
  };

  return (
    <div className="grid gap-2">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            id={id}
            type="button"
            variant="outline"
            role="combobox"
            aria-expanded={open}
            aria-describedby={describedBy}
            aria-invalid={invalid}
            className="w-full justify-between font-normal"
          >
            {value.length ? `${value.length} action${value.length === 1 ? "" : "s"} selected` : "Select actions"}
            <ChevronsUpDown aria-hidden="true" className="opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-(--radix-popover-trigger-width) min-w-72 p-0" align="start">
          <Command>
            <CommandInput placeholder="Search actions…" />
            <CommandList>
              <CommandEmpty>No matching action.</CommandEmpty>
              <CommandGroup heading="Wildcards">{item("*", "Every action")}</CommandGroup>
              {groups.map(([area, list]) => (
                <CommandGroup key={area} heading={area}>
                  {item(`${area}:*`, `Every ${area} action`)}
                  {list.map((a) => item(a.action, a.description))}
                </CommandGroup>
              ))}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
      {value.length > 0 && (
        <ul aria-label="Selected actions" className="flex flex-wrap gap-1.5">
          {value.map((a) => (
            <li key={a}>
              <Badge variant={a.endsWith("*") ? "default" : "secondary"} className="gap-1 pr-1 font-mono">
                {a}
                <button
                  type="button"
                  aria-label={`Remove ${a}`}
                  onClick={() => toggle(a)}
                  className="rounded-full p-0.5 hover:bg-background hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
                >
                  <X aria-hidden="true" className="size-3" />
                </button>
              </Badge>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
