import { Check, CircleX, GitPullRequest, Plus, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { errorMessage } from "@/api/client";
import { useMergeRequests, useProtections } from "@/api/queries";
import { MR_STATES, type BranchProtection, type MergeRequest, type MergeRequestState } from "@/api/types";
import { BranchFlow } from "@/components/mr/BranchFlow";
import { CreateMergeRequestDialog } from "@/components/mr/CreateMergeRequestDialog";
import { MergeStateBadge } from "@/components/mr/MergeStateBadge";
import { Timestamp } from "@/components/mr/Timestamp";
import { UserChip } from "@/components/mr/UserAvatar";
import { approvalCount, matchingProtections, requiredApprovals } from "@/components/mr/model";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { mergeRequestPath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { useTreeContext } from "./context";

type Filter = MergeRequestState | "all";
const FILTERS: Filter[] = [...MR_STATES, "all"];
const LABEL: Record<Filter, string> = { open: "Open", merged: "Merged", closed: "Closed", all: "All" };

function parseFilter(raw: string | null): Filter {
  return FILTERS.find((f) => f === raw) ?? "open";
}

function matchesSearch(mr: MergeRequest, q: string): boolean {
  if (!q) return true;
  const needle = q.toLowerCase().replace(/^[!#](?=\d)/, "");
  return [mr.title, mr.author, mr.source_branch, mr.target_branch, String(mr.number)].some((s) =>
    s.toLowerCase().includes(needle),
  );
}

export function MergeRequestsTab() {
  const { tenant, tree, summary } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const filter = parseFilter(params.get("state"));
  const q = params.get("q") ?? "";
  const [creating, setCreating] = useState(false);
  // One request for every state: the filter counts come from the same list.
  const mrs = useMergeRequests(tenant, tree, "");
  const protections = useProtections(tenant, tree);

  const update = (next: { state?: Filter; q?: string }) => {
    setParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        if (next.state !== undefined) p.set("state", next.state);
        if (next.q !== undefined) {
          if (next.q) p.set("q", next.q);
          else p.delete("q");
        }
        return p;
      },
      { replace: true },
    );
  };

  const all = useMemo(() => mrs.data ?? [], [mrs.data]);
  const counts = useMemo(() => {
    const c: Record<Filter, number> = { open: 0, merged: 0, closed: 0, all: all.length };
    for (const mr of all) c[mr.state] += 1;
    return c;
  }, [all]);
  const visible = useMemo(
    () =>
      all
        .filter((mr) => (filter === "all" || mr.state === filter) && matchesSearch(mr, q.trim()))
        .sort((a, b) => b.updated_at.localeCompare(a.updated_at)),
    [all, filter, q],
  );

  return (
    <Card className="gap-0 py-0">
      <div className="flex flex-col gap-3 border-b p-4 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex items-center gap-2">
          <GitPullRequest aria-hidden className="size-4 text-muted-foreground" />
          <h2 className="text-base font-semibold">Merge requests</h2>
        </div>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            value={filter}
            onValueChange={(v) => {
              if (v) update({ state: parseFilter(v) });
            }}
            aria-label="Filter by state"
            className="w-full sm:w-fit"
          >
            {FILTERS.map((f) => (
              <ToggleGroupItem key={f} value={f} aria-label={`${LABEL[f]} (${counts[f]})`} className="flex-1 gap-1.5 sm:flex-none">
                {LABEL[f]}
                <span className="font-mono text-xs text-muted-foreground tabular-nums">{mrs.isPending ? "–" : counts[f]}</span>
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <div className="relative sm:w-64">
            <Search aria-hidden className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              type="search"
              aria-label="Search merge requests"
              placeholder="Search title, author, branch…"
              value={q}
              onChange={(e) => update({ q: e.target.value })}
              className="h-8 pl-8"
            />
          </div>
          <Button size="sm" onClick={() => setCreating(true)}>
            <Plus aria-hidden />
            New merge request
          </Button>
        </div>
      </div>

      <CardContent className="px-0">
        {mrs.isPending ? (
          <div aria-label="Loading merge requests" className="flex flex-col gap-3 p-4">
            {Array.from({ length: 4 }, (_, i) => (
              <Skeleton key={i} className="h-8 w-full" />
            ))}
          </div>
        ) : mrs.isError ? (
          <div className="p-4">
            <Alert>
              <CircleX aria-hidden />
              <AlertTitle>Could not load merge requests</AlertTitle>
              <AlertDescription>
                <p>{errorMessage(mrs.error)}</p>
                <Button size="sm" variant="outline" onClick={() => void mrs.refetch()}>
                  Retry
                </Button>
              </AlertDescription>
            </Alert>
          </div>
        ) : visible.length === 0 ? (
          <EmptyList
            total={all.length}
            filter={filter}
            query={q.trim()}
            onClear={() => update({ q: "" })}
            onCreate={() => setCreating(true)}
          />
        ) : (
          <MergeRequestTable rows={visible} protections={protections.data ?? []} tenant={tenant} tree={tree} />
        )}
      </CardContent>

      <CreateMergeRequestDialog
        tenant={tenant}
        tree={tree}
        defaultBranch={summary.tree.default_branch}
        open={creating}
        onOpenChange={setCreating}
      />
    </Card>
  );
}

function MergeRequestTable({
  rows,
  protections,
  tenant,
  tree,
}: {
  rows: MergeRequest[];
  protections: BranchProtection[];
  tenant: string;
  tree: string;
}) {
  return (
    <Table aria-label="Merge requests">
      <TableHeader className="sticky top-0 z-10 bg-card">
        <TableRow>
          <TableHead className="w-16 pl-4">#</TableHead>
          <TableHead>Title</TableHead>
          <TableHead className="hidden md:table-cell">Branches</TableHead>
          <TableHead className="hidden lg:table-cell">Author</TableHead>
          <TableHead className="text-right">Approvals</TableHead>
          <TableHead className="hidden sm:table-cell">Updated</TableHead>
          <TableHead className="pr-4">State</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((mr) => {
          const required = requiredApprovals(matchingProtections(protections, mr.target_branch));
          const have = approvalCount(mr);
          const met = have >= required;
          return (
            <TableRow key={mr.id} className={cn(mr.state === "closed" && "text-muted-foreground")}>
              <TableCell className="pl-4 font-mono text-muted-foreground tabular-nums">!{mr.number}</TableCell>
              <TableCell className="max-w-[28rem] whitespace-normal">
                <Link
                  to={mergeRequestPath(tenant, tree, mr.number)}
                  className="font-medium break-words underline-offset-4 hover:underline focus-visible:underline"
                >
                  {mr.title}
                </Link>
                <BranchFlow source={mr.source_branch} target={mr.target_branch} className="mt-1 md:hidden" />
              </TableCell>
              <TableCell className="hidden md:table-cell">
                <BranchFlow source={mr.source_branch} target={mr.target_branch} />
              </TableCell>
              <TableCell className="hidden lg:table-cell">
                <UserChip name={mr.author} />
              </TableCell>
              <TableCell className="text-right">
                <span
                  className={cn("inline-flex items-center gap-1 font-mono tabular-nums", met && required > 0 && "font-semibold")}
                  aria-label={required > 0 ? `${have} of ${required} required approvals` : `${have} approvals`}
                >
                  {met && required > 0 && <Check aria-hidden className="size-3.5" />}
                  {required > 0 ? `${have}/${required}` : have}
                </span>
              </TableCell>
              <TableCell className="hidden text-muted-foreground sm:table-cell">
                <Timestamp value={mr.updated_at} />
              </TableCell>
              <TableCell className="pr-4">
                <MergeStateBadge state={mr.state} />
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}

function EmptyList({
  total,
  filter,
  query,
  onClear,
  onCreate,
}: {
  total: number;
  filter: Filter;
  query: string;
  onClear: () => void;
  onCreate: () => void;
}) {
  const body: { title: string; hint: string; action?: { label: string; run: () => void } } =
    total === 0
      ? { title: "No merge requests yet", hint: "Open one to propose merging a branch.", action: { label: "New merge request", run: onCreate } }
      : query
        ? { title: `No merge requests match “${query}”`, hint: "Try a different title, author or branch.", action: { label: "Clear search", run: onClear } }
        : { title: `No ${filter === "all" ? "" : `${filter} `}merge requests`, hint: "Pick another state to see more." };
  return (
    <div className="flex flex-col items-center gap-2 px-4 py-12 text-center">
      <GitPullRequest aria-hidden className="size-8 text-muted-foreground" />
      <p className="font-medium">{body.title}</p>
      <p className="text-sm text-muted-foreground">{body.hint}</p>
      {body.action && (
        <Button size="sm" variant="outline" onClick={body.action.run}>
          {body.action.label}
        </Button>
      )}
    </div>
  );
}
