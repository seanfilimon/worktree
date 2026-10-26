import type { ReactNode } from "react";
import { MotionProvider } from "@/components/home/motion";
import { SiteFooter } from "@/components/site/site-footer";
import { SiteHeader } from "@/components/site/site-header";
import { SmoothScroll } from "@/components/site/smooth-scroll";

/**
 * Marketing and content pages (home, features, guides, articles, project pages): the shared header and footer
 * inside the rails, Lenis smooth scrolling, and reduced-motion-aware animations. Docs use the Fumadocs layout.
 */
export default function SiteLayout({ children }: { children: ReactNode }) {
  return (
    <SmoothScroll>
      <MotionProvider>
        <div className="site flex min-h-screen flex-1 flex-col">
          <SiteHeader />
          <main className="flex flex-1 flex-col">{children}</main>
          <SiteFooter />
        </div>
      </MotionProvider>
    </SmoothScroll>
  );
}
