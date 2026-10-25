import { useMemo } from "react";
import { TrendChart, type TrendPoint } from "@/components/charts/TrendChart";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { parsePrometheus, requestRate, requestTotals } from "@/lib/metrics";
import { ErrorState } from "./states";
import { useMetricsText } from "./useMetrics";
import { useSampledHistory } from "./useSampledHistory";

const fmtRate = (v: number) => (v >= 10 ? v.toFixed(0) : v.toFixed(1));
const fmtMs = (v: number) => (v >= 100 ? `${v.toFixed(0)} ms` : `${v.toFixed(1)} ms`);
const collecting = "Collecting samples: the first point appears after two scrapes, 15 seconds apart.";

/** Request rate and latency derived from successive /metrics scrapes. */
export function RequestMetricsCard({ className }: { className?: string }) {
  const metrics = useMetricsText(true);
  const totals = useMemo(() => (metrics.data === undefined ? undefined : requestTotals(parsePrometheus(metrics.data))), [metrics.data]);
  const samples = useSampledHistory(totals, metrics.dataUpdatedAt, 61);

  const points = useMemo(() => {
    const out: TrendPoint[] = [];
    for (let i = 1; i < samples.length; i++) {
      const prev = samples[i - 1]!;
      const next = samples[i]!;
      const p = requestRate(prev.value, prev.t, next.value, next.t);
      out.push({ t: p.t, rps: p.rps, errors: p.errorsPerSec, avg: p.avgMs, p95: p.p95Ms });
    }
    return out;
  }, [samples]);
  const last = points.at(-1);

  const summary: [string, string][] = [
    ["Requests/s", last ? fmtRate(last.rps ?? 0) : "—"],
    ["5xx/s", last ? fmtRate(last.errors ?? 0) : "—"],
    ["Mean latency", last?.avg === undefined ? "—" : fmtMs(last.avg)],
    ["p95 latency", last?.p95 === undefined ? "—" : fmtMs(last.p95)],
  ];

  return (
    <Card className={className}>
      <CardHeader>
        <CardTitle>HTTP traffic</CardTitle>
        <CardDescription>From Prometheus /metrics, sampled every 15 seconds while this page is open</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {metrics.isError && <ErrorState className="m-0" error={metrics.error} onRetry={() => void metrics.refetch()} />}
        <dl aria-label="Current HTTP traffic" className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {summary.map(([label, value]) => (
            <div key={label} className="rounded-md border px-3 py-2">
              <dt className="text-xs text-muted-foreground">{label}</dt>
              <dd className="font-mono text-lg font-semibold tabular-nums">{value}</dd>
            </div>
          ))}
        </dl>
        <div className="grid gap-4 lg:grid-cols-2">
          <section aria-label="Request rate">
            <h3 className="mb-1 text-xs font-medium text-muted-foreground">Request rate (req/s)</h3>
            <TrendChart
              data={points}
              xKey="t"
              label="Requests per second over time"
              series={[
                { key: "rps", label: "Requests/s", tone: 1 },
                { key: "errors", label: "5xx/s", tone: 3, dashed: true },
              ]}
              formatY={fmtRate}
              empty={collecting}
            />
          </section>
          <section aria-label="Latency">
            <h3 className="mb-1 text-xs font-medium text-muted-foreground">Latency (ms)</h3>
            <TrendChart
              data={points}
              xKey="t"
              kind="line"
              label="Mean and p95 request latency over time"
              series={[
                { key: "avg", label: "Mean", tone: 1 },
                { key: "p95", label: "p95", tone: 3, dashed: true },
              ]}
              formatY={(v) => v.toFixed(0)}
              empty={collecting}
            />
          </section>
        </div>
      </CardContent>
    </Card>
  );
}
