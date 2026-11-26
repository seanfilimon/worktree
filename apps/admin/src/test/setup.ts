import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { FakeEventSource } from "./fakeEventSource";

// `findBy*` and `waitFor` give up after 1 s by default, which a busy CI
// runner or laptop regularly exceeds for a whole-app render; the limits
// only bound how long a *failing* test waits.
configure({ asyncUtilTimeout: 5000 });

// ---- jsdom gaps that Radix / cmdk / the shadcn sidebar rely on ----

if (typeof window.matchMedia !== "function") {
  // Desktop, light colour scheme, no reduced motion.
  window.matchMedia = (query: string): MediaQueryList =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    }) as MediaQueryList;
}

// Node 22.4+ defines its own `localStorage`/`sessionStorage` globals, which
// are unusable without `--localstorage-file` and shadow jsdom's (every test
// then fails on `localStorage.getItem`). Install a plain in-memory Storage
// when the one in scope does not work.
class MemoryStorage implements Storage {
  #items = new Map<string, string>();
  get length(): number {
    return this.#items.size;
  }
  clear(): void {
    this.#items.clear();
  }
  getItem(key: string): string | null {
    return this.#items.get(key) ?? null;
  }
  key(index: number): string | null {
    return [...this.#items.keys()][index] ?? null;
  }
  removeItem(key: string): void {
    this.#items.delete(key);
  }
  setItem(key: string, value: string): void {
    this.#items.set(key, String(value));
  }
  [name: string]: unknown;
}
for (const name of ["localStorage", "sessionStorage"] as const) {
  const usable = (() => {
    try {
      return typeof globalThis[name]?.getItem === "function";
    } catch {
      return false;
    }
  })();
  if (!usable) {
    Object.defineProperty(globalThis, name, { value: new MemoryStorage(), configurable: true, writable: true });
  }
}

if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  };
}

const proto: Partial<
  Pick<Element, "scrollIntoView" | "hasPointerCapture" | "setPointerCapture" | "releasePointerCapture">
> = Element.prototype;
proto.scrollIntoView ??= () => undefined;
proto.hasPointerCapture ??= () => false;
proto.setPointerCapture ??= () => undefined;
proto.releasePointerCapture ??= () => undefined;

beforeEach(() => {
  FakeEventSource.reset();
  vi.stubGlobal("EventSource", FakeEventSource);
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  // The shadcn sidebar persists its collapsed state in a cookie.
  document.cookie = "sidebar_state=; max-age=0; path=/";
  document.documentElement.classList.remove("dark");
  delete document.documentElement.dataset.theme;
});
