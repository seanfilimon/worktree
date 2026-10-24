import { Link } from "react-router";
import { useTags } from "@/api/queries";
import { Section } from "@/components/ui/Card";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { Table, Td, Th, Tr } from "@/components/ui/Table";
import { RelativeTime } from "@/components/ui/Time";
import { firstLine, shortId } from "@/lib/format";
import { filesPath } from "@/lib/paths";
import { useTreeContext } from "./context";

export function TagsTab() {
  const { tenant, tree } = useTreeContext();
  const tags = useTags(tenant, tree);

  return (
    <Section title="Tags" flush>
      {tags.isPending ? (
        <SkeletonRows label="Loading tags" />
      ) : tags.isError ? (
        <ErrorState error={tags.error} onRetry={() => void tags.refetch()} />
      ) : tags.data.length === 0 ? (
        <EmptyState title="No tags">Create one with <code className="font-mono">wt tag create NAME</code> and push.</EmptyState>
      ) : (
        <Table label="Tags">
          <thead>
            <tr>
              <Th>Tag</Th>
              <Th>Target</Th>
              <Th>Message</Th>
              <Th>Tagger</Th>
              <Th>Created</Th>
            </tr>
          </thead>
          <tbody>
            {[...tags.data]
              .sort((a, b) => b.created_at.localeCompare(a.created_at))
              .map((t) => (
                <Tr key={t.name}>
                  <Td className="font-mono font-medium">{t.name}</Td>
                  <Td>
                    <Link to={filesPath(tenant, tree, { snapshot: t.target })} className="font-mono text-accent hover:underline" title={t.target}>
                      {shortId(t.target)}
                    </Link>
                  </Td>
                  <Td className="max-w-xs truncate">{firstLine(t.message) || <span className="text-subtle">—</span>}</Td>
                  <Td>{t.tagger}</Td>
                  <Td>
                    <RelativeTime value={t.created_at} className="text-muted" />
                  </Td>
                </Tr>
              ))}
          </tbody>
        </Table>
      )}
    </Section>
  );
}
