import { RefreshCw, Search } from "lucide-react";
import { useDeferredValue, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ErrorState, SkeletonRows } from "./states";
import { useMetricsText } from "./useMetrics";

/** Raw Prometheus exposition text with a case-insensitive line filter. */
export function MetricsViewer() {
  const metrics = useMetricsText(true);
  const [filter, setFilter] = useState("");
  const term = useDeferredValue(filter.trim().toLowerCase());
  const lines = useMemo(() => (metrics.data ?? "").split(/\r?\n/).filter((l) => l !== ""), [metrics.data]);
  const shown = useMemo(() => (term ? lines.filter((l) => l.toLowerCase().includes(term)) : lines), [lines, term]);

  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">Prometheus metrics</CardTitle>
        <CardDescription>
          GET /metrics · {shown.length === lines.length ? `${lines.length} lines` : `${shown.length} of ${lines.length} lines`}
        </CardDescription>
        <CardAction>
          <Button size="sm" variant="outline" onClick={() => void metrics.refetch()} disabled={metrics.isFetching}>
            <RefreshCw aria-hidden="true" className={metrics.isFetching ? "animate-spin motion-reduce:animate-none" : undefined} />
            Refresh
          </Button>
        </CardAction>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-4">
        <div className="relative">
          <Search aria-hidden="true" className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            type="search"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter lines, e.g. go_ or worktree_http"
            aria-label="Filter metrics"
            className="pl-8 font-mono text-xs"
          />
        </div>
        {metrics.isPending ? (
          <SkeletonRows rows={8} label="Loading metrics" />
        ) : metrics.isError ? (
          <ErrorState className="m-0" error={metrics.error} onRetry={() => void metrics.refetch()} />
        ) : (
          <ScrollArea role="region" aria-label="Metrics output" className="h-[28rem] rounded-md border bg-muted/40">
            <pre className="p-3 font-mono text-xs leading-relaxed">
              {shown.length === 0 ? (
                <span className="text-muted-foreground">No lines match “{filter.trim()}”.</span>
              ) : (
                shown.map((line, i) => (
                  <span key={i} className={line.startsWith("#") ? "block text-muted-foreground" : "block"}>
                    {line}
                  </span>
                ))
              )}
            </pre>
          </ScrollArea>
        )}
      </CardContent>
    </Card>
  );
}
