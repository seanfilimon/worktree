import type { ComponentProps, ComponentType, ReactNode } from 'react';
import Link from 'next/link';
import { buttonStyles } from '@/components/base/buttons/button';
import { cx } from '@/utils/cx';

type IconComponent = ComponentType<{ className?: string; 'aria-hidden'?: boolean }>;

/**
 * BoardUI's Button, rendered as a Next.js link for in-site navigation (BoardUI's ButtonLink is a plain anchor).
 * Static files and other sites fall back to an ordinary anchor so they are not treated as client routes.
 */
export function ActionLink({
  href,
  variant = 'primary',
  size = 'medium',
  leadingIcon: Leading,
  trailingIcon: Trailing,
  className,
  children,
  ...props
}: Omit<ComponentProps<typeof Link>, 'href'> & {
  href: string;
  variant?: keyof typeof buttonStyles.variant;
  size?: keyof typeof buttonStyles.size;
  leadingIcon?: IconComponent;
  trailingIcon?: IconComponent;
}) {
  const classes = cx(
    buttonStyles.base,
    buttonStyles.size[size],
    buttonStyles.variant[variant],
    'group/action',
    className,
  );
  const content = (
    <>
      {Leading ? <Leading className={buttonStyles.icon[size]} aria-hidden /> : null}
      <span className={buttonStyles.label[size]}>{children}</span>
      {Trailing ? (
        <Trailing
          className={cx(
            buttonStyles.icon[size],
            'transition-transform duration-200 group-hover/action:translate-x-0.5',
          )}
          aria-hidden
        />
      ) : null}
    </>
  );
  if (/^https?:|\.(json|txt)$/.test(href))
    return (
      <a href={href} className={classes} {...(props as ComponentProps<'a'>)}>
        {content}
      </a>
    );
  return (
    <Link href={href} className={classes} {...props}>
      {content}
    </Link>
  );
}

/** An inline "Read more" style link: BoardUI's LinkButton look, routed through Next.js. */
export function TextLink({
  href,
  className,
  children,
  trailingIcon: Trailing,
}: {
  href: string;
  className?: string;
  children: ReactNode;
  trailingIcon?: IconComponent;
}) {
  return (
    <Link
      href={href}
      className={cx(
        'group/text inline-flex w-fit items-center gap-1 rounded-sm text-body-medium text-text-primary underline-offset-4 hover:underline',
        className,
      )}
    >
      {children}
      {Trailing ? (
        <Trailing
          className="size-4 shrink-0 transition-transform duration-200 group-hover/text:translate-x-0.5"
          aria-hidden
        />
      ) : null}
    </Link>
  );
}
