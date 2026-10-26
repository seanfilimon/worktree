import type { ReactNode } from "react";
import Link from "next/link";
import { RiArrowRightLine } from "react-icons/ri";
import { gutter } from "@/components/site/frame";
import { StaggerChild, StaggerList } from "@/components/site/motion-text";
import { cx } from "@/utils/cx";

/**
 * Index lists for guides, articles, and maintainers: a hairline grid (the grid's background shows through 1px
 * gaps) whose cells are whole-card links with a hatched hover fill and an arrow that slides on hover.
 */
export function ContentGrid({
  children,
  count,
  className,
}: {
  children: ReactNode;
  /** The number of cells, so the last row can be completed with hatched filler instead of exposing the grid. */
  count: number;
  className?: string;
}) {
  // Two columns from `sm`, three from `lg`: each breakpoint gets its own fillers.
  const twoUp = count % 2;
  const threeUp = (3 - (count % 3)) % 3;
  return (
    <StaggerList
      className={cx("grid gap-px bg-separator-border sm:grid-cols-2 lg:grid-cols-3", className)}
    >
      {children}
      {twoUp ? <li aria-hidden className="hatch hidden bg-background-full sm:block lg:hidden" /> : null}
      {Array.from({ length: threeUp }, (_, index) => (
        <li key={index} aria-hidden className="hatch hidden bg-background-full lg:block" />
      ))}
    </StaggerList>
  );
}

/** One grid cell. Every visible line is optional except the title. */
export function ContentCell({
  href,
  label,
  title,
  summary,
  tags,
  footer,
  media,
  className,
}: {
  href: string;
  /** A mono line above the title: index, date, section. */
  label?: ReactNode;
  title: ReactNode;
  summary?: ReactNode;
  tags?: string[];
  /** A small line pinned to the bottom of the cell (author, handle, reading time). */
  footer?: ReactNode;
  /** Something above the label, such as an avatar. */
  media?: ReactNode;
  className?: string;
}) {
  return (
    <li className={cx("bg-background-full", className)}>
      <StaggerChild className="h-full">
        <Link href={href} className={cellLink}>
          <HoverHatch />
          {media ? <div className="relative">{media}</div> : null}
          {label ? (
            <p className="relative flex flex-wrap items-center gap-x-2 gap-y-1 font-mono text-caption-1-regular text-text-tertiary">
              {label}
            </p>
          ) : null}
          <h3 className="relative flex items-start justify-between gap-3 text-headline-medium text-balance text-text-primary">
            <span>{title}</span>
            <CellArrow className="mt-1" />
          </h3>
          {summary ? (
            <p className="relative line-clamp-3 text-body-regular leading-6 text-pretty text-text-secondary">
              {summary}
            </p>
          ) : null}
          {tags?.length || footer ? (
            <div className="relative mt-auto flex flex-col gap-3 pt-3">
              {tags?.length ? <TagChips tags={tags} /> : null}
              {footer ? (
                <p className="text-caption-1-regular text-text-secondary">{footer}</p>
              ) : null}
            </div>
          ) : null}
        </Link>
      </StaggerChild>
    </li>
  );
}

/** Grid cell links: full-cell hit area, press feedback, and room for the hatched hover fill. */
export const cellLink =
  "group relative flex h-full flex-col gap-2.5 overflow-hidden p-6 transition-colors duration-200 hover:bg-background-secondary-default active:bg-background-secondary-hover sm:p-8";

/** Diagonal hatching that fades in behind a hovered cell. */
export function HoverHatch() {
  return (
    <span
      aria-hidden
      className="hatch pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-500 group-hover:opacity-100"
    />
  );
}

/** The arrow at the end of a cell's title: tertiary at rest, ink and nudged right on hover. */
export function CellArrow({ className }: { className?: string }) {
  return (
    <RiArrowRightLine
      aria-hidden
      className={cx(
        "size-4 shrink-0 text-foreground-icon-tertiary transition-[color,transform] duration-300 group-hover:translate-x-1 group-hover:text-foreground-icon-primary",
        className,
      )}
    />
  );
}

/** Tags as small monochrome mono chips. */
export function TagChips({ tags, className }: { tags: string[]; className?: string }) {
  return (
    <ul className={cx("flex flex-wrap gap-1.5", className)} aria-label="Tags">
      {tags.map((tag) => (
        <li
          key={tag}
          className="rounded-md border border-separator-border bg-background-full px-1.5 py-0.5 font-mono text-caption-2-regular text-text-secondary"
        >
          {tag}
        </li>
      ))}
    </ul>
  );
}

/** A section label row above a grid: mono index, label, and a count on the right. */
export function GridHeading({
  index,
  label,
  count,
  id,
}: {
  index?: string;
  label: string;
  count?: ReactNode;
  id?: string;
}) {
  return (
    <div
      id={id}
      className={cx("flex items-center justify-between gap-4 border-b border-separator-border py-4", gutter)}
    >
      <h2 className="eyebrow flex items-center gap-2">
        {index ? <span className="text-text-primary">{index}</span> : null}
        {index ? <span aria-hidden className="h-px w-6 bg-text-primary" /> : null}
        {label}
      </h2>
      {count ? (
        <span className="font-mono text-caption-1-regular text-text-tertiary">{count}</span>
      ) : null}
    </div>
  );
}
