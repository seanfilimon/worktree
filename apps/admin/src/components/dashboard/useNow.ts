import { useCallback, useSyncExternalStore } from "react";

interface Ticker {
  now: number;
  listeners: Set<() => void>;
  timer: ReturnType<typeof setInterval> | null;
}

/** One shared interval per period, however many components subscribe. */
const tickers = new Map<number, Ticker>();

function ticker(intervalMs: number): Ticker {
  let t = tickers.get(intervalMs);
  if (!t) {
    t = { now: Date.now(), listeners: new Set(), timer: null };
    tickers.set(intervalMs, t);
  }
  return t;
}

function subscribe(intervalMs: number, listener: () => void): () => void {
  const t = ticker(intervalMs);
  t.listeners.add(listener);
  if (t.timer === null) {
    t.now = Date.now();
    t.timer = setInterval(() => {
      t.now = Date.now();
      for (const l of t.listeners) l();
    }, intervalMs);
  }
  return () => {
    t.listeners.delete(listener);
    if (t.listeners.size === 0) {
      if (t.timer !== null) clearInterval(t.timer);
      tickers.delete(intervalMs);
    }
  };
}

/** The current time (ms), re-rendering every `intervalMs`. */
export function useNow(intervalMs: number): number {
  const sub = useCallback((listener: () => void) => subscribe(intervalMs, listener), [intervalMs]);
  const get = useCallback(() => ticker(intervalMs).now, [intervalMs]);
  return useSyncExternalStore(sub, get);
}
