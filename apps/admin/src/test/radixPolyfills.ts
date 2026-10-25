/**
 * Radix Select relies on pointer capture and scrollIntoView, which jsdom does
 * not implement. Call from `beforeAll` in tests that open a Select.
 */
export function polyfillRadixSelect(): void {
  const proto = Element.prototype;
  if (!("hasPointerCapture" in proto)) Object.assign(proto, { hasPointerCapture: () => false });
  if (!("releasePointerCapture" in proto)) Object.assign(proto, { releasePointerCapture: () => undefined });
  if (!("scrollIntoView" in proto)) Object.assign(proto, { scrollIntoView: () => undefined });
}
