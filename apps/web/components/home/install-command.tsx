'use client';

import { RiCheckLine, RiFileCopyLine } from 'react-icons/ri';
import { useCopyButton } from 'fumadocs-ui/utils/use-copy-button';
import { buttonStyles } from '@/components/base/buttons/button';
import { cx } from '@/utils/cx';

/** The install command as a BoardUI secondary button: one click copies it. */
export function InstallCommand({
  command = 'bash scripts/install.sh',
  className,
}: {
  command?: string;
  className?: string;
}) {
  const [checked, onClick] = useCopyButton(() => navigator.clipboard.writeText(command));
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={checked ? 'Copied' : `Copy ${command}`}
      className={cx(
        buttonStyles.base,
        buttonStyles.size.medium,
        buttonStyles.variant.secondary,
        'gap-3 px-3 font-mono text-body-2-regular',
        className,
      )}
    >
      <span>
        <span className="text-text-tertiary">$ </span>
        {command}
      </span>
      {checked ? (
        <RiCheckLine className="size-4 text-foreground-icon-primary" aria-hidden />
      ) : (
        <RiFileCopyLine className="size-4 text-foreground-icon-secondary" aria-hidden />
      )}
    </button>
  );
}
