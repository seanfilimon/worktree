"use client";

import { useState, useMemo } from "react";
import { useRouter } from "next/navigation";
import { ChevronDown, Check, Tag } from "lucide-react";

export interface Version {
  /** Display name, e.g. "v0.1.0-alpha" */
  name: string;
  /** URL prefix or full URL to navigate to when selected */
  url?: string;
  /** Whether this is the currently active/default version */
  active?: boolean;
  /** Optional label like "latest", "beta", "deprecated" */
  label?: string;
}

interface VersionSelectorProps {
  versions: Version[];
}

export function VersionSelector({ versions }: VersionSelectorProps) {
  const [open, setOpen] = useState(false);
  const router = useRouter();

  const selected = useMemo(() => {
    return versions.find((v) => v.active) ?? versions[0];
  }, [versions]);

  if (versions.length === 0) return null;

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-2 rounded-lg border bg-fd-secondary/50 p-2 text-start text-fd-secondary-foreground transition-colors hover:bg-fd-accent data-[state=open]:bg-fd-accent data-[state=open]:text-fd-accent-foreground"
        data-state={open ? "open" : "closed"}
      >
        <div className="flex size-5 shrink-0 items-center justify-center text-fd-muted-foreground">
          <Tag className="size-3.5" />
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium truncate">{selected.name}</p>
        </div>
        {selected.label && (
          <span className="shrink-0 rounded-md bg-fd-primary/10 px-1.5 py-0.5 text-[0.6875rem] font-medium text-fd-primary">
            {selected.label}
          </span>
        )}
        <ChevronDown
          className={`shrink-0 size-4 text-fd-muted-foreground transition-transform ${
            open ? "rotate-180" : ""
          }`}
        />
      </button>

      {open && (
        <>
          {/* Backdrop to close on outside click */}
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div className="absolute left-0 right-0 top-full z-50 mt-1 flex flex-col gap-0.5 rounded-lg border bg-fd-popover p-1 shadow-md">
            {versions.map((version) => {
              const isActive = version.name === selected.name;
              return (
                <button
                  key={version.name}
                  type="button"
                  onClick={() => {
                    setOpen(false);
                    if (version.url && !isActive) {
                      router.push(version.url);
                    }
                  }}
                  className="flex items-center gap-2 rounded-md p-1.5 text-start hover:bg-fd-accent hover:text-fd-accent-foreground transition-colors"
                >
                  <div className="flex size-5 shrink-0 items-center justify-center text-fd-muted-foreground">
                    <Tag className="size-3" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium leading-none truncate">
                      {version.name}
                    </p>
                  </div>
                  {version.label && (
                    <span className="shrink-0 rounded-md bg-fd-primary/10 px-1.5 py-0.5 text-[0.6875rem] font-medium text-fd-primary">
                      {version.label}
                    </span>
                  )}
                  <Check
                    className={`shrink-0 size-3.5 text-fd-primary ${
                      isActive ? "" : "invisible"
                    }`}
                  />
                </button>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
