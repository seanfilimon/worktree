import type { ComponentType, ReactNode } from 'react';
import { RiArrowRightLine } from 'react-icons/ri';
import { cx } from '@/utils/cx';

export type Icon = ComponentType<{ className?: string; 'aria-hidden'?: boolean }>;

/**
 * Hairline-grid cells for the features, changelog and roadmap pages (the same construction as the home page's
 * closing sections): the list paints the separator color and leaves 1px gaps, each cell paints the page surface.
 */
export const hairlineGrid = 'grid gap-px bg-separator-border';

/** A grid cell that is a link: full-cell hit area, press feedback, and a hatched hover fill. */
export const cellLink =
  'group relative flex h-full flex-col gap-5 overflow-hidden p-6 transition-colors duration-200 hover:bg-background-secondary-default active:bg-background-secondary-hover sm:p-8';

/** Hatching that fades in while the surrounding `group` is hovered. */
export function HoverHatch() {
  return (
    <span
      aria-hidden
      className="hatch pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-500 group-hover:opacity-100"
    />
  );
}

/** A BoardUI icon tile that inverts to solid ink when its cell is hovered. */
export function IconTile({ icon: TileIcon, className }: { icon: Icon; className?: string }) {
  return (
    <span
      className={cx(
        'flex size-9 shrink-0 items-center justify-center rounded-2lg border border-border-button-default bg-background-primary-default text-foreground-icon-primary shadow-xs transition-[background-color,border-color,color,transform] duration-300 group-hover:-rotate-6 group-hover:border-text-primary group-hover:bg-text-primary group-hover:text-background-full',
        className,
      )}
    >
      <TileIcon className="size-[18px]" aria-hidden />
    </span>
  );
}

export function CellArrow({ className }: { className?: string }) {
  return (
    <RiArrowRightLine
      className={cx(
        'size-4 shrink-0 text-foreground-icon-tertiary transition-[color,transform] duration-300 group-hover:translate-x-1 group-hover:text-foreground-icon-primary',
        className,
      )}
      aria-hidden
    />
  );
}

/** A two-digit chapter or row number, in the mono face. */
export function indexLabel(index: number) {
  return String(index + 1).padStart(2, '0');
}

/** A list item with a short hairline lead, for item lists inside cells. */
export function RuleItem({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <li
      className={cx(
        'flex gap-2.5 text-body-regular leading-6 text-text-secondary',
        className,
      )}
    >
      <span aria-hidden className="mt-[0.7rem] h-px w-2.5 shrink-0 bg-text-tertiary" />
      <span className="min-w-0">{children}</span>
    </li>
  );
}
