'use client';

import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { usePathname } from 'next/navigation';
import Lenis from 'lenis';

const LenisContext = createContext<Lenis | null>(null);

/** The page's Lenis instance, or null when smooth scrolling is off (reduced motion, before mount). */
export function useLenis() {
  return useContext(LenisContext);
}

/**
 * Inertial smooth scrolling for the marketing pages (Lenis). Same-page anchor links glide to their targets and
 * land below the sticky bars (Lenis honors each target's `scroll-margin-top`), nested scroll areas (code panes,
 * the playground editors) keep their native scrolling, and visitors who prefer reduced motion get plain native
 * scrolling. Docs pages do not use it: their sidebars scroll on their own.
 */
export function SmoothScroll({ children }: { children: ReactNode }) {
  const [lenis, setLenis] = useState<Lenis | null>(null);
  const pathname = usePathname();

  useEffect(() => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const instance = new Lenis({
      autoRaf: true,
      lerp: 0.11,
      allowNestedScroll: true,
      stopInertiaOnNavigate: true,
    });

    // Lenis' own `anchors` option lets the browser jump first; take over same-page hash links instead.
    const onClick = (event: MouseEvent) => {
      if (
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey
      )
        return;
      const link = (event.target as Element | null)?.closest?.('a[href*="#"]');
      if (!(link instanceof HTMLAnchorElement)) return;
      const url = new URL(link.href);
      if (url.origin !== location.origin || url.pathname !== location.pathname || !url.hash) return;
      const target = document.getElementById(decodeURIComponent(url.hash.slice(1)));
      if (!target) return;
      event.preventDefault();
      instance.scrollTo(target, { duration: 1.1 });
      history.replaceState(history.state, '', url.hash);
    };
    document.addEventListener('click', onClick);
    setLenis(instance);
    return () => {
      document.removeEventListener('click', onClick);
      instance.destroy();
      setLenis(null);
    };
  }, []);

  // A client-side navigation starts the next page at its top, without gliding there.
  useEffect(() => {
    if (!window.location.hash) lenis?.scrollTo(0, { immediate: true, force: true });
  }, [pathname, lenis]);

  return <LenisContext.Provider value={lenis}>{children}</LenisContext.Provider>;
}
