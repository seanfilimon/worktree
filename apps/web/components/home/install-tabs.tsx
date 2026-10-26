"use client";

import { useState } from "react";
import { motion } from "motion/react";
import {
  RiAppleFill,
  RiCheckLine,
  RiFileCopyLine,
  RiTerminalBoxLine,
  RiWindowsFill,
} from "react-icons/ri";
import { useCopyButton } from "fumadocs-ui/utils/use-copy-button";
import { cx } from "@/utils/cx";

const RAW =
  "https://raw.githubusercontent.com/seanfilimon/worktree/main/scripts";

export const installs = [
  {
    id: "unix",
    label: "macOS & Linux",
    icon: RiAppleFill,
    command: `curl -fsSL ${RAW}/install.sh | bash`,
    note: "Downloads the latest release for your OS and CPU, verifies its checksum, and installs wt and worktree-bg into ~/.local/bin.",
  },
  {
    id: "windows",
    label: "Windows",
    icon: RiWindowsFill,
    command: `irm ${RAW}/install.ps1 | iex`,
    note: "In PowerShell. Downloads the latest release, verifies its checksum, installs into %LOCALAPPDATA%\\Programs\\worktree\\bin and adds it to your PATH.",
  },
  {
    id: "cargo",
    label: "Cargo",
    icon: RiTerminalBoxLine,
    command:
      "cargo install --locked --git https://github.com/seanfilimon/worktree worktree-cli worktree-bg",
    note: "Any platform with stable Rust: builds both binaries from the main branch into ~/.cargo/bin.",
  },
] as const;

/** One install command per platform, one click to copy. */
export function InstallTabs({ className }: { className?: string }) {
  const [active, setActive] = useState<(typeof installs)[number]["id"]>("unix");
  const install = installs.find((item) => item.id === active)!;
  const [checked, onCopy] = useCopyButton(() =>
    navigator.clipboard.writeText(install.command),
  );

  return (
    <div
      className={cx(
        "overflow-hidden rounded-2xl border border-border-button-default bg-surface-raised shadow-sm",
        className,
      )}
    >
      <div
        role="tablist"
        aria-label="Platform"
        className="flex gap-1 border-b border-separator-border p-1.5"
      >
        {installs.map((item) => {
          const selected = item.id === active;
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={selected}
              onClick={() => setActive(item.id)}
              className={cx(
                "relative isolate inline-flex h-8 items-center gap-2 rounded-lg px-3 text-body-2-medium transition-colors",
                selected
                  ? "text-text-primary"
                  : "text-text-secondary hover:text-text-primary",
              )}
            >
              {selected ? (
                <motion.span
                  layoutId="install-tab"
                  className="absolute inset-0 -z-10 rounded-lg bg-background-secondary-default"
                  transition={{ type: "spring", stiffness: 450, damping: 38 }}
                />
              ) : null}
              <Icon className="size-4" aria-hidden />
              {item.label}
            </button>
          );
        })}
      </div>
      <div className="flex items-center gap-3 px-4 py-4 sm:px-5">
        <code
          data-lenis-prevent-horizontal
          className="min-w-0 flex-1 font-mono text-[0.8125rem] leading-6 break-all text-text-primary"
        >
          <span className="text-text-tertiary select-none">$ </span>
          {install.command}
        </code>
        <button
          type="button"
          onClick={onCopy}
          aria-label={checked ? "Copied" : "Copy the install command"}
          className="inline-flex size-8 shrink-0 items-center justify-center rounded-lg border border-border-button-default bg-background-primary-default text-foreground-icon-secondary shadow-xs transition-colors hover:border-border-button-hover hover:text-foreground-icon-primary"
        >
          {checked ? (
            <RiCheckLine className="size-4" aria-hidden />
          ) : (
            <RiFileCopyLine className="size-4" aria-hidden />
          )}
        </button>
      </div>
      <p className="border-t border-separator-border px-4 py-3 text-caption-1-regular leading-5 text-text-secondary sm:px-5">
        {install.note}
      </p>
    </div>
  );
}
