import { beforeAll } from "vitest";

/**
 * jsdom lacks a few APIs Radix (Select pointer capture, scrolling the
 * selected item into view) and Recharts (ResizeObserver) use. Call at the top
 * of a test file.
 */
export function installDomPolyfills(): void {
  beforeAll(() => {
    const proto = Element.prototype as unknown as Record<string, unknown>;
    proto.hasPointerCapture ??= () => false;
    proto.setPointerCapture ??= () => undefined;
    proto.releasePointerCapture ??= () => undefined;
    proto.scrollIntoView ??= () => undefined;
    globalThis.ResizeObserver ??= class {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
  });
}
