import { useState } from "react";

export interface Sample<T> {
  /** When the sample was taken (ms since epoch). */
  t: number;
  value: T;
}

/**
 * Keeps the last `max` values of a polled query in memory, appending one
 * sample whenever `updatedAt` (e.g. TanStack Query's `dataUpdatedAt`)
 * changes. History lives as long as the component.
 */
export function useSampledHistory<T>(value: T | undefined, updatedAt: number, max = 60): Sample<T>[] {
  const [history, setHistory] = useState<{ at: number; samples: Sample<T>[] }>({ at: 0, samples: [] });
  if (value !== undefined && updatedAt > 0 && updatedAt !== history.at) {
    // Adjusting state while rendering (instead of in an effect) avoids an extra
    // commit with stale history: https://react.dev/learn/you-might-not-need-an-effect
    const next = { at: updatedAt, samples: [...history.samples, { t: updatedAt, value }].slice(-max) };
    setHistory(next);
    return next.samples;
  }
  return history.samples;
}
