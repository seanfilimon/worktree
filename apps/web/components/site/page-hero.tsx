import type { ReactNode } from "react";
import { HeroGrid } from "@/components/home/hero-grid";
import { cx } from "@/utils/cx";
import { Band, gutter } from "./frame";
import { WordReveal } from "./motion-text";

/**
 * The opening band of every inner page (features, changelog, guides, project pages): the hairline grid, an
 * eyebrow, a large headline whose second half is muted, a lede, and optional actions or meta. The same
 * construction as the home hero, at a smaller scale.
 */
export function PageHero({
  eyebrow,
  title,
  muted,
  children,
  actions,
  meta,
  className,
}: {
  eyebrow: ReactNode;
  /** The headline; plain strings animate word by word. */
  title: string;
  /** An optional second half of the headline, in the secondary tone. */
  muted?: string;
  /** The lede under the headline. */
  children?: ReactNode;
  /** Buttons or links on the right (wide screens) or below the lede. */
  actions?: ReactNode;
  /** A small line under everything (dates, counts, versions). */
  meta?: ReactNode;
  className?: string;
}) {
  return (
    <Band frameClassName="overflow-hidden">
      <HeroGrid />
      <div
        className={cx(
          "relative pt-14 pb-12 md:pt-20 md:pb-14",
          gutter,
          className,
        )}
      >
        <p className="eyebrow animate-float-in">{eyebrow}</p>
        <h1 className="mt-5 max-w-[22ch] text-[2.25rem] leading-[1.06] font-semibold tracking-[-0.04em] text-balance sm:text-[3rem] lg:text-[3.5rem]">
          <WordReveal text={title} immediate delay={0.05} />
          {muted ? (
            <>
              {" "}
              <WordReveal
                text={muted}
                immediate
                delay={0.2}
                className="text-text-secondary"
              />
            </>
          ) : null}
        </h1>
        {children || actions ? (
          <div className="mt-8 grid gap-6 lg:grid-cols-12 lg:items-end">
            {children ? (
              <div className="animate-float-in max-w-2xl text-headline-regular leading-7 text-pretty text-text-secondary [animation-delay:200ms] md:text-[1.0625rem] lg:col-span-7">
                {children}
              </div>
            ) : null}
            {actions ? (
              <div className="animate-float-in flex flex-wrap items-center gap-2.5 [animation-delay:280ms] lg:col-span-5 lg:justify-end">
                {actions}
              </div>
            ) : null}
          </div>
        ) : null}
        {meta ? (
          <div className="animate-float-in mt-6 text-caption-1-regular text-text-secondary [animation-delay:320ms]">
            {meta}
          </div>
        ) : null}
      </div>
    </Band>
  );
}

/**
 * Long-form content inside the rails (policies, articles, guides): a readable measure with the docs' prose
 * styles. `aside` renders a sticky column on wide screens (table of contents, metadata).
 */
export function ProseBand({
  children,
  aside,
  className,
}: {
  children: ReactNode;
  aside?: ReactNode;
  className?: string;
}) {
  return (
    <Band>
      <div
        className={cx(
          "grid gap-12 py-14 md:py-16",
          aside ? "lg:grid-cols-[minmax(0,1fr)_15rem]" : "",
          gutter,
          className,
        )}
      >
        <div className="prose min-w-0 max-w-[72ch]">{children}</div>
        {aside ? (
          <aside className="hidden lg:block">
            <div className="sticky top-24 flex flex-col gap-4">{aside}</div>
          </aside>
        ) : null}
      </div>
    </Band>
  );
}
