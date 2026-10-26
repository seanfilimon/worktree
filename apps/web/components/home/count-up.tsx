'use client';

import { useEffect, useRef, useState } from 'react';
import { animate, useInView, useReducedMotion } from 'motion/react';
import { ease } from './motion';

/** Counts up to `value` the first time it scrolls into view. The server renders the final number. */
export function CountUp({ value, className }: { value: number; className?: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const inView = useInView(ref, { once: true, amount: 0.8 });
  const reduced = useReducedMotion();
  const [display, setDisplay] = useState(value);
  const started = useRef(false);

  // Below the fold on load: start from zero so the count is visible when it arrives.
  useEffect(() => {
    if (
      !reduced &&
      !started.current &&
      ref.current &&
      ref.current.getBoundingClientRect().top > window.innerHeight
    ) {
      setDisplay(0);
    }
  }, [reduced]);

  useEffect(() => {
    if (!inView || reduced || started.current) return;
    started.current = true;
    const controls = animate(0, value, {
      duration: 1.4,
      ease,
      onUpdate: (latest) => setDisplay(Math.round(latest)),
    });
    return () => controls.stop();
  }, [inView, reduced, value]);

  return (
    <span ref={ref} className={className}>
      {display}
    </span>
  );
}
