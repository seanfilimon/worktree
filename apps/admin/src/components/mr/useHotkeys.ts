import { useEffect } from "react";

/** Single-key shortcuts that must not fire while typing or inside a dialog. */
function ignoreTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target.closest('[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]')) return true;
  if (target instanceof HTMLElement && target.isContentEditable) return true;
  return target.matches("input, textarea, select");
}

/**
 * Binds unmodified single-key shortcuts (e.g. `{ a: approve }`) for as long
 * as the calling page is mounted. Undefined handlers are inactive.
 */
export function useHotkeys(bindings: Record<string, (() => void) | undefined>): void {
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.repeat || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
      if (ignoreTarget(e.target)) return;
      const run = bindings[e.key];
      if (!run) return;
      e.preventDefault();
      run();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [bindings]);
}
