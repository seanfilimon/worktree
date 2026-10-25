import { Link, NavLink, useLocation } from "react-router";
import { useAuth } from "@/auth/context";
import { Wordmark } from "@/components/Wordmark";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
  useSidebar,
} from "@/components/ui/sidebar";
import { isNavActive, visibleNavGroups } from "./nav";
import { UserMenu } from "./UserMenu";

/** Inverted (filled) active item instead of the default subtle accent. */
const ACTIVE =
  "data-[active=true]:bg-sidebar-primary data-[active=true]:text-sidebar-primary-foreground data-[active=true]:hover:bg-sidebar-primary data-[active=true]:hover:text-sidebar-primary-foreground";

export function AppSidebar() {
  const { isAdmin } = useAuth();
  const { pathname } = useLocation();
  const { setOpenMobile } = useSidebar();
  const close = () => setOpenMobile(false);

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" asChild tooltip="W0rkTree admin">
              <Link to="/" onClick={close}>
                <span
                  aria-hidden="true"
                  className="flex aspect-square size-8 items-center justify-center rounded-md bg-sidebar-primary font-mono text-xs font-bold text-sidebar-primary-foreground"
                >
                  W0
                </span>
                <span className="grid flex-1 leading-tight">
                  <Wordmark className="text-sm" />
                  <span className="truncate text-xs text-muted-foreground">Server admin</span>
                </span>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <nav aria-label="Main" className="contents">
          {visibleNavGroups(isAdmin).map((group) => (
            <SidebarGroup key={group.label}>
              <SidebarGroupLabel>{group.label}</SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {group.items.map((item) => (
                    <SidebarMenuItem key={item.to}>
                      <SidebarMenuButton
                        asChild
                        isActive={isNavActive(item, pathname)}
                        tooltip={item.label}
                        className={ACTIVE}
                      >
                        <NavLink to={item.to} end={item.end} onClick={close}>
                          <item.icon aria-hidden="true" />
                          <span>{item.label}</span>
                        </NavLink>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          ))}
        </nav>
      </SidebarContent>

      <SidebarFooter>
        <UserMenu />
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}
