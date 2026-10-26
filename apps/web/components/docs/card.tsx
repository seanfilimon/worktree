import type { HTMLAttributes, ReactNode } from 'react';
import Link from 'fumadocs-core/link';
import { RiArrowRightLine, RiArrowRightUpLine } from 'react-icons/ri';
import { cx } from '@/utils/cx';

/** MDX card grid: two columns, one on narrow containers (same contract as Fumadocs' `Cards`). */
export function Cards({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cx('@container not-prose my-5 grid grid-cols-2 gap-3', className)} {...props} />
  );
}

/**
 * BoardUI card: hairline edge, contact shadow, and an icon tile. Linked cards lift their border on hover and show
 * an arrow (up-right for other sites).
 */
export function Card({
  icon,
  title,
  description,
  href,
  external,
  className,
  children,
  ...props
}: Omit<HTMLAttributes<HTMLElement>, 'title'> & {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  href?: string;
  external?: boolean;
}) {
  const body = (
    <>
      <span className="flex items-start gap-3">
        {icon ? (
          <span
            aria-hidden
            className="flex size-8 shrink-0 items-center justify-center rounded-2lg border border-border-button-default bg-background-primary-default text-foreground-icon-primary shadow-xs [&_svg]:size-4"
          >
            {icon}
          </span>
        ) : null}
        <span className="flex min-w-0 flex-1 flex-col gap-1">
          <span className="flex items-center gap-1.5 text-body-semibold text-text-primary">
            {title}
            {href ? (
              external ? (
                <RiArrowRightUpLine
                  aria-hidden
                  className="size-3.5 text-foreground-icon-tertiary transition-colors group-hover:text-foreground-icon-primary"
                />
              ) : (
                <RiArrowRightLine
                  aria-hidden
                  className="size-3.5 -translate-x-1 text-foreground-icon-tertiary opacity-0 transition-[opacity,transform,color] group-hover:translate-x-0 group-hover:text-foreground-icon-primary group-hover:opacity-100"
                />
              )
            ) : null}
          </span>
          {description ? (
            <span className="text-body-regular leading-6 text-text-secondary">{description}</span>
          ) : null}
          {children ? (
            // The grid is not-prose (so the whole card link isn't underlined); inline code gets its pill back here.
            <span className="text-body-regular leading-6 text-text-secondary empty:hidden [&_code]:rounded-md [&_code]:border [&_code]:border-separator-border [&_code]:bg-background-secondary-default [&_code]:px-1 [&_code]:py-px [&_code]:font-mono [&_code]:text-[0.8125rem] [&_code]:text-text-primary">
              {children}
            </span>
          ) : null}
        </span>
      </span>
    </>
  );
  const classes = cx(
    'group block rounded-2xl border border-border-button-default bg-background-primary-default p-4 shadow-xs @max-lg:col-span-full',
    href &&
      'transition-[background-color,border-color] duration-150 hover:border-border-button-hover hover:bg-background-primary-hover',
    className,
  );
  return href ? (
    <Link href={href} external={external} data-card className={classes} {...props}>
      {body}
    </Link>
  ) : (
    <div data-card className={classes} {...props}>
      {body}
    </div>
  );
}
