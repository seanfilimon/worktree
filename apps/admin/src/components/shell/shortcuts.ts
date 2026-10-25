/** True on macOS / iOS, where shortcuts use ⌘ instead of Ctrl. */
export function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  return /mac|iphone|ipad|ipod/i.test(nav.userAgentData?.platform ?? nav.platform ?? "");
}

/** Whether a key event comes from a text field, where "/" must type a slash. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

function insideOverlay(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest('[role="dialog"], [role="alertdialog"], [role="menu"]') !== null;
}

/**
 * ⌘K / Ctrl+K toggles the palette; a bare "/" opens it unless the user is
 * typing or working in another dialog or menu.
 */
export function paletteShortcut(e: KeyboardEvent): "toggle" | "open" | null {
  if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) return "toggle";
  if (e.key === "/" && !e.metaKey && !e.ctrlKey && !e.altKey && !isTypingTarget(e.target) && !insideOverlay(e.target)) {
    return "open";
  }
  return null;
}
