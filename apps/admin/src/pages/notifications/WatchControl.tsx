import { BellOff, ChevronDown, Eye } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useToast } from "@/toast/store";
import { isWatchLevel, useSetWatch, useWatch, WATCH_DESCRIPTION, WATCH_LABEL, WATCH_LEVELS, type WatchLevel, type WatchScope } from "./api";

const ICON = { participating: Eye, watching: Eye, ignoring: BellOff } as const;
const BUTTON_LABEL: Record<WatchLevel, string> = { participating: "Watch", watching: "Watching", ignoring: "Ignoring" };

/**
 * "Watch" control for a tree header (or a tenant): participating /
 * watching / ignoring plus "also email me" (§15.3). Hidden when the server
 * has no notifications service.
 */
export function WatchControl({ scope }: { scope: WatchScope }) {
  const watch = useWatch(scope);
  const set = useSetWatch();
  const toast = useToast();
  if (watch.isError || !watch.data) return null;
  const level: WatchLevel = isWatchLevel(watch.data.level) ? watch.data.level : "participating";
  const Icon = ICON[level];
  const where = scope.tree ? `${scope.tenant}/${scope.tree}` : scope.tenant;

  const change = (next: WatchLevel, email = watch.data?.email) =>
    set.mutate(
      { scope, level: next, email },
      { onSuccess: (w) => toast.success(`${WATCH_LABEL[isWatchLevel(w.level) ? w.level : next]} ${where}`, w.email ? "Emails on" : undefined) },
    );

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="h-7 gap-1.5" aria-label={`Watch settings: ${WATCH_LABEL[level]}`} disabled={set.isPending}>
          <Icon aria-hidden="true" />
          {BUTTON_LABEL[level]}
          <ChevronDown aria-hidden="true" className="opacity-60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-80">
        <DropdownMenuLabel>Notifications for {where}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuRadioGroup value={level} onValueChange={(v) => isWatchLevel(v) && v !== level && change(v)}>
          {WATCH_LEVELS.map((l) => (
            <DropdownMenuRadioItem key={l} value={l} className="items-start py-2">
              <span className="flex flex-col gap-0.5">
                <span className="font-medium">{WATCH_LABEL[l]}</span>
                <span className="text-xs text-muted-foreground">{WATCH_DESCRIPTION[l]}</span>
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuCheckboxItem checked={watch.data.email} onCheckedChange={(on) => change(level, on)} disabled={level === "ignoring"}>
          Also email me watching notifications
        </DropdownMenuCheckboxItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
