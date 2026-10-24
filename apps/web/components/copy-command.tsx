"use client";

import * as React from "react";
import { IoCheckmark, IoCopy } from "react-icons/io5";
import { cn } from "@/lib/utils";

const RESET_DELAY_MS = 2000;

/**
 * A shell command rendered as code with an accessible copy-to-clipboard button.
 * Multi-line commands are preserved verbatim.
 */
export function CopyCommand({
  command,
  className,
  prompt = false,
}: {
  command: string;
  className?: string;
  /** Prefix each line with a `$` prompt (visual only, not copied). */
  prompt?: boolean;
}) {
  const [copied, setCopied] = React.useState(false);

  React.useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), RESET_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
    } catch {
      // Clipboard access can be denied (insecure context, permissions); the
      // command stays visible and selectable, so there is nothing to recover.
    }
  };

  return (
    <div
      className={cn(
        "relative rounded-lg bg-muted p-4 pr-12 font-mono text-sm text-left",
        className,
      )}
    >
      <pre className="overflow-x-auto whitespace-pre">
        <code>
          {command.split("\n").map((line, index) => (
            <React.Fragment key={index}>
              {index > 0 && "\n"}
              {prompt && (
                <span className="select-none text-muted-foreground">$ </span>
              )}
              {line}
            </React.Fragment>
          ))}
        </code>
      </pre>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? "Copied" : "Copy command"}
        className="absolute right-2 top-2 inline-flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-background hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {copied ? (
          <IoCheckmark className="h-4 w-4" aria-hidden="true" />
        ) : (
          <IoCopy className="h-4 w-4" aria-hidden="true" />
        )}
      </button>
      <span className="sr-only" aria-live="polite">
        {copied ? "Command copied to clipboard" : ""}
      </span>
    </div>
  );
}
