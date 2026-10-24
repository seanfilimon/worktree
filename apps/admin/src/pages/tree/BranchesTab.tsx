import { Link } from "react-router";
import { useBranches } from "@/api/queries";
import { Icon } from "@/components/icons";
import { Badge } from "@/components/legacy/Badge";
import { Section } from "@/components/legacy/Card";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/legacy/States";
import { Table, Td, Th, Tr } from "@/components/legacy/Table";
import { RelativeTime } from "@/components/legacy/Time";
import { shortId } from "@/lib/format";
import { filesPath, treePath } from "@/lib/paths";
import { useTreeContext } from "./context";

export function BranchesTab() {
  const { tenant, tree, summary } = useTreeContext();
  const branches = useBranches(tenant, tree);
  const defaultBranch = summary.tree.default_branch;

  return (
    <Section title="Branches" flush>
      {branches.isPending ? (
        <SkeletonRows label="Loading branches" />
      ) : branches.isError ? (
        <ErrorState error={branches.error} onRetry={() => void branches.refetch()} />
      ) : branches.data.length === 0 ? (
        <EmptyState title="No branches yet">Push a branch with <code className="font-mono">wt push</code>.</EmptyState>
      ) : (
        <Table label="Branches">
          <thead>
            <tr>
              <Th>Branch</Th>
              <Th>Tip</Th>
              <Th>Updated</Th>
              <Th>Created</Th>
              <Th>
                <span className="sr-only">Actions</span>
              </Th>
            </tr>
          </thead>
          <tbody>
            {[...branches.data]
              .sort((a, b) => (a.name === defaultBranch ? -1 : b.name === defaultBranch ? 1 : a.name.localeCompare(b.name)))
              .map((b) => (
                <Tr key={b.name}>
                  <Td>
                    <span className="flex flex-wrap items-center gap-1.5">
                      <Icon name="branch" className="size-3.5 text-subtle" />
                      <span className="font-mono font-medium">{b.name}</span>
                      {b.name === defaultBranch && <Badge tone="info">default</Badge>}
                      {b.protected && (
                        <Badge tone="warning" title="Branch protection rules apply">
                          <Icon name="lock" className="size-3" />
                          protected
                        </Badge>
                      )}
                    </span>
                  </Td>
                  <Td>
                    {b.tip ? (
                      <Link to={filesPath(tenant, tree, { snapshot: b.tip })} className="font-mono text-foreground hover:underline" title={b.tip}>
                        {shortId(b.tip)}
                      </Link>
                    ) : (
                      <span className="text-subtle">empty</span>
                    )}
                  </Td>
                  <Td>
                    <RelativeTime value={b.updated_at} className="text-muted-foreground" />
                  </Td>
                  <Td>
                    <RelativeTime value={b.created_at} className="text-muted-foreground" />
                  </Td>
                  <Td className="text-right">
                    <Link
                      to={`${treePath(tenant, tree, "history")}?branch=${encodeURIComponent(b.name)}`}
                      className="text-[13px] text-muted-foreground hover:text-fg hover:underline"
                    >
                      History
                    </Link>
                  </Td>
                </Tr>
              ))}
          </tbody>
        </Table>
      )}
    </Section>
  );
}
