import type { ReactNode } from "react";
import { buttonStyles } from "@/components/base/buttons/button";
import { cx } from "@/utils/cx";
import type { Icon } from "./cells";

/**
 * A BoardUI button for a mailto: or other-host link. ActionLink routes anything that is not http(s) through
 * Next.js, which is wrong for mail addresses, so those use this plain anchor instead.
 */
export function AnchorAction({
  href,
  variant = "secondary",
  leadingIcon: Leading,
  trailingIcon: Trailing,
  className,
  children,
}: {
  href: string;
  variant?: keyof typeof buttonStyles.variant;
  leadingIcon?: Icon;
  trailingIcon?: Icon;
  className?: string;
  children: ReactNode;
}) {
  const external = href.startsWith("http");
  return (
    <a
      href={href}
      className={cx(
        buttonStyles.base,
        buttonStyles.size.medium,
        buttonStyles.variant[variant],
        "group/action",
        className,
      )}
      {...(external ? { target: "_blank", rel: "noopener noreferrer" } : {})}
    >
      {Leading ? <Leading className={buttonStyles.icon.medium} aria-hidden /> : null}
      <span className={buttonStyles.label.medium}>{children}</span>
      {Trailing ? (
        <Trailing
          className={cx(
            buttonStyles.icon.medium,
            "transition-transform duration-200 group-hover/action:translate-x-0.5",
          )}
          aria-hidden
        />
      ) : null}
    </a>
  );
}
