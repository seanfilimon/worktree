import Link from "next/link";
import { RiArrowLeftLine, RiArrowRightLine } from "react-icons/ri";
import { Band } from "@/components/site/frame";
import { cx } from "@/utils/cx";
import { HoverHatch } from "./content-grid";

export type PagerLink = {
  href: string;
  title: string;
  /** A short line under the title. */
  detail?: string;
};

/**
 * The band after long-form content: previous on the left, next on the right, as two hairline-separated cells.
 * A missing side becomes a quiet hatched cell so the rails stay balanced.
 */
export function PagerBand({
  previous,
  next,
  previousLabel = "Previous",
  nextLabel = "Next",
}: {
  previous?: PagerLink;
  next?: PagerLink;
  previousLabel?: string;
  nextLabel?: string;
}) {
  if (!previous && !next) return null;
  return (
    <Band>
      <nav aria-label="More pages" className="grid gap-px bg-separator-border sm:grid-cols-2">
        <PagerCell link={previous} label={previousLabel} direction="previous" />
        <PagerCell link={next} label={nextLabel} direction="next" />
      </nav>
    </Band>
  );
}

function PagerCell({
  link,
  label,
  direction,
}: {
  link?: PagerLink;
  label: string;
  direction: "previous" | "next";
}) {
  if (!link) {
    return <div aria-hidden className="hatch hidden bg-background-full sm:block" />;
  }
  const Arrow = direction === "previous" ? RiArrowLeftLine : RiArrowRightLine;
  return (
    <Link
      href={link.href}
      rel={direction === "previous" ? "prev" : "next"}
      className={cx(
        "group relative flex flex-col gap-2 overflow-hidden bg-background-full p-6 transition-colors duration-200 hover:bg-background-secondary-default sm:p-8",
        direction === "next" && "sm:items-end sm:text-right",
      )}
    >
      <HoverHatch />
      <span className="eyebrow relative flex items-center gap-1.5">
        {direction === "previous" ? (
          <Arrow
            className="size-3.5 transition-transform duration-300 group-hover:-translate-x-1"
            aria-hidden
          />
        ) : null}
        {label}
        {direction === "next" ? (
          <Arrow
            className="size-3.5 transition-transform duration-300 group-hover:translate-x-1"
            aria-hidden
          />
        ) : null}
      </span>
      <span className="relative text-headline-medium text-balance text-text-primary">
        {link.title}
      </span>
      {link.detail ? (
        <span className="relative line-clamp-2 max-w-md text-body-regular leading-6 text-text-secondary">
          {link.detail}
        </span>
      ) : null}
    </Link>
  );
}
