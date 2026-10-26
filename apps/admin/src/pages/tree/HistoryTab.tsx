import { GitCompareArrows, History, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { useSnapshots } from "@/api/queries";
import { useForcePushes, type ReflogEntry } from "@/api/historyV2";
import type { Snapshot } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { firstLine, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { useTreeContext } from "./context";
import { ForcePushBadge, ForcePushHistory } from "./history/ForcePushes";
import { BranchPicker } from "./history/RevisionSelect";
import { AuthorAvatar, EmptyState, LoadingRows, QueryError, RelativeTime, SnapshotBadges } from "./history/shared";

const dayFormat = new Intl.DateTimeFormat("en", { weekday: "short", month: "short", day: "numeric", year: "numeric" });

interface DayGroup {
  key: string;
  label: string;
  items: Snapshot[];
}

/** Groups newest-first snapshots by local calendar day, keeping order. */
function groupByDay(snapshots: readonly Snapshot[]): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const s of snapshots) {
    const d = new Date(s.created_at);
    const key = Number.isNaN(d.getTime()) ? "unknown" : `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    const last = groups.at(-1);
    if (last?.key === key) last.items.push(s);
    else groups.push({ key, label: key === "unknown" ? "Unknown date" : dayFormat.format(d), items: [s] });
  }
  return groups;
}

export function HistoryTab() {
  const { tenant, tree, summary } = useTreeContext();
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const branch = params.get("branch") || summary.tree.default_branch;
  const snapshots = useSnapshots(tenant, tree, branch);
  const forced = useForcePushes(tenant, tree, branch); // history-v2 (§16.10)
  const [selected, setSelected] = useState<string[]>([]);

  const groups = useMemo(() => groupByDay(snapshots.data ?? []), [snapshots.data]);

  const toggle = (id: string, on: boolean) =>
    setSelected((cur) => (on ? [...cur.filter((x) => x !== id), id].slice(-2) : cur.filter((x) => x !== id)));

  const compare = () => {
    const list = snapshots.data ?? [];
    // The list is newest first, so the larger index is the older revision.
    const [a, b] = [...selected].sort((x, y) => list.findIndex((s) => s.id === y) - list.findIndex((s) => s.id === x));
    if (!a || !b) return;
    void navigate(`${treePath(tenant, tree, "compare")}?${new URLSearchParams({ from: a, to: b }).toString()}`);
  };

  return (
    <Card className="gap-4">
      <CardHeader className="flex-wrap">
        <CardTitle>Snapshot history</CardTitle>
        <CardDescription>Newest first, up to 50 snapshots. Select two to compare them.</CardDescription>
        <CardAction>
          <BranchPicker
            tenant={tenant}
            tree={tree}
            value={branch}
            onChange={(b) => {
              setSelected([]);
              setParams({ branch: b }, { replace: true });
            }}
          />
        </CardAction>
      </CardHeader>

      <div
        role="toolbar"
        aria-label="Selection"
        className="mx-6 flex flex-wrap items-center justify-between gap-2 rounded-md border bg-muted/40 px-3 py-2 text-sm"
      >
        <span className="text-muted-foreground" aria-live="polite">
          {selected.length === 0 ? "No snapshots selected" : `${selected.length} of 2 selected`}
        </span>
        <span className="flex gap-2">
          {selected.length > 0 && (
            <Button variant="ghost" size="sm" onClick={() => setSelected([])}>
              <X aria-hidden="true" />
              Clear
            </Button>
          )}
          <Button size="sm" onClick={compare} disabled={selected.length !== 2}>
            <GitCompareArrows aria-hidden="true" />
            Compare
          </Button>
        </span>
      </div>

      <CardContent>
        {snapshots.isPending ? (
          <LoadingRows label="Loading history" rows={6} />
        ) : snapshots.isError ? (
          <QueryError error={snapshots.error} onRetry={() => void snapshots.refetch()} />
        ) : snapshots.data.length === 0 ? (
          <EmptyState title="No snapshots on this branch" icon={<History aria-hidden="true" />}>
            Push one with <code className="font-mono">wt push</code>.
          </EmptyState>
        ) : (
          <section aria-label={`Snapshots on ${branch}`} className="flex flex-col gap-5">
            <ForcePushHistory tenant={tenant} tree={tree} branch={branch} entries={forced.list} />
            {groups.map((g) => (
              <section key={g.key} aria-label={g.label}>
                <h3 className="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
                  <span aria-hidden="true" className="size-1.5 rounded-full bg-foreground" />
                  {g.label}
                </h3>
                <ol className="ml-[3px] divide-y overflow-hidden rounded-lg border">
                  {g.items.map((s) => (
                    <SnapshotRow
                      key={s.id}
                      snapshot={s}
                      href={treePath(tenant, tree, `snapshots/${encodeURIComponent(s.id)}`)}
                      selected={selected.includes(s.id)}
                      onSelect={(on) => toggle(s.id, on)}
                      forced={forced.byTip.get(s.id)}
                    />
                  ))}
                </ol>
              </section>
            ))}
          </section>
        )}
      </CardContent>
    </Card>
  );
}

function SnapshotRow({
  snapshot: s,
  href,
  selected,
  onSelect,
  forced,
}: {
  snapshot: Snapshot;
  href: string;
  selected: boolean;
  onSelect: (on: boolean) => void;
  forced?: ReflogEntry;
}) {
  const message = firstLine(s.message);
  return (
    <li className={cn("relative flex items-start gap-3 px-3 py-2.5 hover:bg-accent/60", selected && "bg-accent")}>
      <Checkbox
        className="relative z-10 mt-1"
        checked={selected}
        onCheckedChange={(c) => onSelect(c === true)}
        aria-label={`Select snapshot ${shortId(s.id)} for comparison`}
      />
      <AuthorAvatar name={s.author} className="mt-0.5 hidden sm:flex" />
      <div className="min-w-0 flex-1">
        <p className="flex flex-wrap items-center gap-2">
          <Link
            to={href}
            className="truncate font-medium outline-none after:absolute after:inset-0 hover:underline focus-visible:underline"
          >
            {message || <span className="text-muted-foreground italic">no message</span>}
          </Link>
          <SnapshotBadges snapshot={s} />
          {forced && <ForcePushBadge entry={forced} />}
        </p>
        <p className="mt-0.5 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{s.author}</span> snapshotted <RelativeTime value={s.created_at} />
        </p>
      </div>
      <span className="shrink-0 rounded border px-1.5 py-0.5 font-mono text-xs" title={s.id}>
        {shortId(s.id)}
      </span>
    </li>
  );
}
