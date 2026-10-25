import { Boxes, Building2, Camera, FolderTree, HardDrive, Layers, Split, Users, type LucideIcon } from "lucide-react";
import type { ServerStats } from "@/api/types";
import { formatBytes, formatNumber } from "@/lib/format";
import { StatCard } from "./StatCard";

const CARDS: { key: keyof ServerStats; label: string; icon: LucideIcon; format?: (n: number) => string }[] = [
  { key: "tenants", label: "Tenants", icon: Building2 },
  { key: "accounts", label: "Accounts", icon: Users },
  { key: "trees", label: "Trees", icon: FolderTree },
  { key: "branches", label: "Branches", icon: Split },
  { key: "snapshots", label: "Snapshots", icon: Camera },
  { key: "staged", label: "Staged WIP", icon: Layers },
  { key: "objects", label: "Objects", icon: Boxes },
  { key: "storage_bytes", label: "Storage", icon: HardDrive, format: formatBytes },
];

/** Server-wide KPI tiles (GET /admin/stats). */
export function StatsGrid({ stats, loading }: { stats: ServerStats | undefined; loading: boolean }) {
  return (
    <ul aria-label="Server statistics" className="grid grid-cols-2 gap-3 sm:grid-cols-4">
      {CARDS.map(({ key, label, icon, format = formatNumber }) => (
        <li key={key} aria-label={label}>
          <StatCard label={label} icon={icon} loading={loading || !stats} value={stats ? format(stats[key]) : null} />
        </li>
      ))}
    </ul>
  );
}
