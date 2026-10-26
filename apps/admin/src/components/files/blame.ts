import type { BlameLine } from "@/api/v06";

/** Index of each line that starts a run of lines attributed to the same snapshot. */
export function blameRunStarts(lines: readonly BlameLine[]): Set<number> {
  const starts = new Set<number>();
  lines.forEach((l, i) => {
    if (i === 0 || lines[i - 1]!.snapshot_id !== l.snapshot_id) starts.add(i);
  });
  return starts;
}

const AGE_UNITS: [suffix: string, seconds: number][] = [
  ["y", 365 * 86_400],
  ["mo", 30 * 86_400],
  ["w", 7 * 86_400],
  ["d", 86_400],
  ["h", 3_600],
  ["m", 60],
];

/**
 * A short age ("now", "5m", "3h", "2d", "4w", "3mo", "1y") for the narrow
 * blame gutter (hardening §15.10); "—" for a missing or unparsable time.
 */
export function compactAge(iso: string | undefined, now: number = Date.now()): string {
  if (!iso) return "—";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "—";
  const secs = Math.max(0, Math.round((now - t) / 1000));
  for (const [suffix, unit] of AGE_UNITS) {
    if (secs >= unit) return `${Math.floor(secs / unit)}${suffix}`;
  }
  return "now";
}
