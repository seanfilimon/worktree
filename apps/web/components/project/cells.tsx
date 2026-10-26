import type { ComponentType, ReactNode } from "react";
import Link from "next/link";
import { RiArrowRightLine, RiArrowRightUpLine } from "react-icons/ri";
import { StaggerChild, StaggerList } from "@/components/site/motion-text";
import { cx } from "@/utils/cx";

export type Icon = ComponentType<{ className?: string; "aria-hidden"?: boolean }>;

/**
 * Hairline grids for the project pages (about, community, contact, contributing, security): cells separated by
 * 1px of the grid's own background, a full-cell hit area, an icon tile that inverts on hover, and a hatched fill.
 * The same construction as the closing sections of the home page.
 */
export const cellLink =
  "group relative flex h-full flex-col gap-2 overflow-hidden p-6 transition-colors duration-200 hover:bg-background-secondary-default active:bg-background-secondary-hover sm:p-8";

/** A static cell: same padding as a linked cell, no hover. */
export const cellStatic = "relative flex h-full flex-col gap-2 p-6 sm:p-8";

/** Links that leave the site (other hosts, mail) are plain anchors; external hosts open in a new tab. */
export function isPlainHref(href: string) {
  return /^(https?:|mailto:)/.test(href);
}

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
        "flex size-9 shrink-0 items-center justify-center rounded-2lg border border-border-button-default bg-background-primary-default text-foreground-icon-primary shadow-xs transition-[background-color,border-color,color,transform] duration-300 group-hover:-rotate-6 group-hover:border-text-primary group-hover:bg-text-primary group-hover:text-background-full",
        className,
      )}
    >
      <TileIcon className="size-[18px]" aria-hidden />
    </span>
  );
}

/** The arrow at the end of a cell's title: diagonal for other hosts, straight for pages on this site. */
export function CellArrow({ external = false, className }: { external?: boolean; className?: string }) {
  const Arrow = external ? RiArrowRightUpLine : RiArrowRightLine;
  return (
    <Arrow
      className={cx(
        "size-4 shrink-0 text-foreground-icon-tertiary transition-[color,transform] duration-300 group-hover:text-foreground-icon-primary",
        external
          ? "group-hover:translate-x-0.5 group-hover:-translate-y-0.5"
          : "group-hover:translate-x-1",
        className,
      )}
      aria-hidden
    />
  );
}

/** A whole-cell link: Next.js routing for pages on this site, a plain anchor for anything else. */
export function CellAnchor({
  href,
  className,
  children,
}: {
  href: string;
  className?: string;
  children: ReactNode;
}) {
  const classes = cx(cellLink, className);
  if (isPlainHref(href)) {
    const external = href.startsWith("http");
    return (
      <a
        href={href}
        className={classes}
        {...(external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
      >
        {children}
      </a>
    );
  }
  return (
    <Link href={href} className={classes}>
      {children}
    </Link>
  );
}

export interface LinkCellItem {
  icon?: Icon;
  title: string;
  body: ReactNode;
  href: string;
  /** A short label under the body, such as the address the link opens (set in the mono face). */
  label?: string;
}

/**
 * A channel or resource grid: icon tile, title with arrow, one line, and an optional address. `columns` sets the
 * widest layout; the grid steps down to one column on phones.
 */
export function LinkGrid({
  items,
  columns = 3,
  className,
  cellClassName = "bg-background-full",
}: {
  items: LinkCellItem[];
  columns?: 2 | 3 | 4;
  className?: string;
  cellClassName?: string;
}) {
  return (
    <StaggerList
      className={cx(
        "grid gap-px border-t border-separator-border bg-separator-border",
        columns === 2 && "md:grid-cols-2",
        columns === 3 && "md:grid-cols-3",
        columns === 4 && "sm:grid-cols-2 lg:grid-cols-4",
        className,
      )}
    >
      {items.map(({ icon, ...item }) => {
        const external = item.href.startsWith("http");
        return (
          <li key={item.title} className={cellClassName}>
            <StaggerChild className="h-full">
              <CellAnchor href={item.href}>
                <HoverHatch />
                {icon ? <IconTile icon={icon} className="relative mb-3" /> : null}
                <span className="relative flex items-center gap-1.5 text-headline-medium">
                  {item.title}
                  <CellArrow external={external} />
                </span>
                <span className="relative text-body-regular leading-6 text-pretty text-text-secondary">
                  {item.body}
                </span>
                {item.label ? (
                  <span
                    className={cx(
                      "relative mt-auto pt-4 text-text-secondary transition-colors duration-300 group-hover:text-text-primary",
                      // Addresses (no spaces) in the mono face, action phrases in the body face.
                      /\s/.test(item.label) ? "text-body-2-medium" : "font-mono text-caption-1-regular",
                    )}
                  >
                    {item.label}
                  </span>
                ) : null}
              </CellAnchor>
            </StaggerChild>
          </li>
        );
      })}
    </StaggerList>
  );
}

/** The "01" style index used on numbered cells and steps. */
export function StepNumber({ value, className }: { value: number; className?: string }) {
  return (
    <span className={cx("font-mono text-caption-1-regular text-text-tertiary tabular-nums", className)}>
      {String(value).padStart(2, "0")}
    </span>
  );
}
