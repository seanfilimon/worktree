import { ChevronsUpDown, LogOut, UserCog } from "lucide-react";
import { Link } from "react-router";
import { useAuth } from "@/auth/context";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SidebarMenu, SidebarMenuButton, SidebarMenuItem, useSidebar } from "@/components/ui/sidebar";
import { ThemeRadioItems } from "./ThemeToggle";

function initials(name: string): string {
  const parts = name.trim().split(/\s+/);
  const letters = parts.length > 1 ? `${parts[0]?.[0] ?? ""}${parts.at(-1)?.[0] ?? ""}` : name.slice(0, 2);
  return letters.toUpperCase();
}

/** Sidebar footer: the signed-in account with profile, theme and sign-out. */
export function UserMenu() {
  const { account, isAdmin, signOut } = useAuth();
  const { isMobile, setOpenMobile } = useSidebar();
  if (!account) return null;
  const name = account.display_name || account.username;

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              aria-label={`Account menu for ${account.username}`}
              className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
            >
              <Avatar className="size-8 rounded-md">
                <AvatarFallback className="rounded-md bg-sidebar-primary font-mono text-xs font-semibold text-sidebar-primary-foreground">
                  {initials(name)}
                </AvatarFallback>
              </Avatar>
              <div className="grid flex-1 text-left text-sm leading-tight">
                <span className="truncate font-medium">{name}</span>
                <span className="truncate text-xs text-muted-foreground">@{account.username}</span>
              </div>
              <ChevronsUpDown aria-hidden="true" className="ml-auto" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            side={isMobile ? "top" : "right"}
            align="end"
            sideOffset={8}
            className="w-(--radix-dropdown-menu-trigger-width) min-w-56"
          >
            <DropdownMenuLabel className="font-normal">
              <div className="flex items-center gap-2">
                <div className="grid min-w-0 flex-1 leading-tight">
                  <span className="truncate text-sm font-medium">{name}</span>
                  <span className="truncate text-xs text-muted-foreground">{account.email}</span>
                </div>
                {isAdmin && <Badge variant="outline">admin</Badge>}
              </div>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem asChild>
                <Link to="/settings" onClick={() => setOpenMobile(false)}>
                  <UserCog aria-hidden="true" />
                  Profile &amp; settings
                </Link>
              </DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="text-xs text-muted-foreground">Theme</DropdownMenuLabel>
            <ThemeRadioItems />
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => void signOut()}>
              <LogOut aria-hidden="true" />
              Sign out
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}
