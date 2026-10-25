import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { cn } from "@/lib/utils";

function initials(name: string): string {
  const parts = name.split(/[\s._-]+/).filter(Boolean);
  const letters = parts.length > 1 ? parts[0]!.charAt(0) + parts[1]!.charAt(0) : name.slice(0, 2);
  return letters.toUpperCase() || "?";
}

/** Initials avatar; decorative unless `label` is set (the name is usually shown beside it). */
export function UserAvatar({
  name,
  size = "sm",
  label = false,
  className,
}: {
  name: string;
  size?: "sm" | "default";
  label?: boolean;
  className?: string;
}) {
  return (
    <Avatar size={size} className={className} aria-hidden={label ? undefined : true} aria-label={label ? name : undefined} title={name}>
      <AvatarFallback className={cn("font-medium", size === "sm" ? "text-[10px]" : "text-xs")}>{initials(name)}</AvatarFallback>
    </Avatar>
  );
}

/** Avatar followed by the username. */
export function UserChip({ name, className }: { name: string; className?: string }) {
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5", className)}>
      <UserAvatar name={name} />
      <span className="truncate font-medium">{name}</span>
    </span>
  );
}
