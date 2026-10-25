import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { FakeEventSource } from "./fakeEventSource";

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

if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  };
}

const proto: Partial<Pick<Element, "scrollIntoView" | "hasPointerCapture" | "releasePointerCapture">> =
  Element.prototype;
proto.scrollIntoView ??= () => undefined;
proto.hasPointerCapture ??= () => false;
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
