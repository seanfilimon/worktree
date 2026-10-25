import {
  Building2,
  ChevronRight,
  Copy,
  Download,
  File,
  Folder,
  FolderTree,
  GitBranch,
  LayoutDashboard,
  Lock,
  LogOut,
  Menu,
  ScrollText,
  Settings,
  Users,
  X,
  type LucideProps,
} from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * Named lucide icons kept for pages that still use `<Icon name=…>`.
 * New code should import from `lucide-react` directly.
 */
const ICONS = {
  dashboard: LayoutDashboard,
  tenants: Building2,
  trees: FolderTree,
  audit: ScrollText,
  accounts: Users,
  settings: Settings,
  menu: Menu,
  close: X,
  folder: Folder,
  file: File,
  chevron: ChevronRight,
  logout: LogOut,
  lock: Lock,
  branch: GitBranch,
  download: Download,
  copy: Copy,
} as const;

export type IconName = keyof typeof ICONS;

/** 16px decorative icon (always aria-hidden). */
export function Icon({ name, className, ...rest }: { name: IconName } & LucideProps) {
  const Component = ICONS[name];
  return <Component aria-hidden="true" className={cn("size-4 shrink-0", className)} {...rest} />;
}
