'use client';

import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { useReducedMotionSafe } from './motion';

const CELL = 56;
const TRAIL = 7;

interface Lit {
  key: number;
  x: number;
  y: number;
}

/**
 * The hero's hairline grid. With a mouse, the square under the pointer fills in and the last few squares fade out
 * behind it, like a trail; squares snap exactly to the grid lines (drawn at 56px, starting 1px before the frame's
 * inner edge, see `.grid-lines`). Touch and reduced-motion visitors see the still grid.
 */
export function HeroGrid() {
  const ref = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotionSafe();
  const [lit, setLit] = useState<Lit[]>([]);
  const last = useRef<string>('');
  const counter = useRef(0);

  useEffect(() => {
    const grid = ref.current;
    const host = grid?.parentElement;
    if (!grid || !host || reduced) return;
    const onMove = (event: PointerEvent) => {
      if (event.pointerType !== 'mouse') return;
      const bounds = grid.getBoundingClientRect();
      const x = Math.floor((event.clientX - bounds.left) / CELL);
      const y = Math.floor((event.clientY - bounds.top) / CELL);
      if (y < 0 || x < 0 || (y + 1) * CELL > bounds.height) return;
      const id = `${x}:${y}`;
      if (id === last.current) return;
      last.current = id;
      // Read the key now: the updater may run after later moves have advanced the counter.
      const key = ++counter.current;
      setLit((current) => [...current.slice(-(TRAIL - 1)), { key, x, y }]);
    };
    const onLeave = () => {
      last.current = '';
      setLit([]);
    };
    host.addEventListener('pointermove', onMove);
    host.addEventListener('pointerleave', onLeave);
    return () => {
      host.removeEventListener('pointermove', onMove);
      host.removeEventListener('pointerleave', onLeave);
    };
  }, [reduced]);

  return (
    <div
      ref={ref}
      aria-hidden
      className="grid-lines pointer-events-none absolute inset-x-0 top-0 h-[34rem]"
    >
      <AnimatePresence>
        {lit.map((cell, index) => (
          <motion.span
            key={cell.key}
            className="absolute bg-background-tertiary-default"
            style={{ left: cell.x * CELL, top: cell.y * CELL, width: CELL - 1, height: CELL - 1 }}
            initial={{ opacity: 0 }}
            animate={{ opacity: (index + 1) / lit.length }}
            exit={{ opacity: 0, transition: { duration: 0.6 } }}
            transition={{ duration: 0.25 }}
          />
        ))}
      </AnimatePresence>
    </div>
  );
}
