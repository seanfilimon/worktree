import type { BlameLine } from "@/api/v06";

/** Index of each line that starts a run of lines attributed to the same snapshot. */
export function blameRunStarts(lines: readonly BlameLine[]): Set<number> {
  const starts = new Set<number>();
  lines.forEach((l, i) => {
    if (i === 0 || lines[i - 1]!.snapshot_id !== l.snapshot_id) starts.add(i);
  });
  return starts;
}
