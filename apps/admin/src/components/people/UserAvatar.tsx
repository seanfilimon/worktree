import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { cn } from "@/lib/utils";
import { initials } from "./initials";

interface UserAvatarProps {
  /** Name the initials are derived from. */
  name: string;
  size?: "sm" | "default" | "lg";
  /** Square corners for tenants/organisations. */
  square?: boolean;
  className?: string;
}

/** Monochrome initials avatar; decorative (the name is always shown next to it). */
export function UserAvatar({ name, size = "default", square = false, className }: UserAvatarProps) {
  return (
    <Avatar size={size} aria-hidden="true" className={cn(square && "rounded-md", className)}>
      <AvatarFallback className={cn("border bg-secondary font-medium text-foreground", square && "rounded-md")}>
        {initials(name)}
      </AvatarFallback>
    </Avatar>
  );
}
