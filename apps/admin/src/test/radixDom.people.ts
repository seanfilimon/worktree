/**
 * jsdom lacks a few DOM APIs Radix primitives call (Select, Switch,
 * DropdownMenu, Checkbox). Import this module before rendering them.
 */
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = ResizeObserverStub;
}

const shims: Record<string, () => unknown> = {
  hasPointerCapture: () => false,
  setPointerCapture: () => undefined,
  releasePointerCapture: () => undefined,
  scrollIntoView: () => undefined,
};

for (const [name, fn] of Object.entries(shims)) {
  if (!(name in Element.prototype)) {
    Object.defineProperty(Element.prototype, name, { value: fn, configurable: true, writable: true });
  }
}
