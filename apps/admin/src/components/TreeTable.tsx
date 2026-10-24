import { Link } from "react-router";
import type { TreeSummary } from "@/api/types";
import { treePath } from "@/lib/paths";
import { StateBadge } from "./legacy/Badge";
import { Table, Td, Th, Tr } from "./legacy/Table";
import { RelativeTime } from "./legacy/Time";

export interface TreeRow {
  tenant: string;
  summary: TreeSummary;
}

export function TreeTable({ rows }: { rows: TreeRow[] }) {
  return (
    <Table label="Trees">
      <thead>
        <tr>
          <Th>Tree</Th>
          <Th>Visibility</Th>
          <Th>Default branch</Th>
          <Th className="text-right">Branches</Th>
          <Th className="text-right">Snapshots</Th>
          <Th className="text-right">Staged</Th>
          <Th className="text-right">Open MRs</Th>
          <Th>Updated</Th>
        </tr>
      </thead>
      <tbody>
        {rows.map(({ tenant, summary: s }) => (
          <Tr key={s.tree.id}>
            <Td>
              <Link to={treePath(tenant, s.tree.name)} className="font-mono font-medium hover:underline">
                <span className="text-muted-foreground">{tenant}/</span>
                <span className="text-foreground">{s.tree.name}</span>
              </Link>
              {s.tree.description && <p className="max-w-sm truncate text-xs text-muted-foreground">{s.tree.description}</p>}
            </Td>
            <Td>
              <StateBadge value={s.tree.visibility} />
            </Td>
            <Td className="font-mono">{s.tree.default_branch}</Td>
            <Td className="text-right tabular-nums">{s.branch_count}</Td>
            <Td className="text-right tabular-nums">{s.snapshot_count}</Td>
            <Td className="text-right tabular-nums">{s.staged_count}</Td>
            <Td className="text-right tabular-nums">{s.open_mr_count}</Td>
            <Td>
              <RelativeTime value={s.tree.updated_at} className="text-muted-foreground" />
            </Td>
          </Tr>
        ))}
      </tbody>
    </Table>
  );
}
