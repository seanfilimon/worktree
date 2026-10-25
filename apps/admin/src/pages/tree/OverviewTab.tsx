import { Activity, BookOpen, Files, GitCommitHorizontal, HardDrive, Users } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { Link } from "react-router";
import { Area, AreaChart, Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { useBlob, useBranches, useRecentEvents, useSnapshot, useSnapshots, useTreeStats } from "@/api/queries";
import type { LanguageShare, TreeStats } from "@/api/types";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { describeEvent } from "@/lib/describeEvent";
import { firstLine, formatBytes, formatNumber, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { useTreeContext } from "./context";
import { Markdown } from "./history/Markdown";
import { AuthorAvatar, EmptyState, LoadingRows, QueryError, RelativeTime, SnapshotBadges } from "./history/shared";

const WINDOWS = [7, 30, 90] as const;
type DaysWindow = (typeof WINDOWS)[number];

const README_RE = /^readme(\.(md|markdown|txt))?$/i;
const README_MAX_BYTES = 256 * 1024;

const dayTick = new Intl.DateTimeFormat("en", { month: "short", day: "numeric", timeZone: "UTC" });
const dayLabel = new Intl.DateTimeFormat("en", { weekday: "short", month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });
const fmtDay = (fmt: Intl.DateTimeFormat) => (v: unknown) => {
  const t = typeof v === "string" ? Date.parse(v) : Number.NaN;
  return Number.isNaN(t) ? String(v) : fmt.format(t);
};

function StatCard({ label, value, icon, loading }: { label: string; value: string; icon: ReactNode; loading: boolean }) {
  return (
    <Card className="gap-2 py-4">
      <CardHeader className="px-4">
        <CardDescription className="flex items-center gap-2 text-xs font-medium tracking-wide uppercase">
          <span className="[&_svg]:size-3.5">{icon}</span>
          {label}
        </CardDescription>
      </CardHeader>
      <CardContent className="px-4">
        {loading ? <Skeleton className="h-7 w-20" /> : <p className="text-2xl font-semibold tracking-tight tabular-nums">{value}</p>}
      </CardContent>
    </Card>
  );
}

const activityConfig = { snapshots: { label: "Snapshots", color: "var(--chart-1)" } } satisfies ChartConfig;

function ActivityChart({ stats, days }: { stats: TreeStats; days: number }) {
  const total = stats.activity.reduce((n, d) => n + d.snapshots, 0);
  return (
    <ChartContainer
      config={activityConfig}
      className="aspect-auto h-52 w-full"
      role="img"
      aria-label={`Snapshots per day over the last ${days} days: ${total} in total`}
    >
      <AreaChart data={stats.activity} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
        <CartesianGrid vertical={false} />
        <XAxis dataKey="date" tickLine={false} axisLine={false} tickMargin={8} minTickGap={28} tickFormatter={fmtDay(dayTick)} />
        <YAxis allowDecimals={false} tickLine={false} axisLine={false} width={28} />
        <ChartTooltip cursor={false} content={<ChartTooltipContent indicator="line" labelFormatter={fmtDay(dayLabel)} />} />
        <Area
          dataKey="snapshots"
          type="monotoneX"
          stroke="var(--color-snapshots)"
          strokeWidth={1.5}
          fill="var(--color-snapshots)"
          fillOpacity={0.12}
          isAnimationActive={false}
        />
      </AreaChart>
    </ChartContainer>
  );
}

/** Greyscale ramp from --chart-1 (strongest) to --chart-5, theme aware. */
function ramp(i: number, n: number): string {
  const p = n <= 1 ? 0 : Math.round((i / (n - 1)) * 100);
  return `color-mix(in oklch, var(--chart-1), var(--chart-5) ${p}%)`;
}

function LanguagesBar({ languages }: { languages: LanguageShare[] }) {
  const total = languages.reduce((n, l) => n + l.bytes, 0);
  const { config, row } = useMemo(() => {
    const config: ChartConfig = {};
    const row: Record<string, number | string> = { row: "languages" };
    languages.forEach((l, i) => {
      config[`l${i}`] = { label: l.language, color: ramp(i, languages.length) };
      row[`l${i}`] = l.bytes;
    });
    return { config, row };
  }, [languages]);

  if (languages.length === 0 || total === 0) return <EmptyState title="No files to analyse" className="py-6" />;

  return (
    <div className="flex flex-col gap-4">
      <ChartContainer config={config} className="aspect-auto h-3 w-full overflow-hidden rounded-full" aria-hidden="true">
        <BarChart data={[row]} layout="vertical" margin={{ top: 0, right: 0, bottom: 0, left: 0 }} barCategoryGap={0}>
          <XAxis type="number" hide domain={[0, total]} />
          <YAxis type="category" dataKey="row" hide />
          {languages.map((_, i) => (
            <Bar key={i} dataKey={`l${i}`} stackId="languages" fill={`var(--color-l${i})`} isAnimationActive={false} />
          ))}
        </BarChart>
      </ChartContainer>
      <ul aria-label="Languages" className="grid grid-cols-1 gap-x-4 gap-y-1.5 text-sm sm:grid-cols-2">
        {languages.map((l, i) => (
          <li key={l.language} className="flex items-center gap-2">
            <span aria-hidden="true" className="size-2.5 shrink-0 rounded-full border border-foreground/20" style={{ background: ramp(i, languages.length) }} />
            <span className="min-w-0 flex-1 truncate">{l.language}</span>
            <span className="text-muted-foreground tabular-nums">{((l.bytes / total) * 100).toFixed(1)}%</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Readme({ tip }: { tip: string }) {
  const { tenant, tree } = useTreeContext();
  const detail = useSnapshot(tenant, tree, tip);
  const file = useMemo(() => {
    const roots = (detail.data?.files ?? []).filter((f) => !f.path.includes("/") && README_RE.test(f.path));
    return roots.find((f) => /\.(md|markdown)$/i.test(f.path)) ?? roots[0];
  }, [detail.data]);
  const blob = useBlob(tenant, tree, tip, file?.path ?? "", !!file && file.size <= README_MAX_BYTES);
  const text = useMemo(() => (blob.data ? new TextDecoder().decode(blob.data) : ""), [blob.data]);

  if (!file) return null;
  const isMarkdown = /\.(md|markdown)$/i.test(file.path);

  return (
    <Card className="lg:col-span-3">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <BookOpen aria-hidden="true" className="size-4" />
          <h2 className="font-mono">{file.path}</h2>
        </CardTitle>
      </CardHeader>
      <CardContent>
        {file.size > README_MAX_BYTES ? (
          <p className="text-sm text-muted-foreground">{file.path} is too large to preview here.</p>
        ) : blob.isPending ? (
          <LoadingRows label="Loading README" rows={4} />
        ) : blob.isError ? (
          <QueryError error={blob.error} onRetry={() => void blob.refetch()} />
        ) : isMarkdown ? (
          <Markdown source={text} />
        ) : (
          <pre className="overflow-x-auto font-mono text-xs leading-5 whitespace-pre-wrap">{text}</pre>
        )}
      </CardContent>
    </Card>
  );
}

export function OverviewTab() {
  const { tenant, tree, summary } = useTreeContext();
  const defaultBranch = summary.tree.default_branch;
  const [days, setDays] = useState<DaysWindow>(30);
  const stats = useTreeStats(tenant, tree, "", days);
  const snapshots = useSnapshots(tenant, tree, defaultBranch);
  const events = useRecentEvents({ tenant, tree, limit: 20 });
  const branches = useBranches(tenant, tree);
  const tip = branches.data?.find((b) => b.name === defaultBranch)?.tip ?? "";
  const s = stats.data;
  const snapshotPath = (id: string) => treePath(tenant, tree, `snapshots/${encodeURIComponent(id)}`);

  return (
    <div className="grid gap-4 lg:grid-cols-3">
      <section aria-label="Tree statistics" className="grid grid-cols-2 gap-4 lg:col-span-3 lg:grid-cols-4">
        <StatCard label="Snapshots" icon={<GitCommitHorizontal aria-hidden="true" />} loading={!s} value={s ? formatNumber(s.snapshot_count) : ""} />
        <StatCard label="Files" icon={<Files aria-hidden="true" />} loading={!s} value={s ? formatNumber(s.file_count) : ""} />
        <StatCard label="Size" icon={<HardDrive aria-hidden="true" />} loading={!s} value={s ? formatBytes(s.total_bytes) : ""} />
        <StatCard label="Contributors" icon={<Users aria-hidden="true" />} loading={!s} value={s ? formatNumber(s.contributors.length) : ""} />
      </section>

      {stats.isError && <QueryError className="lg:col-span-3" error={stats.error} onRetry={() => void stats.refetch()} />}

      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle>
            <h2>Activity</h2>
          </CardTitle>
          <CardDescription>Snapshots per day on {s?.branch || defaultBranch}</CardDescription>
          <CardAction>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              aria-label="Activity window"
              value={String(days)}
              onValueChange={(v) => {
                const n = Number(v);
                if (WINDOWS.includes(n as DaysWindow)) setDays(n as DaysWindow);
              }}
            >
              {WINDOWS.map((w) => (
                <ToggleGroupItem key={w} value={String(w)} aria-label={`Last ${w} days`}>
                  {w}d
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </CardAction>
        </CardHeader>
        <CardContent>{s ? <ActivityChart stats={s} days={days} /> : <Skeleton className="h-52 w-full" />}</CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>
            <h2>Languages</h2>
          </CardTitle>
          <CardDescription>By bytes at the branch tip</CardDescription>
        </CardHeader>
        <CardContent>{s ? <LanguagesBar languages={s.languages} /> : <LoadingRows label="Loading languages" rows={3} />}</CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>
            <h2>Contributors</h2>
          </CardTitle>
        </CardHeader>
        <CardContent>
          {!s ? (
            <LoadingRows label="Loading contributors" rows={3} />
          ) : s.contributors.length === 0 ? (
            <EmptyState title="No contributors yet" icon={<Users aria-hidden="true" />} className="py-6" />
          ) : (
            <Table aria-label="Contributors">
              <TableHeader>
                <TableRow>
                  <TableHead>Author</TableHead>
                  <TableHead className="text-right">Snapshots</TableHead>
                  <TableHead className="text-right">Last active</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {[...s.contributors]
                  .sort((a, b) => b.snapshots - a.snapshots)
                  .slice(0, 8)
                  .map((c) => (
                    <TableRow key={c.author}>
                      <TableCell>
                        <span className="flex items-center gap-2">
                          <AuthorAvatar name={c.author} />
                          <span className="truncate font-medium">{c.author}</span>
                        </span>
                      </TableCell>
                      <TableCell className="text-right tabular-nums">{formatNumber(c.snapshots)}</TableCell>
                      <TableCell className="text-right">
                        <RelativeTime value={c.last_at} className="text-muted-foreground" />
                      </TableCell>
                    </TableRow>
                  ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>
            <h2>Latest snapshots</h2>
          </CardTitle>
          <CardAction>
            <Link to={treePath(tenant, tree, "history")} className="text-xs text-muted-foreground hover:text-foreground hover:underline">
              View all
            </Link>
          </CardAction>
        </CardHeader>
        <CardContent>
          {snapshots.isPending ? (
            <LoadingRows label="Loading snapshots" rows={3} />
          ) : snapshots.isError ? (
            <QueryError error={snapshots.error} onRetry={() => void snapshots.refetch()} />
          ) : snapshots.data.length === 0 ? (
            <EmptyState title="No snapshots yet" className="py-6" />
          ) : (
            <ol aria-label="Latest snapshots" className="flex flex-col gap-3">
              {snapshots.data.slice(0, 5).map((sn) => (
                <li key={sn.id} className="flex items-start gap-2 text-sm">
                  <AuthorAvatar name={sn.author} />
                  <span className="min-w-0 flex-1">
                    <span className="flex flex-wrap items-center gap-1.5">
                      <Link to={snapshotPath(sn.id)} className="truncate font-medium hover:underline">
                        {firstLine(sn.message) || "no message"}
                      </Link>
                      <SnapshotBadges snapshot={sn} />
                    </span>
                    <span className="block text-xs text-muted-foreground">
                      <span className="font-mono">{shortId(sn.id)}</span> · {sn.author} · <RelativeTime value={sn.created_at} />
                    </span>
                  </span>
                </li>
              ))}
            </ol>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>
            <h2>Recent events</h2>
          </CardTitle>
        </CardHeader>
        <CardContent>
          {events.isPending ? (
            <LoadingRows label="Loading events" rows={3} />
          ) : events.isError ? (
            <QueryError error={events.error} onRetry={() => void events.refetch()} />
          ) : events.data.length === 0 ? (
            <EmptyState title="No recent events" icon={<Activity aria-hidden="true" />} className="py-6" />
          ) : (
            <ol aria-label="Recent events" className="flex flex-col gap-2.5">
              {[...events.data]
                .reverse()
                .slice(0, 8)
                .map((e, i) => {
                  const d = describeEvent(e);
                  return (
                    <li key={`${e.at}-${i}`} className="flex items-start gap-2 text-sm">
                      <Activity aria-hidden="true" className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                      <span className="min-w-0 flex-1">
                        <span className="font-medium">{e.actor || "system"}</span> {d.action}
                        {d.target && <span className="font-mono text-xs"> {d.target}</span>}
                        <span className="block text-xs text-muted-foreground">
                          <RelativeTime value={e.at} />
                        </span>
                      </span>
                    </li>
                  );
                })}
            </ol>
          )}
        </CardContent>
      </Card>

      {tip && <Readme tip={tip} />}
    </div>
  );
}
