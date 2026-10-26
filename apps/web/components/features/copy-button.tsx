'use client';

import { RiCheckLine, RiFileCopyLine } from 'react-icons/ri';
import { useCopyButton } from 'fumadocs-ui/utils/use-copy-button';

/** A small title-bar button that copies `text` (for the command frames on the features page). */
export function CopyButton({ text }: { text: string }) {
  const [checked, onClick] = useCopyButton(() => navigator.clipboard.writeText(text));
  const Icon = checked ? RiCheckLine : RiFileCopyLine;
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={checked ? 'Copied' : `Copy ${text}`}
      className="inline-flex h-7 items-center gap-1.5 rounded-lg px-2 text-caption-1-medium text-text-secondary transition-colors hover:bg-background-secondary-default hover:text-text-primary"
    >
      <Icon className="size-3.5" aria-hidden />
      {checked ? 'Copied' : 'Copy'}
    </button>
  );
}
