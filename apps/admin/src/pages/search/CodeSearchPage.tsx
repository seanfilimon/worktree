import { Braces, CaseSensitive, FileCode2, FileSearch, GitBranch, Loader2, Regex, Search, TextSearch, TriangleAlert } from "lucide-react";
import { useId, useMemo, useState, type FormEvent, type ReactNode } from "react";
import { Link, useSearchParams } from "react-router";
import { useTenants } from "@/api/queries";
import { isUnavailable, MIN_CODE_QUERY, type CodeSearchHit } from "@/api/v06";
import { useAuth } from "@/auth/context";
import { EmptyState, ErrorState, LoadingRows } from "@/components/files/states";
import { PageHeader } from "@/components/PageHeader";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { formatNumber, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { CodeIndexPanel } from "./CodeIndexPanel";
import { codeHitHref, groupHits, highlightParts } from "./codeSearch";
import {
  isSymbolKind,
  SYMBOL_KINDS,
  useCodeSearchV2,
  useSymbolSearch,
  useTreeBranchNames,
  type BranchScope,
  type CodeSearchV2Query,
  type SymbolHit,
  type SymbolKind,
  type SymbolQuery,
} from "./searchApi";
import { branchSelection, groupSymbols, qualifiedName, setBranchSelection, SYMBOL_KIND_LABEL, treeRefOf } from "./symbols";

const ALL = "__all__";
const DEFAULT_BRANCH = "__default__";
const ALL_BRANCHES = "__all_branches__";
const CONTEXT_LINES = 2;
const LIMIT = 200;

type Mode = "code" | "symbol";

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

function Highlighted({ text, q, regex, caseSensitive }: { text: string; q: string; regex: boolean; caseSensitive: boolean }) {
  return (
    <>
      {highlightParts(text, q, regex, caseSensitive).map((p, i) =>
        p.match ? (
          <mark key={i} className="rounded-[2px] bg-foreground px-0.5 font-bold text-background">
            {p.text}
          </mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
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
        <Highlighted text={hit.text} q={q} regex={regex} caseSensitive={caseSensitive} />
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

function SymbolRow({ s, q, showBranch }: { s: SymbolHit; q: string; showBranch: boolean }) {
  return (
    <li className="flex flex-col gap-1 px-3 py-2 sm:flex-row sm:items-baseline sm:gap-3">
      <Badge variant="outline" className="w-fit shrink-0 font-mono text-[11px]">
        {SYMBOL_KIND_LABEL[s.kind] ?? s.kind}
      </Badge>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <Link to={codeHitHref(s, s.line)} className="w-fit font-mono text-[13px] font-semibold hover:underline">
          {s.container && <span className="text-muted-foreground">{s.container}.</span>}
          <Highlighted text={s.name} q={q} regex={false} caseSensitive={false} />
        </Link>
        <code className="truncate font-mono text-xs text-muted-foreground" title={s.signature}>
          {s.signature}
        </code>
      </div>
      <span className="shrink-0 font-mono text-xs text-muted-foreground">
        {showBranch && (
          <span className="mr-2 inline-flex items-center gap-1">
            <GitBranch aria-hidden="true" className="size-3" />
            {s.branch}
          </span>
        )}
        <Link to={codeHitHref(s, s.line)} aria-label={`${qualifiedName(s)} at ${s.path} line ${s.line}`} className="hover:underline">
          {s.path}:{s.line}
        </Link>
      </span>
    </li>
  );
}

interface PageQuery extends BranchScope {
  mode: Mode;
  q: string;
  tenant?: string;
  tree?: string;
  path?: string;
  regex: boolean;
  case: boolean;
  kind?: SymbolKind;
}

function readQuery(params: URLSearchParams): PageQuery {
  const sel = branchSelection(params);
  const kind = params.get("symbol_kind");
  return {
    mode: params.get("kind") === "symbol" ? "symbol" : "code",
    q: params.get("q") ?? "",
    tenant: params.get("tenant") ?? undefined,
    tree: params.get("tree") ?? undefined,
    path: params.get("path") ?? undefined,
    regex: params.get("regex") === "1",
    case: params.get("case") === "1",
    kind: isSymbolKind(kind) ? kind : undefined,
    branch: sel && sel !== "*" ? sel : undefined,
    allBranches: sel === "*",
  };
}

function selectValueOf(sel: string): string {
  if (sel === "*") return ALL_BRANCHES;
  return sel || DEFAULT_BRANCH;
}

/** `/search/code`: search code (or symbol definitions) on the branches of every readable tree. */
export function CodeSearchPage() {
  const id = useId();
  const { isAdmin } = useAuth();
  const [params, setParams] = useSearchParams();
  const query = readQuery(params);
  const tenants = useTenants();
  const [mode, setMode] = useState<Mode>(query.mode);
  const [q, setQ] = useState(query.q);
  const [tenant, setTenant] = useState(query.tenant ?? "");
  const [tree, setTree] = useState(query.tree ?? "");
  const [path, setPath] = useState(query.path ?? "");
  const [regex, setRegex] = useState(query.regex);
  const [caseSensitive, setCaseSensitive] = useState(query.case);
  const [kind, setKind] = useState<SymbolKind | "">(query.kind ?? "");
  const [branchSel, setBranchSel] = useState(branchSelection(params));
  // Navigation to another search (e.g. from the command palette) resets the form.
  const [synced, setSynced] = useState(params.toString());
  if (synced !== params.toString()) {
    setSynced(params.toString());
    setMode(query.mode);
    setQ(query.q);
    setTenant(query.tenant ?? "");
    setTree(query.tree ?? "");
    setPath(query.path ?? "");
    setRegex(query.regex);
    setCaseSensitive(query.case);
    setKind(query.kind ?? "");
    setBranchSel(branchSelection(params));
  }
  const symbolMode = query.mode === "symbol";
  const scope: BranchScope = { branch: query.branch, allBranches: query.allBranches };
  const codeQuery: CodeSearchV2Query = {
    q: query.q,
    tenant: query.tenant,
    tree: query.tree,
    path: query.path,
    regex: query.regex,
    case: query.case,
    context: CONTEXT_LINES,
    limit: LIMIT,
    ...scope,
  };
  const symbolQuery: SymbolQuery = { q: query.q, tenant: query.tenant, tree: query.tree, path: query.path, kind: query.kind, limit: LIMIT, ...scope };
  const code = useCodeSearchV2(codeQuery, !symbolMode);
  const symbols = useSymbolSearch(symbolQuery, symbolMode);
  const search = symbolMode ? symbols : code;
  const formTree = treeRefOf(tenant, tree);
  const branchNames = useTreeBranchNames(formTree);

  const term = query.q.trim();
  const tooShort = !symbolMode && !query.regex && term.length > 0 && term.length < MIN_CODE_QUERY;
  const groups = useMemo(() => groupHits(code.data?.hits ?? []), [code.data]);
  const symbolGroups = useMemo(() => groupSymbols(symbols.data?.symbols ?? []), [symbols.data]);
  const fileCount = groups.reduce((n, g) => n + g.files.length, 0);
  const resultCount = symbolMode ? (symbols.data?.symbols.length ?? 0) : (code.data?.hits.length ?? 0);
  const showBranch = Boolean(query.branch || query.allBranches);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const next = new URLSearchParams();
    if (q.trim()) next.set("q", q.trim());
    if (mode === "symbol") next.set("kind", "symbol");
    if (tenant) next.set("tenant", tenant);
    if (tree.trim()) next.set("tree", tree.trim());
    if (path.trim()) next.set("path", path.trim());
    if (mode === "code" && regex) next.set("regex", "1");
    if (mode === "code" && caseSensitive) next.set("case", "1");
    if (mode === "symbol" && kind) next.set("symbol_kind", kind);
    setBranchSelection(next, branchSel);
    setParams(next);
  };

  const branchOptions = useMemo(() => {
    const names = new Set(branchNames.data ?? []);
    if (branchSel && branchSel !== "*") names.add(branchSel);
    return [...names].sort();
  }, [branchNames.data, branchSel]);

  let body: ReactNode;
  if (!term) {
    body = symbolMode ? (
      <EmptyState icon={Braces} title="Find symbol definitions">
        Functions, methods, types and constants in Go, Rust, TypeScript, JavaScript, Python, Java and C/C++. Exact names rank first,
        then prefixes, then substrings.
      </EmptyState>
    ) : (
      <EmptyState icon={FileSearch} title="Search code across your trees">
        Searches the tip of every tree&apos;s default branch you can read (or the branches you pick). Use at least {MIN_CODE_QUERY}{" "}
        characters, or a regular expression (RE2 syntax).
      </EmptyState>
    );
  } else if (tooShort) {
    body = (
      <EmptyState icon={Search} title="Query too short">
        Literal searches need at least {MIN_CODE_QUERY} characters.
      </EmptyState>
    );
  } else if (search.isPending) {
    body = <LoadingRows rows={8} label={symbolMode ? "Searching symbols" : "Searching code"} />;
  } else if (search.isError) {
    body = isUnavailable(search.error) ? (
      <div className="p-4">
        <UnavailableNotice feature={symbolMode ? "Symbol search" : "Code search"} />
      </div>
    ) : (
      <ErrorState error={search.error} onRetry={() => void search.refetch()} />
    );
  } else if (resultCount === 0) {
    body = (
      <EmptyState icon={Search} title={`No results for “${term}”`}>
        Searched {formatNumber(search.data.searched_trees)} {search.data.searched_trees === 1 ? "tree" : "trees"}. Binary files, files over
        1 MiB and paths you may not read are not searched.
      </EmptyState>
    );
  } else if (symbolMode) {
    body = (
      <div className="flex flex-col gap-6 p-4">
        {symbolGroups.map((g) => (
          <section key={g.tree} aria-label={`Symbols in ${g.tree}`} className="flex flex-col gap-3">
            <h2 className="font-mono text-sm font-semibold">
              <Link to={treePath(g.tenant, g.name)} className="hover:underline">
                {g.tree}
              </Link>
            </h2>
            <ul className="divide-y overflow-hidden rounded-lg border bg-card">
              {g.symbols.map((s) => (
                <SymbolRow key={`${s.branch}:${s.path}:${s.line}:${s.name}`} s={s} q={term} showBranch={showBranch} />
              ))}
            </ul>
          </section>
        ))}
      </div>
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
              {!showBranch && (
                <Badge variant="outline" className="font-mono">
                  {g.files[0]!.branch}
                </Badge>
              )}
            </h2>
            {g.files.map((f) => (
              <article key={`${f.branch}:${f.key}`} aria-label={f.path} className="overflow-hidden rounded-lg border bg-card">
                <header className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b bg-muted/40 px-3 py-2">
                  <FileCode2 aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
                  <Link to={codeHitHref(f)} className="min-w-0 flex-1 truncate font-mono text-[13px] font-semibold hover:underline">
                    {f.path}
                  </Link>
                  {showBranch && (
                    <Badge variant="outline" className="gap-1 font-mono">
                      <GitBranch aria-hidden="true" />
                      {f.branch}
                    </Badge>
                  )}
                  <span className="text-xs text-muted-foreground tabular-nums">
                    {f.hits.length} {f.hits.length === 1 ? "match" : "matches"} · <span className="font-mono">{shortId(f.snapshot_id)}</span>
                  </span>
                </header>
                <div className="overflow-x-auto font-mono text-[12.5px] leading-5">
                  <table className="w-full border-collapse">
                    {f.hits.map((h) => (
                      <HitBlock key={`${h.line}`} hit={h} q={term} regex={query.regex} caseSensitive={query.case} />
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

  const truncated = symbolMode ? symbols.data?.truncated : code.data?.truncated;

  return (
    <>
      <PageHeader
        title="Code search"
        breadcrumbs={[{ label: "Code search" }]}
        description="Find code and symbol definitions on the branches of every tree you can read."
      />
      <form onSubmit={submit} role="search" aria-label="Code search" className="mb-4 flex flex-col gap-3 rounded-xl border bg-card p-3">
        <div className="flex flex-wrap gap-2 sm:flex-nowrap">
          <ToggleGroup
            type="single"
            variant="outline"
            value={mode}
            onValueChange={(v) => v && setMode(v as Mode)}
            aria-label="Search mode"
            className="w-fit shrink-0"
          >
            <ToggleGroupItem value="code" aria-label="Code" className="data-[state=on]:bg-foreground data-[state=on]:text-background">
              <TextSearch aria-hidden="true" />
              <span className="hidden sm:inline">Code</span>
            </ToggleGroupItem>
            <ToggleGroupItem value="symbol" aria-label="Symbols" className="data-[state=on]:bg-foreground data-[state=on]:text-background">
              <Braces aria-hidden="true" />
              <span className="hidden sm:inline">Symbols</span>
            </ToggleGroupItem>
          </ToggleGroup>
          <Label htmlFor={`${id}-q`} className="sr-only">
            {mode === "symbol" ? "Search symbols" : "Search code"}
          </Label>
          <Input
            id={`${id}-q`}
            type="search"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={mode === "symbol" ? "parseConfig" : regex ? "func\\s+New[A-Z]\\w*" : "handleBlob"}
            className="min-w-0 flex-1 basis-48 font-mono"
            autoComplete="off"
            spellCheck={false}
            autoFocus
          />
          {mode === "code" && (
            <>
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
            </>
          )}
          <Button type="submit">
            {search.isFetching ? <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" /> : <Search aria-hidden="true" />}
            Search
          </Button>
        </div>
        <div className={`grid gap-3 sm:grid-cols-2 ${mode === "symbol" ? "lg:grid-cols-5" : "lg:grid-cols-4"}`}>
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
          <div className="grid gap-1.5">
            <Label htmlFor={`${id}-branch`} className="text-xs">
              Branch
            </Label>
            <Select
              value={selectValueOf(branchSel)}
              onValueChange={(v) => setBranchSel(v === DEFAULT_BRANCH ? "" : v === ALL_BRANCHES ? "*" : v)}
            >
              <SelectTrigger id={`${id}-branch`} size="sm" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={DEFAULT_BRANCH}>Default branch</SelectItem>
                <SelectItem value={ALL_BRANCHES}>All branches</SelectItem>
                {branchOptions.map((b) => (
                  <SelectItem key={b} value={b}>
                    {b}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {mode === "symbol" && (
            <div className="grid gap-1.5">
              <Label htmlFor={`${id}-kind`} className="text-xs">
                Kind
              </Label>
              <Select value={kind || ALL} onValueChange={(v) => setKind(v === ALL ? "" : (v as SymbolKind))}>
                <SelectTrigger id={`${id}-kind`} size="sm" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={ALL}>All kinds</SelectItem>
                  {SYMBOL_KINDS.map((k) => (
                    <SelectItem key={k} value={k}>
                      {SYMBOL_KIND_LABEL[k]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
        </div>
      </form>
      {search.data && term && !tooShort && resultCount > 0 && (
        <p role="status" className="mb-3 text-sm text-muted-foreground">
          <span className="font-semibold text-foreground tabular-nums">{formatNumber(resultCount)}</span>{" "}
          {symbolMode ? (resultCount === 1 ? "symbol" : "symbols") : resultCount === 1 ? "result" : "results"}
          {!symbolMode && (
            <>
              {" "}
              in <span className="font-semibold text-foreground tabular-nums">{formatNumber(fileCount)}</span> {fileCount === 1 ? "file" : "files"}
            </>
          )}{" "}
          · searched {formatNumber(search.data.searched_trees)} {search.data.searched_trees === 1 ? "tree" : "trees"}
          {query.allBranches ? " (all branches)" : query.branch ? ` (branch ${query.branch})` : ""}
        </p>
      )}
      {truncated && term && (
        <Alert className="mb-3" aria-label="Results truncated">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>Results truncated</AlertTitle>
          <AlertDescription>
            {symbolMode ? "Only the best matches are shown." : "Only the first results are shown."} Narrow the search with a tenant, tree or path
            filter.
          </AlertDescription>
        </Alert>
      )}
      <div className="rounded-xl border bg-card">{body}</div>
      {isAdmin && <CodeIndexPanel />}
    </>
  );
}
