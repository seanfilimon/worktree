import type { ReactNode } from "react";
import { Area, AreaChart, CartesianGrid, Line, LineChart, XAxis, YAxis } from "recharts";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { cn } from "@/lib/utils";
import { DASH, seriesConfig, type ChartSeries } from "./series";

export type TrendPoint = Record<string, number | undefined>;

interface TrendChartProps {
  data: readonly TrendPoint[];
  /** Numeric x value (usually a timestamp in ms). */
  xKey: string;
  series: readonly ChartSeries[];
  /** Accessible name of the chart. */
  label: string;
  kind?: "area" | "line";
  /** Stack area series on top of each other. */
  stacked?: boolean;
  formatX?: (x: number) => string;
  formatY?: (y: number) => string;
  /** Shown instead of the chart when `data` has fewer than two points. */
  empty?: ReactNode;
  className?: string;
}

const clock = new Intl.DateTimeFormat("en", { hour: "2-digit", minute: "2-digit" });
const defaultFormatX = (x: number) => clock.format(x);
const defaultFormatY = (y: number) => (Number.isInteger(y) ? String(y) : y.toFixed(1));

/**
 * Monochrome time-series chart (shadcn Chart over recharts): greyscale
 * `--chart-N` tones, dashed strokes for secondary series, no animation so
 * live updates do not flicker.
 */
export function TrendChart({
  data,
  xKey,
  series,
  label,
  kind = "area",
  stacked = false,
  formatX = defaultFormatX,
  formatY = defaultFormatY,
  empty,
  className,
}: TrendChartProps) {
  if (data.length < 2 && empty !== undefined) {
    return (
      <div className={cn("flex h-48 items-center justify-center text-center text-xs text-muted-foreground", className)}>
        {empty}
      </div>
    );
  }

  const config = seriesConfig(series);
  const Chart = kind === "area" ? AreaChart : LineChart;
  const rows = data as TrendPoint[];

  return (
    <ChartContainer config={config} role="img" aria-label={label} className={cn("aspect-auto h-48 w-full", className)}>
      <Chart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: 0 }} accessibilityLayer>
        <CartesianGrid vertical={false} strokeDasharray="2 4" />
        <XAxis
          dataKey={xKey}
          type="number"
          scale="time"
          domain={["dataMin", "dataMax"]}
          tickFormatter={(v: number) => formatX(v)}
          tickLine={false}
          axisLine={false}
          minTickGap={32}
          tickMargin={6}
        />
        <YAxis
          width={44}
          tickFormatter={(v: number) => formatY(v)}
          tickLine={false}
          axisLine={false}
          allowDecimals={false}
          tickMargin={4}
        />
        <ChartTooltip
          cursor={kind === "line"}
          content={
            <ChartTooltipContent
              indicator="line"
              labelFormatter={(_, payload) => {
                const x = (payload[0]?.payload as TrendPoint | undefined)?.[xKey];
                return x === undefined ? "" : formatX(x);
              }}
              formatter={(value, name) => (
                <div className="flex w-full items-center gap-2">
                  <span
                    aria-hidden="true"
                    className="h-2.5 w-1 shrink-0 rounded-[2px]"
                    style={{ backgroundColor: `var(--color-${String(name)})` }}
                  />
                  <span className="text-muted-foreground">{config[String(name)]?.label ?? name}</span>
                  <span className="ml-auto font-mono font-medium text-foreground tabular-nums">{formatY(Number(value))}</span>
                </div>
              )}
            />
          }
        />
        {series.map((s) =>
          kind === "area" ? (
            <Area
              key={s.key}
              dataKey={s.key}
              type="monotone"
              stroke={`var(--color-${s.key})`}
              fill={`var(--color-${s.key})`}
              fillOpacity={0.12}
              strokeWidth={1.5}
              strokeDasharray={s.dashed ? DASH : undefined}
              stackId={stacked ? "stack" : undefined}
              isAnimationActive={false}
              connectNulls
            />
          ) : (
            <Line
              key={s.key}
              dataKey={s.key}
              type="monotone"
              stroke={`var(--color-${s.key})`}
              strokeWidth={1.5}
              strokeDasharray={s.dashed ? DASH : undefined}
              dot={false}
              isAnimationActive={false}
              connectNulls
            />
          ),
        )}
      </Chart>
    </ChartContainer>
  );
}
