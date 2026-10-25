export interface LineRange {
  start: number;
  end: number;
}

/** Parses "#L10" or "#L10-L20" (either order); null for anything else. */
export function parseLineHash(hash: string): LineRange | null {
  const m = /^#?L(\d+)(?:-L?(\d+))?$/.exec(hash);
  if (!m) return null;
  const a = Number(m[1]);
  const b = m[2] === undefined ? a : Number(m[2]);
  if (a < 1 || b < 1) return null;
  return { start: Math.min(a, b), end: Math.max(a, b) };
}

/** "L10" or "L10-L20" (without "#"). */
export function lineHash(range: LineRange): string {
  return range.start === range.end ? `L${range.start}` : `L${range.start}-L${range.end}`;
}
