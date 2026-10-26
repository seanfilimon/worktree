import { Search } from "lucide-react";
import { LiveIndicator } from "@/components/LiveIndicator";
import { Button } from "@/components/ui/button";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { Breadcrumbs } from "./Breadcrumbs";
import type { HeaderTrail } from "./headerTrail";
import { useLiveStatus } from "./liveStatus";
import { NotificationBell } from "./NotificationBell";
import { isApplePlatform } from "./shortcuts";
import { ThemeToggle } from "./ThemeToggle";

interface AppHeaderProps {
  trail: HeaderTrail | undefined;
  onOpenPalette: () => void;
}

/** Sticky top bar: sidebar toggle, breadcrumbs, search, live status, theme. */
export function AppHeader({ trail, onOpenPalette }: AppHeaderProps) {
  const live = useLiveStatus();
  const apple = isApplePlatform();
  const crumbs = trail?.crumbs.length ? trail.crumbs : trail?.title ? [{ label: trail.title }] : [];

  return (
    <header className="sticky top-0 z-20 flex h-12 shrink-0 items-center gap-2 border-b bg-background/95 px-3 backdrop-blur supports-[backdrop-filter]:bg-background/80 sm:px-4">
      <SidebarTrigger className="-ml-1 size-8" />
      <Separator orientation="vertical" className="mr-1 data-[orientation=vertical]:h-4" />
      <Breadcrumbs crumbs={crumbs} collapseOnMobile className="flex-1" />

      <div className="ml-auto flex shrink-0 items-center gap-1">
        <Button
          variant="outline"
          size="sm"
          onClick={onOpenPalette}
          aria-label="Search"
          aria-keyshortcuts={apple ? "Meta+K /" : "Control+K /"}
          className="h-8 gap-2 px-2 text-muted-foreground sm:w-56 sm:justify-start sm:px-3"
        >
          <Search aria-hidden="true" />
          <span className="hidden flex-1 text-left font-normal sm:inline">Search…</span>
          <KbdGroup aria-hidden="true" className="hidden sm:inline-flex">
            <Kbd>{apple ? "⌘" : "Ctrl"}</Kbd>
            <Kbd>K</Kbd>
          </KbdGroup>
        </Button>
        <LiveIndicator status={live} compact />
        <NotificationBell /> {/* notifications (§15.3) */}
        <ThemeToggle />
      </div>
    </header>
  );
}
