'use client';

import { RiSearchLine } from 'react-icons/ri';
import { useSearchContext } from 'fumadocs-ui/contexts/search';
import { Kbd } from '@/components/base/kbd/kbd';
import { cx } from '@/utils/cx';

/**
 * Opens the docs search dialog. Styled as a BoardUI input (secondary surface, hairline border, shortcut hint) so
 * it reads as a field; used in the marketing header and, through Fumadocs' searchTrigger slot, in the docs.
 */
export function SearchButton({
  className,
  label = 'Search docs',
}: {
  className?: string;
  label?: string;
}) {
  const { setOpenSearch, hotKey } = useSearchContext();
  return (
    <button
      type="button"
      data-search-full=""
      onClick={() => setOpenSearch(true)}
      className={cx(
        'group inline-flex h-8 items-center gap-2 rounded-lg border border-border-button-default bg-background-primary-default ps-2.5 pe-1.5 text-body-regular text-text-placeholder shadow-xs',
        'transition-[background-color,border-color,color] duration-150 hover:border-border-button-hover hover:text-text-secondary',
        'outline-none focus-visible:ring-2 focus-visible:ring-border-focus-ring',
        className,
      )}
    >
      <RiSearchLine className="size-4 shrink-0 text-foreground-icon-secondary" aria-hidden />
      <span className="me-auto truncate">{label}</span>
      <span className="ms-3 inline-flex gap-0.5">
        {hotKey.map((key, index) => (
          <Kbd key={index} className="min-w-5 bg-background-secondary-default text-text-secondary">
            {key.display}
          </Kbd>
        ))}
      </span>
    </button>
  );
}

/** Icon-only variant for narrow toolbars. */
export function SearchIconButton({ className }: { className?: string }) {
  const { setOpenSearch } = useSearchContext();
  return (
    <button
      type="button"
      data-search=""
      aria-label="Search docs"
      onClick={() => setOpenSearch(true)}
      className={cx(
        'inline-flex size-8 items-center justify-center rounded-lg text-foreground-icon-secondary transition-colors hover:bg-background-secondary-default hover:text-foreground-icon-primary',
        'outline-none focus-visible:ring-2 focus-visible:ring-border-focus-ring',
        className,
      )}
    >
      <RiSearchLine className="size-[18px]" aria-hidden />
    </button>
  );
}
