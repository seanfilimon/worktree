'use client';

import { useSearchContext } from 'fumadocs-ui/contexts/search';
import { SearchButton, SearchIconButton } from '@/components/site/search-button';
import { cx } from '@/utils/cx';

/**
 * Fumadocs layout slots rendered with BoardUI styling. The docs sidebar's full-width trigger and the mobile
 * header's icon trigger open the same search dialog as the marketing header.
 */
export function DocsSearchFull({
  className,
  hideIfDisabled,
}: {
  className?: string;
  hideIfDisabled?: boolean;
}) {
  const { enabled } = useSearchContext();
  if (hideIfDisabled && !enabled) return null;
  return <SearchButton className={cx('h-9 w-full', className)} label="Search" />;
}

export function DocsSearchSm({
  className,
  hideIfDisabled,
}: {
  className?: string;
  hideIfDisabled?: boolean;
}) {
  const { enabled } = useSearchContext();
  if (hideIfDisabled && !enabled) return null;
  return <SearchIconButton className={className} />;
}
