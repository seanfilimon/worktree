import type { ComponentProps, ReactNode } from 'react';
import { cx } from '@/utils/cx';
import { DrawLine, FadeUp, WordReveal } from './motion-text';

/**
 * Layout primitives for the marketing pages. Every band sits inside the same centered frame, whose two vertical
 * hairlines (the rails) run from the header to the footer; bands are separated by full-width hairlines, and a small
 * crosshair marks each place a rail meets one. Content inside uses `gutter`, so left edges line up everywhere.
 */
export const gutter = 'px-5 sm:px-8 lg:px-12';

export function Frame({ className, children, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cx(
        'relative mx-auto w-full max-w-site border-x border-separator-border sm:w-[calc(100%-2.5rem)]',
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
}

export function Band({
  id,
  className,
  frameClassName,
  marks = true,
  children,
  ...props
}: Omit<ComponentProps<'section'>, 'children'> & {
  frameClassName?: string;
  /** Crosshairs where the rails meet this band's bottom hairline. */
  marks?: boolean;
  children: ReactNode;
}) {
  return (
    <section
      id={id}
      className={cx('relative scroll-mt-28 border-b border-separator-border', className)}
      {...props}
    >
      <Frame className={frameClassName}>
        {children}
        {marks ? (
          <>
            <Crosshair className="-bottom-[6px] -left-[6px]" />
            <Crosshair className="-right-[6px] -bottom-[6px]" />
          </>
        ) : null}
      </Frame>
    </section>
  );
}

/** An 11px plus sign whose center pixel sits exactly on a rail/hairline intersection. */
export function Crosshair({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cx('pointer-events-none absolute z-10 hidden size-[11px] sm:block', className)}
    >
      <span className="absolute top-0 left-[5px] h-full w-px bg-text-tertiary" />
      <span className="absolute top-[5px] left-0 h-px w-full bg-text-tertiary" />
    </span>
  );
}

/** Section number, eyebrow label, heading, and lede, in the reading order every chapter uses. */
export function SectionHeading({
  index,
  eyebrow,
  title,
  children,
  className,
}: {
  index?: string;
  eyebrow: string;
  title: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <header className={cx('flex max-w-2xl flex-col gap-4', className)}>
      <p className="eyebrow flex items-center gap-2">
        {index ? <span className="text-text-primary">{index}</span> : null}
        {index ? <DrawLine className="h-px w-6 bg-text-primary" delay={0.1} /> : null}
        {eyebrow}
      </p>
      <h2 className="text-display-4-semibold tracking-[-0.025em] text-balance md:text-display-3-semibold md:leading-[1.12]">
        <WordReveal text={title} />
      </h2>
      {children ? (
        <FadeUp delay={0.25}>
          <p className="text-headline-regular leading-7 text-pretty text-text-secondary md:text-[1.0625rem]">
            {children}
          </p>
        </FadeUp>
      ) : null}
    </header>
  );
}
