import type { ReactNode } from 'react';
import { RiPauseLine, RiPlayLine, RiRestartLine } from 'react-icons/ri';
import { cx } from '@/utils/cx';

/** The card every landing-page diagram sits in: a quiet title bar, the figure, and an optional footnote. */
export function DiagramFrame({
  label,
  live = false,
  actions,
  footer,
  className,
  bodyClassName,
  children,
}: {
  label: ReactNode;
  live?: boolean;
  actions?: ReactNode;
  footer?: ReactNode;
  className?: string;
  bodyClassName?: string;
  children: ReactNode;
}) {
  return (
    <figure
      className={cx(
        'relative min-w-0 overflow-hidden rounded-2xl border border-border-button-default bg-surface-raised shadow-sm',
        className,
      )}
    >
      <div className="flex h-11 items-center gap-2 border-b border-separator-border px-4 text-caption-1-regular text-text-secondary sm:px-5">
        {live ? <span aria-hidden className="live-dot" /> : null}
        <span className="truncate">{label}</span>
        {actions ? (
          <div className="ms-auto flex shrink-0 items-center gap-1.5">{actions}</div>
        ) : null}
      </div>
      <div className={cx('p-4 sm:p-6', bodyClassName)}>{children}</div>
      {footer ? (
        <figcaption className="border-t border-separator-border px-4 py-3 text-caption-1-regular leading-5 text-text-secondary sm:px-5">
          {footer}
        </figcaption>
      ) : null}
    </figure>
  );
}

const toolbarButton =
  'inline-flex h-7 items-center gap-1.5 rounded-lg px-2 text-caption-1-medium text-text-secondary transition-colors hover:bg-background-secondary-default hover:text-text-primary';

/** Pauses or resumes a diagram that plays by itself. */
export function PlayToggle({
  paused,
  onClick,
  compact = false,
}: {
  paused: boolean;
  onClick: () => void;
  compact?: boolean;
}) {
  const Icon = paused ? RiPlayLine : RiPauseLine;
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={paused ? 'Play the animation' : 'Pause the animation'}
      className={toolbarButton}
    >
      <Icon className="size-3.5" aria-hidden />
      {compact ? null : paused ? 'Play' : 'Pause'}
    </button>
  );
}

export function ReplayButton({
  onClick,
  label = 'Replay',
}: {
  onClick: () => void;
  label?: string;
}) {
  return (
    <button type="button" onClick={onClick} className={toolbarButton}>
      <RiRestartLine className="size-3.5" aria-hidden />
      {label}
    </button>
  );
}

/** A two-to-four option switch whose selected thumb is drawn by the caller (so it can animate between options). */
export function segmentClass(active: boolean) {
  return cx(
    'relative z-0 inline-flex h-8 items-center rounded-md px-3 text-body-2-medium transition-colors',
    active ? 'text-text-primary' : 'text-text-secondary hover:text-text-primary',
  );
}
