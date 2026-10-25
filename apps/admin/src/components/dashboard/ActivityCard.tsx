import { useQueryClient } from "@tanstack/react-query";
import { Pause, Play, Radio } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { keys, useRecentEvents } from "@/api/queries";
import { EVENT_TYPES } from "@/api/types";
import { useServerEvents } from "@/api/useServerEvents";
import { Sparkline } from "@/components/charts/Sparkline";
import { LiveIndicator } from "@/components/LiveIndicator";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { describeEvent } from "@/lib/describeEvent";
import { eventKey, eventsPerMinute, MAX_FEED, mergeFeed, type FeedEntry } from "./activity";
import { EmptyState, ErrorState, SkeletonRows } from "./states";
import { TimeAgo } from "./TimeAgo";
import { useNow } from "./useNow";

const ALL = "all";

/**
 * Live activity: seeded from GET /events/recent, then appended from the SSE
 * stream. Filterable by event type; pausing freezes the list while new
 * events keep being collected.
 */
export function ActivityCard({ className }: { className?: string }) {
  const queryClient = useQueryClient();
  const recent = useRecentEvents({ limit: 50 });
  const [live, setLive] = useState<FeedEntry[]>([]);
  const seq = useRef(0);
  const [type, setType] = useState(ALL);
  /** Highest `seq` shown while paused; null = following live. */
  const [pausedAt, setPausedAt] = useState<number | null>(null);
  const now = useNow(15_000);

  const status = useServerEvents({}, (event) => {
    seq.current += 1;
    const entry: FeedEntry = { key: eventKey(event), seq: seq.current, event };
    setLive((items) => [...items, entry].slice(-MAX_FEED));
    if (event.type === "branch.pushed") void queryClient.invalidateQueries({ queryKey: ["audit"] });
    void queryClient.invalidateQueries({ queryKey: keys.stats });
  });

  const entries = useMemo(() => mergeFeed(recent.data ?? [], live), [recent.data, live]);
  const shown = pausedAt === null ? entries : entries.filter((e) => e.seq <= pausedAt);
  const pending = entries.length - shown.length;
  const visible = shown.filter((e) => type === ALL || e.event.type === type).reverse();
  const types = useMemo(
    () => [...new Set<string>([...EVENT_TYPES, ...entries.map((e) => e.event.type)])].sort(),
    [entries],
  );
  const perMinute = useMemo(() => eventsPerMinute(entries.map((e) => e.event), now), [entries, now]);
  const total = perMinute.reduce((n, p) => n + p.value, 0);

  const togglePause = () => setPausedAt(pausedAt === null ? seq.current : null);

  return (
    <Card className={className}>
      <CardHeader>
        <CardTitle>Live activity</CardTitle>
        <CardDescription>
          {total} {total === 1 ? "event" : "events"} in the last 30 minutes
        </CardDescription>
        <CardAction>
          <LiveIndicator status={status} />
        </CardAction>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-0">
        <div className="px-6">
          <Sparkline data={perMinute} label="Events per minute, last 30 minutes" valueLabel="Events" />
        </div>
        <div className="flex flex-wrap items-center gap-2 border-y px-6 py-2">
          <Select value={type} onValueChange={setType}>
            <SelectTrigger size="sm" aria-label="Event type" className="min-w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>All events</SelectItem>
              {types.map((t) => (
                <SelectItem key={t} value={t} className="font-mono text-xs">
                  {t}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button size="sm" variant="outline" aria-pressed={pausedAt !== null} onClick={togglePause}>
            {pausedAt === null ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
            {pausedAt === null ? "Pause" : "Resume"}
          </Button>
          {pending > 0 && (
            <span role="status" className="ml-auto text-xs font-medium">
              {pending} new {pending === 1 ? "event" : "events"} while paused
            </span>
          )}
        </div>
        {recent.isPending && live.length === 0 ? (
          <SkeletonRows rows={5} label="Loading recent activity" />
        ) : recent.isError && live.length === 0 ? (
          <ErrorState error={recent.error} onRetry={() => void recent.refetch()} />
        ) : visible.length === 0 ? (
          <EmptyState icon={Radio} title={type === ALL ? "Waiting for activity" : "No matching events"}>
            {type === ALL
              ? "Pushes, staged work, tags and merge requests appear here in real time."
              : `No ${type} events yet.`}
          </EmptyState>
        ) : (
          <ScrollArea className="h-80">
            <ol aria-label="Live activity" className="divide-y">
              {visible.map(({ key, event }) => {
                const { action, target } = describeEvent(event);
                return (
                  <li key={key} className="flex items-baseline gap-3 px-6 py-2.5 text-[13px]">
                    <Badge variant="outline" className="hidden w-32 shrink-0 justify-start font-mono sm:inline-flex">
                      <span className="truncate">{event.type}</span>
                    </Badge>
                    <span className="min-w-0 flex-1">
                      <span className="font-medium">{event.actor || "system"}</span> {action}
                      {target && <span className="text-muted-foreground"> — {target}</span>}
                    </span>
                    <TimeAgo value={event.at} className="shrink-0 text-xs text-muted-foreground" />
                  </li>
                );
              })}
            </ol>
          </ScrollArea>
        )}
      </CardContent>
    </Card>
  );
}
