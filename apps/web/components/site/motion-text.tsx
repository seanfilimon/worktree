'use client';

import { Fragment, type ReactNode } from 'react';
import { motion } from 'motion/react';
import { cx } from '@/utils/cx';

const ease = [0.16, 1, 0.3, 1] as const;

/**
 * Scroll-triggered text animations for the marketing pages. Each plays once, the first time its text scrolls into
 * view. Under `MotionConfig reducedMotion="user"` (the landing page sets it) the movement is dropped and only the
 * fade remains.
 */

/** Words rise and sharpen one after another. Plain strings only; other nodes render as they are. */
export function WordReveal({
  text,
  className,
  delay = 0,
  stagger = 0.045,
  immediate = false,
}: {
  text: ReactNode;
  className?: string;
  delay?: number;
  stagger?: number;
  /** Play on mount instead of when scrolled into view (for above-the-fold headlines). */
  immediate?: boolean;
}) {
  if (typeof text !== 'string') return <span className={className}>{text}</span>;
  const words = text.split(' ');
  const target = { opacity: 1, y: 0, filter: 'blur(0px)' };
  return (
    <span className={className}>
      {words.map((word, index) => (
        <Fragment key={index}>
          <motion.span
            className="inline-block"
            initial={{ opacity: 0, y: '0.35em', filter: 'blur(6px)' }}
            {...(immediate
              ? { animate: target }
              : { whileInView: target, viewport: { once: true, amount: 0.6 } })}
            transition={{ duration: 0.7, ease, delay: delay + index * stagger }}
          >
            {word}
          </motion.span>
          {index < words.length - 1 ? ' ' : null}
        </Fragment>
      ))}
    </span>
  );
}

/** Fades and lifts a block into place. */
export function FadeUp({
  children,
  className,
  delay = 0,
  y = 14,
}: {
  children: ReactNode;
  className?: string;
  delay?: number;
  y?: number;
}) {
  return (
    <motion.div
      className={className}
      initial={{ opacity: 0, y }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, amount: 0.3 }}
      transition={{ duration: 0.7, ease, delay }}
    >
      {children}
    </motion.div>
  );
}

/** A hairline that draws itself from the left. */
export function DrawLine({ className, delay = 0 }: { className?: string; delay?: number }) {
  return (
    <motion.span
      aria-hidden
      className={cx('block origin-left', className)}
      initial={{ scaleX: 0 }}
      whileInView={{ scaleX: 1 }}
      viewport={{ once: true }}
      transition={{ duration: 0.8, ease, delay }}
    />
  );
}

/** A list whose items rise in one after another. */
export function StaggerList({
  children,
  className,
  delay = 0,
}: {
  children: ReactNode;
  className?: string;
  delay?: number;
}) {
  return (
    <motion.ul
      className={className}
      initial="hidden"
      whileInView="shown"
      viewport={{ once: true, amount: 0.2 }}
      transition={{ staggerChildren: 0.08, delayChildren: delay }}
    >
      {children}
    </motion.ul>
  );
}

export const staggerItem = {
  hidden: { opacity: 0, y: 12 },
  shown: { opacity: 1, y: 0, transition: { duration: 0.6, ease } },
};

/**
 * A block that takes part in the nearest StaggerList's sequence. Use it inside a plain `li` when the item itself
 * paints a grid cell (hairline grids), so only the contents fade and the cell never exposes the grid color.
 */
export function StaggerChild({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <motion.div className={className} variants={staggerItem}>
      {children}
    </motion.div>
  );
}

/** A list item for StaggerList. */
export function StaggerItem({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <motion.li className={className} variants={staggerItem}>
      {children}
    </motion.li>
  );
}
