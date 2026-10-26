import type { ReactNode } from 'react';
import { RiServerLine, RiTerminalBoxLine } from 'react-icons/ri';
import { DiagramFrame } from '@/components/home/frame';
import { cx } from '@/utils/cx';
import { CopyButton } from './copy-button';

const httpMethod = /^(GET|POST|PUT|PATCH|DELETE)\s+/;

/**
 * One command from the features page in a diagram card: a terminal prompt for `wt` commands, or the method and
 * path for a REST call. The body is hatched so a single line still reads as a figure.
 */
export function CommandFrame({
  command,
  caption,
  className,
}: {
  command: string;
  caption?: ReactNode;
  className?: string;
}) {
  const method = command.match(httpMethod)?.[1];
  const rest = method ? command.slice(method.length).trim() : command;
  const LabelIcon = method ? RiServerLine : RiTerminalBoxLine;
  return (
    <DiagramFrame
      className={className}
      label={
        <span className="flex items-center gap-2">
          <LabelIcon className="size-3.5 text-foreground-icon-tertiary" aria-hidden />
          {method ? 'REST API' : 'Terminal'}
        </span>
      }
      actions={<CopyButton text={command} />}
      footer={caption}
      bodyClassName="relative p-0 sm:p-0"
    >
      <div aria-hidden className="hatch pointer-events-none absolute inset-0 opacity-70" />
      <div className="relative flex min-h-36 items-center px-4 py-10 sm:px-6">
        <code
          className={cx(
            'block w-full rounded-xl border border-border-button-default bg-background-full px-4 py-3 font-mono text-body-2-regular break-words shadow-xs',
          )}
        >
          {method ? (
            <span className="me-2 rounded-sm bg-text-primary px-1.5 py-px text-caption-2-semibold text-background-full">
              {method}
            </span>
          ) : (
            <span className="text-text-tertiary select-none">$ </span>
          )}
          <span className="text-text-primary">{rest}</span>
          <span aria-hidden className="ms-1 inline-block h-[1.05em] w-[0.5em] translate-y-[0.15em] animate-pulse bg-text-tertiary" />
        </code>
      </div>
    </DiagramFrame>
  );
}
