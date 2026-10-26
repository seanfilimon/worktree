'use client';

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { MotionConfig, motion, useInView, useReducedMotion } from 'motion/react';

/** The landing page's one easing curve: a quick start and a long, soft settle. */
export const ease = [0.16, 1, 0.3, 1] as const;

/** Respects the visitor's reduced-motion setting for every animation below it. */
export function MotionProvider({ children }: { children: ReactNode }) {
  return <MotionConfig reducedMotion="user">{children}</MotionConfig>;
}

/**
 * The reduced-motion setting, but only after hydration. `useReducedMotion` knows the real value on the first client
 * render while the server does not, and rendering from it directly makes hydration fail.
 */
export function useReducedMotionSafe() {
  const reduced = useReducedMotion();
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);
  return mounted && Boolean(reduced);
}

/** Fades and lifts its children into place the first time they scroll into view. */
export function Reveal({
  children,
  delay = 0,
  className,
}: {
  children: ReactNode;
  delay?: number;
  className?: string;
}) {
  return (
    <motion.div
      className={className}
      initial={{ opacity: 0, y: 18 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, amount: 0.2 }}
      transition={{ duration: 0.7, ease, delay }}
    >
      {children}
    </motion.div>
  );
}

/**
 * Drives a diagram through `count` steps while it is on screen: `interval` between steps, `hold` on the last one,
 * then from the top again. With reduced motion the diagram rests on its final, complete step until the reader
 * presses play themselves.
 */
export function useStepper(
  count: number,
  {
    interval = 1300,
    hold = 2800,
  }: { interval?: number | ((step: number) => number); hold?: number } = {},
) {
  const ref = useRef<HTMLDivElement>(null);
  const inView = useInView(ref, { amount: 0.35 });
  const reduced = useReducedMotionSafe();
  const [step, setStep] = useState(0);
  const [paused, setPaused] = useState(false);
  // While the reader points at (or focuses) the diagram, the timer waits without counting as a pause.
  const [hovered, setHovered] = useState(false);
  // Pressing play is an explicit choice, so it overrides the reduced-motion default.
  const [chosen, setChosen] = useState(false);
  const held = paused || (reduced && !chosen);
  // Read through a ref so an inline `interval` function does not restart the timer on every render.
  const timing = useRef({ interval, hold });
  timing.current = { interval, hold };

  useEffect(() => {
    if (reduced && !chosen) setStep(count - 1);
  }, [reduced, chosen, count]);

  useEffect(() => {
    if (held || hovered || !inView) return;
    const last = step >= count - 1;
    const { interval: every, hold: rest } = timing.current;
    const wait = last ? rest : typeof every === 'function' ? every(step) : every;
    const timer = setTimeout(() => setStep(last ? 0 : step + 1), wait);
    return () => clearTimeout(timer);
  }, [step, inView, held, hovered, count]);

  return {
    ref,
    step,
    /** Hold the timer while the reader is looking (pointer over the diagram); pass false to resume. */
    setHovered,
    hovered,
    /** Whether the diagram is on screen; ambient loops should run only then. */
    inView,
    /** Jump to a step and stop autoplay, for when the reader takes over. */
    select(next: number) {
      setPaused(true);
      setStep(next);
    },
    replay() {
      setChosen(true);
      setPaused(false);
      setStep(0);
    },
    /** Pause or resume autoplay where it is (WCAG 2.2.2: moving content can be paused). */
    toggle() {
      if (held) {
        setChosen(true);
        setPaused(false);
      } else {
        setPaused(true);
      }
    },
    paused: held,
  };
}
