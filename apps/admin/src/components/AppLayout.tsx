import { useCallback, useMemo, useState } from "react";
import { Outlet } from "react-router";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { AppHeader } from "./shell/AppHeader";
import { AppSidebar } from "./shell/AppSidebar";
import { CommandPalette } from "./shell/CommandPalette";
import { HeaderTrailContext, type HeaderTrail, type HeaderTrailRegistry } from "./shell/headerTrail";

/** The shadcn sidebar persists its expanded state in this cookie. */
function sidebarDefaultOpen(): boolean {
  return !/(?:^|;\s*)sidebar_state=false(?:;|$)/.test(document.cookie);
}

interface TrailEntry {
  id: string;
  trail: HeaderTrail;
}

/** Authenticated app shell: collapsible sidebar, header bar and command palette. */
export function AppLayout() {
  const [sidebarOpen] = useState(sidebarDefaultOpen);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [trails, setTrails] = useState<TrailEntry[]>([]);

  const registry = useMemo<HeaderTrailRegistry>(
    () => ({
      set: (id, trail) =>
        setTrails((prev) => {
          if (!trail) return prev.filter((e) => e.id !== id);
          const i = prev.findIndex((e) => e.id === id);
          return i === -1 ? [...prev, { id, trail }] : prev.with(i, { id, trail });
        }),
    }),
    [],
  );
  const openPalette = useCallback(() => setPaletteOpen(true), []);

  return (
    <SidebarProvider defaultOpen={sidebarOpen}>
      <a
        href="#content"
        className="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50 focus:rounded-md focus:bg-background focus:px-3 focus:py-2 focus:text-sm focus:shadow-md focus:ring-2 focus:ring-ring"
      >
        Skip to content
      </a>
      <AppSidebar />
      <SidebarInset className="min-w-0">
        <AppHeader trail={trails.at(-1)?.trail} onOpenPalette={openPalette} />
        <HeaderTrailContext.Provider value={registry}>
          <div
            id="content"
            tabIndex={-1}
            className="mx-auto w-full max-w-7xl flex-1 px-4 py-6 focus:outline-none sm:px-6"
          >
            <Outlet />
          </div>
        </HeaderTrailContext.Provider>
      </SidebarInset>
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
    </SidebarProvider>
  );
}
