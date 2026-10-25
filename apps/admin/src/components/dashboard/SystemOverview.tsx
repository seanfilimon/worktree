import { Cpu, Database, Lock, LockOpen, Radio, Server, Timer, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import type { SystemInfo } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { absoluteTime, formatBytes, formatNumber } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useNow } from "./useNow";

/** "2d 3h 4m 5s"; leading zero units are omitted. */
function formatUptime(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const parts: [number, string][] = [
    [Math.floor(s / 86400), "d"],
    [Math.floor((s % 86400) / 3600), "h"],
    [Math.floor((s % 3600) / 60), "m"],
    [s % 60, "s"],
  ];
  const first = parts.findIndex(([n]) => n > 0);
  return parts
    .slice(first < 0 ? 3 : first)
    .map(([n, u]) => `${n}${u}`)
    .join(" ");
}

function percent(part: number, whole: number): number {
  return whole > 0 ? Math.min(100, Math.round((part / whole) * 100)) : 0;
}

function Panel({ title, icon: Icon, children, className }: { title: string; icon: LucideIcon; children: ReactNode; className?: string }) {
  return (
    <Card className={cn("gap-3 py-4", className)}>
      <CardHeader className="flex items-center justify-between px-4">
        <CardTitle className="text-sm">{title}</CardTitle>
        <Icon aria-hidden="true" className="size-4 text-muted-foreground" />
      </CardHeader>
      <CardContent className="px-4">
        <dl aria-label={title} className="flex flex-col gap-2 text-[13px]">
          {children}
        </dl>
      </CardContent>
    </Card>
  );
}

function Field({ label, children, mono = true }: { label: string; children: ReactNode; mono?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="shrink-0 text-muted-foreground">{label}</dt>
      <dd className={cn("min-w-0 truncate text-right", mono && "font-mono tabular-nums")}>{children}</dd>
    </div>
  );
}

function Meter({ label, value, of }: { label: string; value: number; of: number }) {
  const pct = percent(value, of);
  return (
    <div className="flex flex-col gap-1">
      <Field label={label}>
        {formatBytes(value)} <span className="text-muted-foreground">({pct}%)</span>
      </Field>
      <Progress value={pct} aria-label={`${label}: ${pct}% of system memory`} className="h-1.5" />
    </div>
  );
}

/** Live cards for GET /admin/system. `updatedAt` = when `info` was fetched (ms). */
export function SystemOverview({ info, updatedAt }: { info: SystemInfo; updatedAt: number }) {
  const now = useNow(1000);
  const uptime = info.uptime_seconds + Math.max(0, now - updatedAt) / 1000;
  const { memory, storage, event_bus: bus } = info;
  const onDisk = storage.object_bytes + storage.meta_db_bytes;

  return (
    <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
      <Panel title="Build" icon={Server}>
        <Field label="Version">{info.version}</Field>
        <Field label="Go">{info.go_version}</Field>
        <Field label="Platform">
          {info.os}/{info.arch}
        </Field>
        <Field label="CPUs">{formatNumber(info.num_cpu)}</Field>
      </Panel>

      <Panel title="Uptime" icon={Timer}>
        <div className="flex flex-col gap-0.5">
          <dt className="sr-only">Running for</dt>
          <dd className="font-mono text-2xl font-semibold tabular-nums">
            {formatUptime(uptime)}
          </dd>
        </div>
        <Field label="Started" mono={false}>
          <time dateTime={info.started_at}>{absoluteTime(info.started_at)}</time>
        </Field>
      </Panel>

      <Panel title="Configuration" icon={Lock}>
        <Field label="Registration" mono={false}>
          {info.allow_registration ? (
            <Badge>
              <LockOpen aria-hidden="true" />
              Open
            </Badge>
          ) : (
            <Badge variant="outline">
              <Lock aria-hidden="true" />
              Closed
            </Badge>
          )}
        </Field>
        <Field label="Data directory">
          <span title={info.data_dir}>{info.data_dir}</span>
        </Field>
      </Panel>

      <Panel title="Runtime" icon={Cpu}>
        <Field label="Goroutines">{formatNumber(info.num_goroutine)}</Field>
        <Field label="GC runs">{formatNumber(memory.gc_runs)}</Field>
        <Field label="System memory">{formatBytes(memory.sys_bytes)}</Field>
        <Meter label="Heap allocated" value={memory.alloc_bytes} of={memory.sys_bytes} />
        <Meter label="Heap in use" value={memory.heap_inuse_bytes} of={memory.sys_bytes} />
      </Panel>

      <Panel title="Storage" icon={Database}>
        <Field label="Objects">{formatNumber(storage.objects)}</Field>
        <Field label="Object bytes">{formatBytes(storage.object_bytes)}</Field>
        <Field label="Metadata DB">{formatBytes(storage.meta_db_bytes)}</Field>
        <div className="flex flex-col gap-1">
          <Field label="Objects share of disk">{percent(storage.object_bytes, onDisk)}%</Field>
          <Progress value={percent(storage.object_bytes, onDisk)} aria-label="Objects share of on-disk size" className="h-1.5" />
        </div>
      </Panel>

      <Panel title="Event bus" icon={Radio}>
        <Field label="Subscribers">{formatNumber(bus.subscribers)}</Field>
        <Field label="Published">{formatNumber(bus.published)}</Field>
        <Field label="Delivered">{formatNumber(bus.delivered)}</Field>
        <Field label="Dropped">
          {bus.dropped > 0 ? <Badge variant="default">{formatNumber(bus.dropped)}</Badge> : "0"}
        </Field>
      </Panel>

    </div>
  );
}
