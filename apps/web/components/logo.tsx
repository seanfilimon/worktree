import type { SVGProps } from "react";
import { cx } from "@/utils/cx";

/**
 * W0rkTree mark: a trunk with one branch, three snapshot nodes, drawn on a 24px grid in the current color
 * (the same construction as the Better IAM shield, so both marks sit at the same weight in a header).
 */
export function LogoMark(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      aria-hidden="true"
      {...props}
    >
      <circle cx="7" cy="5" r="2.25" />
      <circle cx="7" cy="19" r="2.25" />
      <circle cx="17" cy="8" r="2.25" />
      <path d="M7 7.25v9.5" />
      <path d="M17 10.25c0 3.6-2.6 5.3-7.6 6.1" />
    </svg>
  );
}

export function Logo({ className }: { className?: string }) {
  return (
    // Block-level flex, not inline-flex: an inline box sits on the parent's text baseline and rides a few pixels high.
    <span
      className={cx(
        "flex w-fit items-center gap-2 text-headline-semibold",
        className,
      )}
    >
      <LogoMark className="size-5 text-text-primary" />
      <span className="tracking-[-0.01em]">
        W0rk<span className="text-text-secondary">Tree</span>
      </span>
    </span>
  );
}
