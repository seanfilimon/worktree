import type { ChartConfig } from "@/components/ui/chart";

/** Greyscale ramp step (`--chart-1` darkest/most prominent … `--chart-5`). */
export type ChartTone = 1 | 2 | 3 | 4 | 5;

/** One plotted value column of a monochrome chart. */
export interface ChartSeries {
  /** Data key; also names the `--color-<key>` CSS variable. */
  key: string;
  label: string;
  /** Defaults to the series' position (1-based) in the list. */
  tone?: ChartTone;
  /** Dashed stroke: tells series apart without relying on shade alone. */
  dashed?: boolean;
}

/** The CSS color of a greyscale ramp step. */
export function toneColor(tone: ChartTone): string {
  return `var(--chart-${tone})`;
}

/** shadcn ChartConfig for a list of series. */
export function seriesConfig(series: readonly ChartSeries[]): ChartConfig {
  return Object.fromEntries(
    series.map((s, i) => [s.key, { label: s.label, color: toneColor(s.tone ?? (Math.min(i + 1, 5) as ChartTone)) }]),
  );
}

/** Stroke dash pattern for dashed series. */
export const DASH = "4 3";
