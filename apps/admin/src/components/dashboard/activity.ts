import type { ServerEvent } from "@/api/types";
import type { SparkPoint } from "@/components/charts/Sparkline";

/** A feed row. `seq` is 0 for seeded (recent) events, then 1, 2, … per live event. */
export interface FeedEntry {
  key: string;
  seq: number;
  event: ServerEvent;
}

/** Most events the dashboard keeps in memory. */
export const MAX_FEED = 200;

/** Identity of an event, used to drop live duplicates of seeded events. */
export function eventKey(e: ServerEvent): string {
  return [e.type, e.at, e.tenant_id, e.tree_id ?? "", e.actor ?? "", JSON.stringify(e.payload ?? null)].join("|");
}

/** Seed (oldest first) followed by live entries, deduplicated and capped; oldest first. */
export function mergeFeed(seed: readonly ServerEvent[], live: readonly FeedEntry[]): FeedEntry[] {
  const seen = new Set<string>();
  const out: FeedEntry[] = [];
  for (const entry of [...seed.map((event) => ({ key: eventKey(event), seq: 0, event })), ...live]) {
    if (seen.has(entry.key)) continue;
    seen.add(entry.key);
    out.push(entry);
  }
  return out.slice(-MAX_FEED);
}

const minuteLabel = new Intl.DateTimeFormat("en", { hour: "2-digit", minute: "2-digit" });

/** Per-minute event counts for the `minutes` minutes ending at `now`, oldest first. */
export function eventsPerMinute(events: readonly ServerEvent[], now: number, minutes = 30): SparkPoint[] {
  const end = Math.floor(now / 60_000);
  const start = end - minutes + 1;
  const counts = new Array<number>(minutes).fill(0);
  for (const e of events) {
    const minute = Math.floor(Date.parse(e.at) / 60_000);
    if (minute >= start && minute <= end) counts[minute - start]! += 1;
  }
  return counts.map((value, i) => ({ label: minuteLabel.format((start + i) * 60_000), value }));
}
