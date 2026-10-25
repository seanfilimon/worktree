import { Area, AreaChart, YAxis } from "recharts";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { cn } from "@/lib/utils";
import { toneColor, type ChartTone } from "./series";

export interface SparkPoint {
  /** Bucket label shown in the tooltip, e.g. "14:05". */
  label: string;
  value: number;
}

interface SparklineProps {
  data: readonly SparkPoint[];
  /** Accessible name, e.g. "Events per minute, last 30 minutes". */
  label: string;
  /** Series name in the tooltip. */
  valueLabel?: string;
  tone?: ChartTone;
  className?: string;
}

/** Axis-less monochrome area chart for inline trends. */
export function Sparkline({ data, label, valueLabel = "Value", tone = 1, className }: SparklineProps) {
  return (
    <ChartContainer
      config={{ value: { label: valueLabel, color: toneColor(tone) } }}
      role="img"
      aria-label={label}
      className={cn("aspect-auto h-16 w-full", className)}
    >
      <AreaChart data={data} margin={{ top: 4, right: 0, bottom: 0, left: 0 }}>
        <YAxis hide domain={[0, (max: number) => Math.max(1, max)]} />
        <ChartTooltip
          cursor={false}
          content={<ChartTooltipContent indicator="line" labelFormatter={(_, p) => (p[0]?.payload as SparkPoint | undefined)?.label ?? ""} />}
        />
        <Area
          dataKey="value"
          type="monotone"
          stroke="var(--color-value)"
          fill="var(--color-value)"
          fillOpacity={0.15}
          strokeWidth={1.5}
          isAnimationActive={false}
        />
      </AreaChart>
    </ChartContainer>
  );
}
