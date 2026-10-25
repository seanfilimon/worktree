import {
  Building2,
  FolderTree,
  LayoutDashboard,
  ScrollText,
  Server,
  Settings,
  ShieldQuestionMark,
  Users,
  type LucideIcon,
} from "lucide-react";
import { matchPath } from "react-router";

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
  /** Match the path exactly (the dashboard at "/"). */
  end?: boolean;
  /** Only shown to server admins. */
  adminOnly?: boolean;
  /** Extra search terms for the command palette. */
  keywords?: string[];
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

const NAV_GROUPS: NavGroup[] = [
  {
    label: "Overview",
    items: [{ to: "/", label: "Dashboard", icon: LayoutDashboard, end: true, keywords: ["home", "stats", "activity"] }],
  },
  {
    label: "Workspace",
    items: [
      { to: "/trees", label: "Trees", icon: FolderTree, keywords: ["repositories", "repos"] },
      { to: "/tenants", label: "Tenants", icon: Building2, keywords: ["organizations", "orgs", "members"] },
    ],
  },
  {
    label: "Administration",
    items: [
      { to: "/accounts", label: "Accounts", icon: Users, adminOnly: true, keywords: ["users"] },
      { to: "/audit", label: "Audit log", icon: ScrollText, keywords: ["events", "security"] },
      {
        to: "/iam/simulator",
        label: "Access simulator",
        icon: ShieldQuestionMark,
        keywords: ["iam", "permissions", "policies", "authorization", "effective"],
      },
      { to: "/system", label: "System", icon: Server, adminOnly: true, keywords: ["server", "health", "runtime"] },
    ],
  },
  {
    label: "Settings",
    items: [
      { to: "/settings", label: "Settings", icon: Settings, keywords: ["tokens", "password", "theme", "profile"] },
    ],
  },
];

/** Navigation groups visible to the caller; empty groups are dropped. */
export function visibleNavGroups(isAdmin: boolean): NavGroup[] {
  return NAV_GROUPS.map((g) => ({ ...g, items: g.items.filter((i) => isAdmin || !i.adminOnly) })).filter(
    (g) => g.items.length > 0,
  );
}

export function isNavActive(item: NavItem, pathname: string): boolean {
  return matchPath({ path: item.to, end: item.end ?? false }, pathname) !== null;
}
