import type { ComponentProps, ReactNode } from 'react';
import {
  RiAlertLine,
  RiCheckboxCircleLine,
  RiCloseCircleLine,
  RiInformationLine,
  RiLightbulbLine,
} from 'react-icons/ri';
import { cx } from '@/utils/cx';

type CalloutType = 'info' | 'warn' | 'warning' | 'error' | 'success' | 'idea' | 'tip' | 'note';

const kinds = {
  info: { icon: RiInformationLine, strong: false },
  warning: { icon: RiAlertLine, strong: true },
  error: { icon: RiCloseCircleLine, strong: true },
  success: { icon: RiCheckboxCircleLine, strong: false },
  idea: { icon: RiLightbulbLine, strong: false },
} as const;

function resolve(type: CalloutType): keyof typeof kinds {
  if (type === 'warn') return 'warning';
  if (type === 'tip' || type === 'note') return 'info';
  return type;
}

/**
 * MDX callouts (also produced by `:::note` admonitions) in the monochrome BoardUI style: a sunken card with an
 * icon tile. Warnings and errors earn weight instead of color: a darker edge and a solid ink tile.
 */
export function Callout({
  type = 'info',
  title,
  icon,
  className,
  children,
  ...props
}: Omit<ComponentProps<'div'>, 'title'> & {
  type?: CalloutType;
  title?: ReactNode;
  icon?: ReactNode;
}) {
  const kind = kinds[resolve(type)] ?? kinds.info;
  const Icon = kind.icon;
  return (
    <div
      role="note"
      className={cx(
        'my-5 flex gap-3 rounded-2xl border bg-surface-sunken p-4 text-body-regular leading-6',
        kind.strong ? 'border-text-secondary' : 'border-border-button-default',
        className,
      )}
      {...props}
    >
      <span
        aria-hidden
        className={cx(
          'flex size-7 shrink-0 items-center justify-center rounded-lg border [&_svg]:size-4',
          kind.strong
            ? 'border-text-primary bg-text-primary text-background-full'
            : 'border-border-button-default bg-background-primary-default text-foreground-icon-primary shadow-xs',
        )}
      >
        {icon ?? <Icon />}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-1 pt-0.5">
        {title ? <p className="my-0! text-body-semibold text-text-primary">{title}</p> : null}
        <div className="prose-no-margin text-body-regular leading-6 text-text-secondary empty:hidden [&_a]:text-text-primary">
          {children}
        </div>
      </div>
    </div>
  );
}
