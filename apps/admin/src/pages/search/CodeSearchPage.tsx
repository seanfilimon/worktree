import { CaseSensitive, FileCode2, FileSearch, Loader2, Regex, Search, TriangleAlert } from "lucide-react";
import { useId, useMemo, useState, type FormEvent, type ReactNode } from "react";
import { Link, useSearchParams } from "react-router";
import { useTenants } from "@/api/queries";
import { isUnavailable, MIN_CODE_QUERY, useCodeSearch, type CodeSearchHit, type CodeSearchQuery } from "@/api/v06";
import { EmptyState, ErrorState, LoadingRows } from "@/components/files/states";
import { PageHeader } from "@/components/PageHeader";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Toggle } from "@/components/ui/toggle";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { formatNumber, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { codeHitHref, groupHits, highlightParts } from "./codeSearch";

const ALL = "__all__";
const CONTEXT_LINES = 2;
const LIMIT = 200;

function CodeLine({ n, href, children, hit }: { n: number; href: string; children: ReactNode; hit?: boolean }) {
  return (
    <tr data-hit={hit ? "" : undefined} className={hit ? "bg-accent" : undefined}>
      <td className="w-px border-r px-3 text-right align-top text-muted-foreground tabular-nums select-none">
        <Link to={href} aria-label={`Line ${n}`} className="hover:text-foreground hover:underline">
          {n}
        </Link>
      </td>
      <td className="px-3 whitespace-pre">{children}</td>
    </tr>
  );
}

function HitBlock({ hit, q, regex, caseSensitive }: { hit: CodeSearchHit; q: string; regex: boolean; caseSensitive: boolean }) {
  const before = hit.before ?? [];
  const after = hit.after ?? [];
  return (
    <tbody className="border-t first:border-t-0">
      {before.map((text, i) => {
        const n = hit.line - before.length + i;
        return (
          <CodeLine key={`b${n}`} n={n} href={codeHitHref(hit, n)}>
            <span className="text-muted-foreground">{text || " "}</span>
          </CodeLine>
        );
      })}
      <CodeLine n={hit.line} href={codeHitHref(hit, hit.line)} hit>
        {highlightParts(hit.text, q, regex, caseSensitive).map((p, i) =>
          p.match ? (
            <mark key={i} className="rounded-[2px] bg-foreground px-0.5 font-bold text-background">
              {p.text}
            </mark>
          ) : (
            <span key={i}>{p.text}</span>
          ),
        )}
      </CodeLine>
      {after.map((text, i) => {
        const n = hit.line + 1 + i;
        return (
          <CodeLine key={`a${n}`} n={n} href={codeHitHref(hit, n)}>
            <span className="text-muted-foreground">{text || " "}</span>
          </CodeLine>
        );
      })}
    </tbody>
  );
}

function readQuery(params: URLSearchParams): CodeSearchQuery {
  return {
    q: params.get("q") ?? "",
    tenant: params.get("tenant") ?? undefined,
    tree: params.get("tree") ?? undefined,
    path: params.get("path") ?? undefined,
    regex: params.get("regex") === "1",
    case: params.get("case") === "1",
    context: CONTEXT_LINES,
    limit: LIMIT,
  };
}

/** `/search/code`: search the default-branch tip of every readable tree. */
export function CodeSearchPage() {
  const id = useId();
  const [params, setParams] = useSearchParams();
  const query = readQuery(params);
  const tenants = useTenants();
  const [q, setQ] = useState(query.q);
  const [tenant, setTenant] = useState(query.tenant ?? "");
  const [tree, setTree] = useState(query.tree ?? "");
  const [path, setPath] = useState(query.path ?? "");
  const [regex, setRegex] = useState(Boolean(query.regex));
  const [caseSensitive, setCaseSensitive] = useState(Boolean(query.case));
  // Navigation to another search (e.g. from the command palette) resets the form.
  const [synced, setSynced] = useState(params.toString());
  if (synced !== params.toString()) {
    setSynced(params.toString());
    setQ(query.q);
    setTenant(query.tenant ?? "");
    setTree(query.tree ?? "");
    setPath(query.path ?? "");
    setRegex(Boolean(query.regex));
    setCaseSensitive(Boolean(query.case));
  }
  const search = useCodeSearch(query);
  const term = query.q.trim();
  const tooShort = !query.regex && term.length > 0 && term.length < MIN_CODE_QUERY;
  const groups = useMemo(() => groupHits(search.data?.hits ?? []), [search.data]);
  const fileCount = groups.reduce((n, g) => n + g.files.length, 0);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const next = new URLSearchParams();
    if (q.trim()) next.set("q", q.trim());
    if (tenant) next.set("tenant", tenant);
    if (tree.trim()) next.set("tree", tree.trim());
    if (path.trim()) next.set("path", path.trim());
    if (regex) next.set("regex", "1");
    if (caseSensitive) next.set("case", "1");
    setParams(next);
  };

  let body: ReactNode;
  if (!term) {
    body = (
      <EmptyState icon={FileSearch} title="Search code across your trees">
        Searches the tip of every tree&apos;s default branch you can read. Use at least {MIN_CODE_QUERY} characters, or a regular expression
        (RE2 syntax).
      </EmptyState>
    );
  } else if (tooShort) {
    body = (
      <EmptyState icon={Search} title="Query too short">
        Literal searches need at least {MIN_CODE_QUERY} characters.
      </EmptyState>
    );
  } else if (search.isPending) {
    body = <LoadingRows rows={8} label="Searching code" />;
  } else if (search.isError) {
    body = isUnavailable(search.error) ? (
      <div className="p-4">
        <UnavailableNotice feature="Code search" />
      </div>
    ) : (
      <ErrorState error={search.error} onRetry={() => void search.refetch()} />
    );
  } else if (search.data.hits.length === 0) {
    body = (
      <EmptyState icon={Search} title={`No results for “${term}”`}>
        Searched {formatNumber(search.data.searched_trees)} {search.data.searched_trees === 1 ? "tree" : "trees"}. Binary files, files over
        1 MiB and paths you may not read are not searched.
      </EmptyState>
    );
  } else {
    body = (
      <div className="flex flex-col gap-6 p-4">
        {groups.map((g) => (
          <section key={g.tree} aria-label={`Results in ${g.tree}`} className="flex flex-col gap-3">
            <h2 className="flex items-center gap-2 font-mono text-sm font-semibold">
              <Link to={treePath(g.files[0]!.tenant, g.files[0]!.tree)} className="hover:underline">
                {g.tree}
              </Link>
              <Badge variant="outline" className="font-mono">
                {g.files[0]!.branch}
              </Badge>
            </h2>
            {g.files.map((f) => (
              <article key={f.key} aria-label={f.path} className="overflow-hidden rounded-lg border bg-card">
                <header className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b bg-muted/40 px-3 py-2">
                  <FileCode2 aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
                  <Link to={codeHitHref(f)} className="min-w-0 flex-1 truncate font-mono text-[13px] font-semibold hover:underline">
                    {f.path}
                  </Link>
                  <span className="text-xs text-muted-foreground tabular-nums">
                    {f.hits.length} {f.hits.length === 1 ? "match" : "matches"} · <span className="font-mono">{shortId(f.snapshot_id)}</span>
                  </span>
                </header>
                <div className="overflow-x-auto font-mono text-[12.5px] leading-5">
                  <table className="w-full border-collapse">
                    {f.hits.map((h) => (
                      <HitBlock key={`${h.line}`} hit={h} q={term} regex={Boolean(query.regex)} caseSensitive={Boolean(query.case)} />
                    ))}
                  </table>
                </div>
              </article>
            ))}
          </section>
        ))}
      </div>
    );
  }

  return (
    <>
      <PageHeader
        title="Code search"
        breadcrumbs={[{ label: "Code search" }]}
        description="Find code on the default branch of every tree you can read."
      />
      <form onSubmit={submit} role="search" aria-label="Code search" className="mb-4 flex flex-col gap-3 rounded-xl border bg-card p-3">
        <div className="flex gap-2">
          <Label htmlFor={`${id}-q`} className="sr-only">
            Search code
          </Label>
          <Input
            id={`${id}-q`}
            type="search"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={regex ? "func\\s+New[A-Z]\\w*" : "handleBlob"}
            className="font-mono"
            autoComplete="off"
            spellCheck={false}
            autoFocus
          />
          <Toggle
            variant="outline"
            pressed={regex}
            onPressedChange={setRegex}
            aria-label="Regular expression"
            title="Regular expression (RE2)"
            className="data-[state=on]:bg-foreground data-[state=on]:text-background"
          >
            <Regex aria-hidden="true" />
          </Toggle>
          <Toggle
            variant="outline"
            pressed={caseSensitive}
            onPressedChange={setCaseSensitive}
            aria-label="Match case"
            title="Match case"
            className="data-[state=on]:bg-foreground data-[state=on]:text-background"
          >
            <CaseSensitive aria-hidden="true" />
          </Toggle>
          <Button type="submit">
            {search.isFetching ? <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" /> : <Search aria-hidden="true" />}
            Search
          </Button>
        </div>
        <div className="grid gap-3 sm:grid-cols-3">
          <div className="grid gap-1.5">
            <Label htmlFor={`${id}-tenant`} className="text-xs">
              Tenant
            </Label>
            <Select value={tenant || ALL} onValueChange={(v) => setTenant(v === ALL ? "" : v)}>
              <SelectTrigger id={`${id}-tenant`} size="sm" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>All tenants</SelectItem>
                {(tenants.data ?? []).map((t) => (
                  <SelectItem key={t.id} value={t.slug}>
                    {t.name || t.slug}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor={`${id}-tree`} className="text-xs">
              Tree
            </Label>
            <Input
              id={`${id}-tree`}
              value={tree}
              onChange={(e) => setTree(e.target.value)}
              placeholder="any tree"
              className="h-8 font-mono"
              autoComplete="off"
              spellCheck={false}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor={`${id}-path`} className="text-xs">
              Path filter
            </Label>
            <Input
              id={`${id}-path`}
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="src/**/*.go"
              className="h-8 font-mono"
              autoComplete="off"
              spellCheck={false}
            />
          </div>
        </div>
      </form>
      {search.data && term && !tooShort && search.data.hits.length > 0 && (
        <p role="status" className="mb-3 text-sm text-muted-foreground">
          <span className="font-semibold text-foreground tabular-nums">{formatNumber(search.data.hits.length)}</span>{" "}
          {search.data.hits.length === 1 ? "result" : "results"} in{" "}
          <span className="font-semibold text-foreground tabular-nums">{formatNumber(fileCount)}</span> {fileCount === 1 ? "file" : "files"} ·
          searched {formatNumber(search.data.searched_trees)} {search.data.searched_trees === 1 ? "tree" : "trees"}
        </p>
      )}
      {search.data?.truncated && term && (
        <Alert className="mb-3" aria-label="Results truncated">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>Results truncated</AlertTitle>
          <AlertDescription>Only the first results are shown. Narrow the search with a tenant, tree or path filter.</AlertDescription>
        </Alert>
      )}
      <div className="rounded-xl border bg-card">{body}</div>
    </>
  );
}
