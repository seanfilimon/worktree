import { useMemo } from "react";
import type { SystemInfo } from "@/api/types";
import { TrendChart } from "@/components/charts/TrendChart";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import type { Sample } from "./useSampledHistory";

const MIB = 1024 * 1024;
const waiting = "Collecting samples: a new point is added every 10 seconds.";

/** Goroutine and heap charts over the in-memory samples of GET /admin/system. */
export function SystemHistory({ samples }: { samples: readonly Sample<SystemInfo>[] }) {
  const points = useMemo(
    () =>
      samples.map(({ t, value }) => ({
        t,
        goroutines: value.num_goroutine,
        heap: value.memory.heap_inuse_bytes / MIB,
        alloc: value.memory.alloc_bytes / MIB,
      })),
    [samples],
  );

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Card className="gap-3 py-4">
        <CardHeader className="px-4">
          <CardTitle className="text-sm">Goroutines</CardTitle>
          <CardDescription>Last {samples.length} of up to 60 samples</CardDescription>
        </CardHeader>
        <CardContent className="px-4">
          <TrendChart
            data={points}
            xKey="t"
            kind="line"
            label="Goroutines over time"
            series={[{ key: "goroutines", label: "Goroutines", tone: 1 }]}
            empty={waiting}
          />
        </CardContent>
      </Card>
      <Card className="gap-3 py-4">
        <CardHeader className="px-4">
          <CardTitle className="text-sm">Heap (MiB)</CardTitle>
          <CardDescription>In use and allocated</CardDescription>
        </CardHeader>
        <CardContent className="px-4">
          <TrendChart
            data={points}
            xKey="t"
            label="Heap memory over time"
            series={[
              { key: "heap", label: "In use", tone: 1 },
              { key: "alloc", label: "Allocated", tone: 3, dashed: true },
            ]}
            formatY={(v) => v.toFixed(0)}
            empty={waiting}
          />
        </CardContent>
      </Card>
    </div>
  );
}
