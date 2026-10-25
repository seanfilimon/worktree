import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import type { MouseEvent, ReactNode } from "react";
import { Link, useNavigate } from "react-router";
import { VisibilityBadge } from "@/components/people/badges";
import { DEFAULT_DIR, sortTreeRows, type TreeRow, type TreeSort, type TreeSortKey } from "@/components/people/treeSort";
import { Button } from "@/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, formatNumber, relativeTime } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";

export type { TreeRow } from "@/components/people/treeSort";

interface TreeTableProps {
  rows: TreeRow[];
  /** Prefix names with the tenant slug (cross-tenant lists). */
  showTenant?: boolean;
  /** When given, Name / Snapshots / Updated headers sort the rows. */
  sort?: TreeSort;
  onSortChange?: (sort: TreeSort) => void;
}

function SortHead({
  label,
  column,
  sort,
  onSortChange,
  className,
}: {
  label: string;
  column: TreeSortKey;
  sort: TreeSort | undefined;
  onSortChange: ((s: TreeSort) => void) | undefined;
  className?: string;
}) {
  if (!sort || !onSortChange) return <TableHead className={className}>{label}</TableHead>;
  const active = sort.key === column;
  const Icon = !active ? ArrowUpDown : sort.dir === "asc" ? ArrowUp : ArrowDown;
  const next: TreeSort = active ? { key: column, dir: sort.dir === "asc" ? "desc" : "asc" } : { key: column, dir: DEFAULT_DIR[column] };
  return (
    <TableHead aria-sort={active ? (sort.dir === "asc" ? "ascending" : "descending") : "none"} className={className}>
      <Button
        variant="ghost"
        size="xs"
        className={cn("-mx-2 font-medium", active ? "text-foreground" : "text-muted-foreground")}
        onClick={() => onSortChange(next)}
      >
        {label}
        <Icon aria-hidden="true" />
      </Button>
    </TableHead>
  );
}

function Count({ children }: { children: ReactNode }) {
  return <TableCell className="hidden text-right tabular-nums md:table-cell">{children}</TableCell>;
}

export function TreeTable({ rows, showTenant = true, sort, onSortChange }: TreeTableProps) {
  const navigate = useNavigate();
  const sorted = sort ? sortTreeRows(rows, sort) : rows;

  const openRow = (e: MouseEvent, to: string) => {
    // Links and buttons inside the row handle their own clicks.
    if ((e.target as HTMLElement).closest("a, button")) return;
    void navigate(to);
  };

  return (
    <Table aria-label="Trees">
      <TableHeader className="sticky top-0 z-10 bg-background">
        <TableRow className="hover:bg-transparent">
          <SortHead label="Tree" column="name" sort={sort} onSortChange={onSortChange} />
          <TableHead>Visibility</TableHead>
          <TableHead className="hidden lg:table-cell">Default branch</TableHead>
          <TableHead className="hidden text-right md:table-cell">Branches</TableHead>
          <SortHead label="Snapshots" column="snapshots" sort={sort} onSortChange={onSortChange} className="hidden text-right md:table-cell" />
          <TableHead className="hidden text-right md:table-cell">Staged</TableHead>
          <TableHead className="hidden text-right md:table-cell">Open MRs</TableHead>
          <SortHead label="Updated" column="updated" sort={sort} onSortChange={onSortChange} />
        </TableRow>
      </TableHeader>
      <TableBody>
        {sorted.map(({ tenant, summary: s }) => {
          const to = treePath(tenant, s.tree.name);
          return (
            <TableRow key={s.tree.id} className="cursor-pointer" onClick={(e) => openRow(e, to)}>
              <TableCell className="max-w-72 whitespace-normal">
                <Link to={to} className="font-mono font-medium hover:underline">
                  {showTenant && <span className="text-muted-foreground">{tenant}/</span>}
                  {s.tree.name}
                </Link>
                {s.tree.description && <p className="truncate text-xs text-muted-foreground">{s.tree.description}</p>}
              </TableCell>
              <TableCell>
                <VisibilityBadge visibility={s.tree.visibility} />
              </TableCell>
              <TableCell className="hidden font-mono text-xs lg:table-cell">{s.tree.default_branch}</TableCell>
              <Count>{formatNumber(s.branch_count)}</Count>
              <Count>{formatNumber(s.snapshot_count)}</Count>
              <Count>{s.staged_count > 0 ? <span className="font-semibold">{formatNumber(s.staged_count)}</span> : 0}</Count>
              <Count>{s.open_mr_count > 0 ? <span className="font-semibold">{formatNumber(s.open_mr_count)}</span> : 0}</Count>
              <TableCell className="text-muted-foreground">
                <time dateTime={s.tree.updated_at} title={absoluteTime(s.tree.updated_at)}>
                  {relativeTime(s.tree.updated_at)}
                </time>
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
