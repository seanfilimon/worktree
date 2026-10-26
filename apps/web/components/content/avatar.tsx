import Image from "next/image";
import { cx } from "@/utils/cx";
import { initials } from "./format";

const sizes = {
  sm: "size-8 text-[0.6875rem]",
  md: "size-12 text-caption-1-semibold",
  lg: "size-16 text-body-semibold",
  xl: "size-24 text-title-3-semibold",
} as const;

/**
 * A maintainer's photo, or their initials in an ink disc (the same mark as the creator credit) when the profile
 * has no avatar.
 */
export function Avatar({
  name,
  src,
  size = "md",
  className,
}: {
  name: string;
  src?: string;
  size?: keyof typeof sizes;
  className?: string;
}) {
  if (src) {
    return (
      <Image
        src={src}
        alt={name}
        width={96}
        height={96}
        className={cx(
          sizes[size],
          "shrink-0 rounded-full border border-border-button-default object-cover grayscale",
          className,
        )}
      />
    );
  }
  return (
    <span
      aria-hidden
      className={cx(
        sizes[size],
        "flex shrink-0 items-center justify-center rounded-full bg-text-primary font-mono tracking-tight text-background-full",
        className,
      )}
    >
      {initials(name)}
    </span>
  );
}
