import { createContext } from "react";

export interface Crumb {
  label: string;
  to?: string;
}

/** What the current page shows in the shell's header bar. */
export interface HeaderTrail {
  crumbs: Crumb[];
  /** Plain-text page title, shown when there are no breadcrumbs. */
  title?: string;
}

/**
 * Lets `PageHeader` hand its breadcrumbs to the app shell's header bar.
 * `set(id, null)` unregisters; the most recently registered trail wins.
 * Null outside the shell, where `PageHeader` renders breadcrumbs inline.
 */
export interface HeaderTrailRegistry {
  set: (id: string, trail: HeaderTrail | null) => void;
}

export const HeaderTrailContext = createContext<HeaderTrailRegistry | null>(null);
