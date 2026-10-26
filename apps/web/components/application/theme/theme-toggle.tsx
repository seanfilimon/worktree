'use client';

import { useEffect, useState } from 'react';
import { flushSync } from 'react-dom';
import { RiMoonLine, RiSunLine } from 'react-icons/ri';
import { useTheme } from 'next-themes';
import { cx } from '@/utils/cx';

/**
 * BoardUI ThemeToggle (`npx boardui add theme-toggle`, segmented appearance), adapted for this site:
 * the theme lives in next-themes (which Fumadocs already runs), so the visitor's system preference is the default
 * and both the docs and the marketing pages share one setting. The circular reveal from the click point is
 * BoardUI's; browsers without view transitions, and reduced-motion visitors, switch instantly.
 */

type ThemeMode = 'light' | 'dark';

const DURATION = 820;
const EASING = 'cubic-bezier(0.16, 1, 0.3, 1)';
const STYLE_ID = 'boardui-theme-transition-style';

type ViewTransitionDocument = Document & {
  startViewTransition?: (update: () => void) => { ready: Promise<void>; finished: Promise<void> };
};

let running = false;

function blurCircleMask() {
  const svg = [
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">',
    '<defs><filter id="blur" x="-50%" y="-50%" width="200%" height="200%">',
    '<feGaussianBlur stdDeviation="2" /></filter></defs>',
    '<circle cx="50" cy="50" r="42" fill="white" filter="url(#blur)" />',
    '</svg>',
  ].join('');
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
}

function installRevealStyle(x: number, y: number, radius: number) {
  document.getElementById(STYLE_ID)?.remove();
  const mask = blurCircleMask();
  const size = radius * 2.5;
  const style = document.createElement('style');
  style.id = STYLE_ID;
  style.textContent = `
    ::view-transition-old(root), ::view-transition-new(root) { animation: none; mix-blend-mode: normal; }
    ::view-transition-old(root) { z-index: 1; }
    ::view-transition-new(root) {
      z-index: 2;
      -webkit-mask-image: ${mask}; mask-image: ${mask};
      -webkit-mask-repeat: no-repeat; mask-repeat: no-repeat;
      animation: boardui-theme-mask-reveal ${DURATION}ms ${EASING} both;
    }
    @keyframes boardui-theme-mask-reveal {
      from { -webkit-mask-position: ${x}px ${y}px; mask-position: ${x}px ${y}px; -webkit-mask-size: 0 0; mask-size: 0 0; }
      to {
        -webkit-mask-position: ${x - size / 2}px ${y - size / 2}px; mask-position: ${x - size / 2}px ${y - size / 2}px;
        -webkit-mask-size: ${size}px ${size}px; mask-size: ${size}px ${size}px;
      }
    }
  `;
  document.head.appendChild(style);
  return style;
}

export function useThemeSwitch() {
  const { resolvedTheme, setTheme } = useTheme();
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  const theme: ThemeMode | undefined = mounted
    ? resolvedTheme === 'dark'
      ? 'dark'
      : 'light'
    : undefined;

  async function change(
    next: ThemeMode,
    origin?: { x: number; y: number },
    element?: HTMLElement | null,
  ) {
    if (next === theme) return;
    const apply = () => {
      // Set the class right away so the captured frame already has the new palette, then let next-themes persist it.
      document.documentElement.classList.toggle('dark', next === 'dark');
      document.documentElement.style.colorScheme = next;
      setTheme(next);
    };
    const transitionDocument = document as ViewTransitionDocument;
    if (
      !transitionDocument.startViewTransition ||
      running ||
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    ) {
      apply();
      return;
    }
    const rect = element?.getBoundingClientRect();
    const x = origin?.x ?? (rect ? rect.left + rect.width / 2 : window.innerWidth / 2);
    const y = origin?.y ?? (rect ? rect.top + rect.height / 2 : window.innerHeight / 2);
    const radius = Math.hypot(
      Math.max(x, window.innerWidth - x),
      Math.max(y, window.innerHeight - y),
    );
    running = true;
    document.documentElement.classList.add('theme-transitioning');
    const style = installRevealStyle(x, y, radius);
    try {
      const transition = transitionDocument.startViewTransition(() => flushSync(apply));
      await transition.finished;
    } catch {
      apply();
    } finally {
      style.remove();
      document.documentElement.classList.remove('theme-transitioning');
      running = false;
    }
  }

  return { theme, change };
}

export interface ThemeToggleProps {
  className?: string;
  /** `md` is BoardUI's 40px segmented control; `sm` fits a 32px toolbar. */
  size?: 'sm' | 'md';
}

export function ThemeToggle({ className, size = 'md' }: ThemeToggleProps) {
  const { theme, change } = useThemeSwitch();
  const options = [
    { mode: 'light' as const, label: 'Use light mode', Icon: RiSunLine },
    { mode: 'dark' as const, label: 'Use dark mode', Icon: RiMoonLine },
  ];
  const small = size === 'sm';

  return (
    <div
      role="group"
      aria-label="Theme"
      className={cx(
        'relative inline-flex w-fit shrink-0 items-center rounded-full bg-background-secondary-default',
        small ? 'gap-0.5 p-0.5' : 'gap-1 p-1',
        className,
      )}
    >
      <span
        aria-hidden
        className={cx(
          'pointer-events-none absolute rounded-full bg-background-primary-default shadow-xs transition-[transform,opacity] duration-200 ease',
          small ? 'top-0.5 left-0.5 size-7' : 'top-1 left-1 size-8',
          theme === undefined && 'opacity-0',
          theme === 'dark' && (small ? 'translate-x-7.5' : 'translate-x-9'),
        )}
      />
      {options.map(({ mode, label, Icon }) => {
        const selected = theme === mode;
        return (
          <button
            key={mode}
            type="button"
            aria-label={label}
            aria-pressed={selected}
            title={mode === 'light' ? 'Light mode' : 'Dark mode'}
            onClick={(event) =>
              void change(
                mode,
                event.clientX === 0 && event.clientY === 0
                  ? undefined
                  : { x: event.clientX, y: event.clientY },
                event.currentTarget,
              )
            }
            className={cx(
              'relative z-10 grid cursor-pointer place-items-center rounded-full outline-none',
              small ? 'size-7' : 'size-8',
              'transition-colors duration-150 ease focus-visible:ring-2 focus-visible:ring-border-focus-ring',
              selected
                ? 'text-foreground-icon-primary'
                : 'text-foreground-icon-secondary hover:text-foreground-icon-primary',
            )}
          >
            <Icon className="size-4" aria-hidden />
          </button>
        );
      })}
    </div>
  );
}
