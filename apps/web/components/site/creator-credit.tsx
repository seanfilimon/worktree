import { creator } from '@/lib/shared';
import { cx } from '@/utils/cx';

/** Initials in an ink disc, the way BoardUI renders an avatar without a photo. */
export function CreatorMark({ className }: { className?: string }) {
  const initials = creator
    .split(' ')
    .map((part) => part[0])
    .join('');
  return (
    <span
      aria-hidden
      className={cx(
        'flex size-6 shrink-0 items-center justify-center rounded-full bg-text-primary font-mono text-[0.625rem] font-semibold tracking-tight text-background-full',
        className,
      )}
    >
      {initials}
    </span>
  );
}

/** "Created by Sean Filimon", with the initials mark: used in the site footer, hero, docs sidebar, and docs pages. */
export function CreatorCredit({
  className,
  label = 'Created by',
}: {
  className?: string;
  label?: string;
}) {
  return (
    <p
      className={cx(
        'flex w-fit items-center gap-2 text-caption-1-regular text-text-secondary',
        className,
      )}
    >
      <CreatorMark />
      <span>
        {label} <span className="text-caption-1-medium text-text-primary">{creator}</span>
      </span>
    </p>
  );
}
