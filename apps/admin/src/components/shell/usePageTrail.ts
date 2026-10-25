import { useContext, useEffect, useId, useMemo } from "react";
import { HeaderTrailContext, type Crumb, type HeaderTrail } from "./headerTrail";

/**
 * Registers the current page's breadcrumbs (and plain-text title) with the
 * app shell's header bar. `inShell` is false outside the shell (tests, auth
 * pages), where the caller should render the breadcrumbs inline.
 */
export function usePageTrail(crumbs: Crumb[] | undefined, title?: string): { trail: HeaderTrail; inShell: boolean } {
  const registry = useContext(HeaderTrailContext);
  const id = useId();
  // Callers pass fresh arrays each render; key the trail on its content.
  const crumbsKey = JSON.stringify(crumbs ?? []);
  const trail = useMemo(() => ({ crumbs: JSON.parse(crumbsKey) as Crumb[], title }), [crumbsKey, title]);

  useEffect(() => {
    if (!registry) return;
    registry.set(id, trail);
    return () => registry.set(id, null);
  }, [registry, id, trail]);

  return { trail, inShell: registry !== null };
}
