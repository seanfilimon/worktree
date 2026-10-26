'use client';

import { useEffect, useRef, useState } from 'react';
import { motion, useScroll, useSpring } from 'motion/react';
import { Frame } from '@/components/site/frame';
import { cx } from '@/utils/cx';

export interface Chapter {
  id: string;
  label: string;
}

/**
 * Sticky chapter bar under the site header: highlights the chapter in view and shows how far down the page you
 * are. Its anchors glide through Lenis (components/site/smooth-scroll.tsx) and land below both sticky bars.
 */
export function ChapterNav({
  chapters,
  label = 'On this page',
}: {
  chapters: readonly Chapter[];
  /** Accessible name of the bar. */
  label?: string;
}) {
  const [active, setActive] = useState<string>();
  const list = useRef<HTMLDivElement>(null);
  const { scrollYProgress } = useScroll();
  const progress = useSpring(scrollYProgress, { stiffness: 200, damping: 40, restDelta: 0.001 });

  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries.filter((entry) => entry.isIntersecting);
        if (visible.length) setActive(visible[0]!.target.id);
      },
      { rootMargin: '-40% 0px -55% 0px' },
    );
    for (const chapter of chapters) {
      const element = document.getElementById(chapter.id);
      if (element) observer.observe(element);
    }
    return () => observer.disconnect();
  }, [chapters]);

  // Keep the active chapter visible in the horizontally scrolling list on small screens.
  useEffect(() => {
    const container = list.current;
    const item = container?.querySelector<HTMLElement>(`[data-chapter="${active}"]`);
    if (!container || !item) return;
    const left = item.offsetLeft - container.clientWidth / 2 + item.clientWidth / 2;
    container.scrollTo({ left, behavior: 'smooth' });
  }, [active]);

  return (
    <nav
      aria-label={label}
      className="sticky top-14 z-30 border-b border-separator-border bg-background-full/80 backdrop-blur-md"
    >
      <Frame>
        <div
          ref={list}
          data-lenis-prevent-horizontal
          // Links carry 10px of padding, so the row sits 10px inside the gutter and the labels line up with it.
          className="flex items-center gap-1 overflow-x-auto px-2.5 py-1.5 [scrollbar-width:none] sm:px-[22px] lg:px-[38px]"
        >
          {chapters.map((chapter, index) => (
            <a
              key={chapter.id}
              href={`#${chapter.id}`}
              data-chapter={chapter.id}
              aria-current={active === chapter.id ? 'location' : undefined}
              className={cx(
                'relative isolate inline-flex h-8 shrink-0 items-center gap-2 rounded-lg px-2.5 text-body-2-regular transition-colors',
                active === chapter.id
                  ? 'text-text-primary'
                  : 'text-text-secondary hover:text-text-primary',
              )}
            >
              {active === chapter.id ? (
                <motion.span
                  layoutId="chapter-active"
                  className="absolute inset-0 -z-10 rounded-lg bg-background-secondary-default"
                  transition={{ type: 'spring', stiffness: 450, damping: 38 }}
                />
              ) : null}
              <span className="font-mono text-caption-2-regular text-text-tertiary tabular-nums">
                {String(index + 1).padStart(2, '0')}
              </span>
              {chapter.label}
            </a>
          ))}
        </div>
        <motion.div
          aria-hidden
          className="absolute inset-x-0 -bottom-px h-px origin-left bg-text-primary"
          style={{ scaleX: progress }}
        />
      </Frame>
    </nav>
  );
}
