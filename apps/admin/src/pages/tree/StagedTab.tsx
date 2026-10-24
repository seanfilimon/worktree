import { useState } from "react";
import { Link, useSearchParams } from "react-router";
import { useStaged } from "@/api/queries";
import type { StagedSnapshot } from "@/api/types";
import { LiveIndicator } from "@/components/LiveIndicator";
import { Badge } from "@/components/legacy/Badge";
import { Card, Section } from "@/components/legacy/Card";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/legacy/States";
import { RelativeTime } from "@/components/legacy/Time";
import { firstLine, shortId } from "@/lib/format";
import { filesPath } from "@/lib/paths";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";

const COLLAPSED_FILES = 8;

function StagedCard({ s }: { s: StagedSnapshot }) {
  const { tenant, tree } = useTreeContext();
  const [expanded, setExpanded] = useState(false);
  const files = s.files_changed ?? [];
  const shown = expanded ? files : files.slice(0, COLLAPSED_FILES);
  const listId = `staged-files-${s.snapshot_id}`;

  return (
    <li>
      <Card className="p-4">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <p className="flex flex-wrap items-center gap-2">
              <span className="font-semibold">{s.author}</span>
              <span className="text-muted-foreground">on</span>
              <Badge tone="info">{s.branch_name}</Badge>
            </p>
            {s.message && <p className="mt-1 truncate text-[13px]">{firstLine(s.message)}</p>}
          </div>
          <div className="text-right text-xs text-muted-foreground">
            <p>
              staged <RelativeTime value={s.staged_at} />
            </p>
            <Link to={filesPath(tenant, tree, { snapshot: s.snapshot_id })} className="font-mono text-foreground hover:underline">
              {shortId(s.snapshot_id)}
            </Link>
          </div>
        </div>
        <p className="mt-3 flex gap-3 font-mono text-xs" aria-label="Change summary">
          <span className="text-foreground">+{s.files_added} added</span>
          <span className="text-warning">~{s.files_modified} modified</span>
          <span className="text-danger">-{s.files_deleted} deleted</span>
        </p>
        {files.length > 0 && (
          <>
            <ul id={listId} aria-label={`Files changed by ${s.author}`} className="mt-2 flex flex-col gap-0.5 font-mono text-xs text-muted-foreground">
              {shown.map((f) => (
                <li key={f} className="truncate">
                  {f}
                </li>
              ))}
            </ul>
            {files.length > COLLAPSED_FILES && (
              <button
                type="button"
                onClick={() => setExpanded((v) => !v)}
                aria-expanded={expanded}
                aria-controls={listId}
                className="mt-1 text-xs font-medium text-foreground hover:underline"
              >
                {expanded ? "Show fewer" : `Show all ${files.length} files`}
              </button>
            )}
          </>
        )}
      </Card>
    </li>
  );
}

export function StagedTab() {
  const { tenant, tree, live } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const branch = params.get("branch") ?? "";
  const staged = useStaged(tenant, tree, branch);

  return (
    <Section
      title="Staged work in progress"
      description="Who is working on what. Updates live as teammates stage or push."
      actions={
        <>
          <LiveIndicator status={live} />
          <BranchSelect tenant={tenant} tree={tree} value={branch} allowAll onChange={(b) => setParams(b ? { branch: b } : {}, { replace: true })} />
        </>
      }
    >
      {staged.isPending ? (
        <SkeletonRows label="Loading staged work" />
      ) : staged.isError ? (
        <ErrorState error={staged.error} onRetry={() => void staged.refetch()} />
      ) : staged.data.length === 0 ? (
        <EmptyState title="Nobody has staged work">
          Teammates share WIP with <code className="font-mono">wt stage</code>, or automatically via the background daemon.
        </EmptyState>
      ) : (
        <ul aria-label="Staged snapshots" className="grid gap-3 lg:grid-cols-2">
          {[...staged.data]
            .sort((a, b) => b.staged_at.localeCompare(a.staged_at))
            .map((s) => (
              <StagedCard key={`${s.account_id}-${s.branch_name}`} s={s} />
            ))}
        </ul>
      )}
    </Section>
  );
}
