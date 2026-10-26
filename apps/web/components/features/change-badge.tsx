import { cx } from '@/utils/cx';

export type ChangeType = 'Added' | 'Changed' | 'Fixed' | 'Breaking';

/** Breaking changes first, then the rest in the order a reader upgrades through them. */
export const changeOrder: ChangeType[] = ['Breaking', 'Added', 'Changed', 'Fixed'];

/**
 * The four change kinds, told apart without color: solid ink for breaking changes, an outline for additions, a
 * tinted fill for changes, and hatching for fixes.
 */
const styles: Record<ChangeType, string> = {
  Breaking: 'border-text-primary bg-text-primary text-background-full',
  Added: 'border-border-button-default bg-background-full text-text-primary',
  Changed: 'border-transparent bg-background-secondary-default text-text-secondary',
  Fixed: 'hatch border-border-button-default bg-background-full text-text-primary',
};

export function ChangeBadge({ type, className }: { type: ChangeType; className?: string }) {
  return (
    <span
      className={cx(
        'inline-flex h-5 shrink-0 items-center rounded-md border px-1.5 font-mono text-caption-2-medium tracking-[0.04em] uppercase',
        styles[type],
        className,
      )}
    >
      {type}
    </span>
  );
}
