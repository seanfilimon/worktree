import {
  Building2,
  Check,
  CircleAlert,
  FileSearch,
  FolderTree,
  GitBranch,
  Loader2,
  LogOut,
  User,
  type LucideIcon,
} from "lucide-react";
import { useCommandState } from "cmdk";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router";
import { errorMessage } from "@/api/client";
import { useSearch } from "@/api/queries";
import { MIN_CODE_QUERY } from "@/api/v06";
import { useAuth } from "@/auth/context";
import {
  Command,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/components/ui/command";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Kbd } from "@/components/ui/kbd";
import { useTheme } from "@/theme/theme";
import { filterCommand } from "./commandFilter";
import { visibleNavGroups } from "./nav";
import { paletteShortcut } from "./shortcuts";
import { THEME_OPTIONS } from "./themeOptions";
import { useDebouncedValue } from "./useDebouncedValue";
import type { Account, SearchResults } from "@/api/types";

const SEARCH_DEBOUNCE_MS = 150;

/** Same item / group sizing as shadcn's CommandDialog. */
const PALETTE_CLASSES =
  "**:data-[slot=command-input-wrapper]:h-12 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-muted-foreground [&_[cmdk-group]]:px-2 [&_[cmdk-group]:not([hidden])_~[cmdk-group]]:pt-0 [&_[cmdk-input-wrapper]_svg]:h-5 [&_[cmdk-input-wrapper]_svg]:w-5 [&_[cmdk-input]]:h-12 [&_[cmdk-item]]:px-2 [&_[cmdk-item]]:py-2.5 [&_[cmdk-item]_svg]:h-4 [&_[cmdk-item]_svg]:w-4";
const enc = encodeURIComponent;

interface ItemProps {
  /** Unique cmdk value (also matched against the query). */
  value: string;
  /** Extra text the query is matched against. */
  keywords?: string[];
  /** Server search results are already filtered: always show them. */
  forceMount?: boolean;
  icon: LucideIcon;
  hint?: ReactNode;
  onSelect: () => void;
  children: ReactNode;
}

function Item({ value, keywords, forceMount, icon: Icon, hint, onSelect, children }: ItemProps) {
  return (
    <CommandItem value={value} keywords={keywords} forceMount={forceMount} onSelect={onSelect}>
      <Icon aria-hidden="true" />
      <span className="min-w-0 flex-1 truncate">{children}</span>
      {hint && <CommandShortcut className="max-w-[45%] truncate tracking-normal">{hint}</CommandShortcut>}
    </CommandItem>
  );
}

/** cmdk value of the first server result in display order ("" if none). */
function firstResultValue(results: SearchResults, accounts: Account[]): string {
  const [tenant] = results.tenants;
  if (tenant) return `tenant:${tenant.slug}`;
  const [tree] = results.trees;
  if (tree) return `tree:${tree.tenant_slug}/${tree.tree.name}`;
  const [branch] = results.branches;
  if (branch) return `branch:${branch.tenant_slug}/${branch.tree}@${branch.branch.name}`;
  const [account] = accounts;
  return account ? `account:${account.username}` : "";
}

/** Shown when neither a local command nor a server result matches. */
function NoResults({ term, serverResults }: { term: string; serverResults: number }) {
  // Force-mounted server results are not part of cmdk's filtered count.
  const localMatches = useCommandState((s) => s.filtered.count);
  if (localMatches > 0 || serverResults > 0) return null;
  return (
    <p role="status" className="py-6 text-center text-sm text-muted-foreground">
      No results for “{term}”.
    </p>
  );
}

interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * ⌘K / Ctrl+K (or "/") command palette: navigation, theme and session
 * commands plus live server search over tenants, trees, branches and
 * (for server admins) accounts.
 */
export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const navigate = useNavigate();
  const { isAdmin, signOut } = useAuth();
  const { preference, setPreference } = useTheme();
  const [query, setQuery] = useState("");
  const term = query.trim();
  const debounced = useDebouncedValue(term, SEARCH_DEBOUNCE_MS);
  const search = useSearch(open ? debounced : "");

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const action = paletteShortcut(e);
      if (!action) return;
      e.preventDefault();
      const next = action === "toggle" ? !open : true;
      if (!next) setQuery("");
      onOpenChange(next);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onOpenChange]);

  const setOpen = (next: boolean) => {
    if (!next) setQuery("");
    onOpenChange(next);
  };
  const run = (action: () => void) => {
    setOpen(false);
    action();
  };
  const go = (to: string) => run(() => void navigate(to));

  const results = term && search.data ? search.data : undefined;
  const accounts = isAdmin ? (results?.accounts ?? []) : [];
  const searching = term !== "" && (debounced !== term || search.isFetching);
  const serverResults =
    (results?.tenants.length ?? 0) + (results?.trees.length ?? 0) + (results?.branches.length ?? 0) + accounts.length;

  // cmdk auto-selects the first match only when the query changes. Server
  // results arrive after that, so when the selection no longer points at a
  // rendered item (every command was filtered out), select the first result.
  const [selected, setSelected] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const firstResult = results ? firstResultValue(results, accounts) : "";
  // v0.6 (§14.9): "Search code for …" jumps to the code search page.
  const codeValue = term.length >= MIN_CODE_QUERY ? `code-search:${term}` : "";
  useEffect(() => {
    if (!firstResult) return;
    setSelected((current) => {
      const rendered =
        current &&
        !current.startsWith("code-search:") &&
        rootRef.current?.querySelector(`[cmdk-item][data-value="${CSS.escape(current)}"]`);
      return rendered ? current : firstResult;
    });
  }, [firstResult]);

  return (
    // shadcn's CommandDialog, composed here so the cmdk root gets a `label`
    // (it names the search input via aria-labelledby).
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="top-[12%] translate-y-0 overflow-hidden p-0 sm:max-w-xl" showCloseButton={false}>
        <DialogHeader className="sr-only">
          <DialogTitle>Command palette</DialogTitle>
          <DialogDescription>
            Jump to a page, run a command, or search tenants, trees, branches and accounts.
          </DialogDescription>
        </DialogHeader>
        <Command
          ref={rootRef}
          label="Search"
          filter={filterCommand}
          value={selected}
          onValueChange={setSelected}
          className={PALETTE_CLASSES}
        >
          <CommandInput value={query} onValueChange={setQuery} placeholder="Search or jump to…" />
          <CommandList className="max-h-[min(60vh,420px)]">
            {term !== "" && !searching && !search.isError && <NoResults term={term} serverResults={serverResults} />}

            <CommandGroup heading="Navigation">
              {visibleNavGroups(isAdmin).flatMap((g) =>
                g.items.map((item) => (
                  <Item
                    key={item.to}
                    value={`nav:${item.to}`}
                    keywords={[item.label, g.label, "go", ...(item.keywords ?? [])]}
                    icon={item.icon}
                    onSelect={() => go(item.to)}
                  >
                    {item.label}
                  </Item>
                )),
              )}
            </CommandGroup>
            <CommandSeparator />
            <CommandGroup heading="Theme">
              {THEME_OPTIONS.map((t) => (
                <Item
                  key={t.value}
                  value={`theme:${t.value}`}
                  keywords={[`${t.label} theme`, "appearance", "mode"]}
                  icon={t.icon}
                  hint={preference === t.value ? <Check aria-label="Current theme" className="size-4" /> : undefined}
                  onSelect={() => run(() => setPreference(t.value))}
                >
                  {t.label} theme
                </Item>
              ))}
            </CommandGroup>
            <CommandSeparator />
            <CommandGroup heading="Session">
              <Item
                value="session:sign-out"
                keywords={["Sign out", "log out", "logout"]}
                icon={LogOut}
                onSelect={() => run(() => void signOut())}
              >
                Sign out
              </Item>
            </CommandGroup>

            {results && results.tenants.length > 0 && (
              <CommandGroup heading="Tenants" forceMount>
                {results.tenants.map((t) => (
                  <Item
                    key={t.id}
                    value={`tenant:${t.slug}`}
                    forceMount
                    icon={Building2}
                    hint={t.slug}
                    onSelect={() => go(`/tenants/${enc(t.slug)}`)}
                  >
                    {t.name || t.slug}
                  </Item>
                ))}
              </CommandGroup>
            )}
            {results && results.trees.length > 0 && (
              <CommandGroup heading="Trees" forceMount>
                {results.trees.map(({ tree, tenant_slug }) => (
                  <Item
                    key={tree.id}
                    value={`tree:${tenant_slug}/${tree.name}`}
                    forceMount
                    icon={FolderTree}
                    hint={tree.description}
                    onSelect={() => go(`/trees/${enc(tenant_slug)}/${enc(tree.name)}`)}
                  >
                    <span className="font-mono text-[13px]">
                      {tenant_slug}/{tree.name}
                    </span>
                  </Item>
                ))}
              </CommandGroup>
            )}
            {results && results.branches.length > 0 && (
              <CommandGroup heading="Branches" forceMount>
                {results.branches.map(({ tenant_slug, tree, branch }) => (
                  <Item
                    key={`${tenant_slug}/${tree}@${branch.name}`}
                    value={`branch:${tenant_slug}/${tree}@${branch.name}`}
                    forceMount
                    icon={GitBranch}
                    hint={`${tenant_slug}/${tree}`}
                    onSelect={() => go(`/trees/${enc(tenant_slug)}/${enc(tree)}/history?branch=${enc(branch.name)}`)}
                  >
                    <span className="font-mono text-[13px]">{branch.name}</span>
                  </Item>
                ))}
              </CommandGroup>
            )}
            {accounts.length > 0 && (
              <CommandGroup heading="Accounts" forceMount>
                {accounts.map((a) => (
                  <Item
                    key={a.id}
                    value={`account:${a.username}`}
                    forceMount
                    icon={User}
                    hint={a.email}
                    onSelect={() => go("/accounts")}
                  >
                    {a.display_name ? `${a.display_name} (@${a.username})` : `@${a.username}`}
                  </Item>
                ))}
              </CommandGroup>
            )}

            {codeValue && (
              <CommandGroup heading="Code" forceMount>
                <Item
                  value={codeValue}
                  forceMount
                  icon={FileSearch}
                  hint="all trees"
                  onSelect={() => go(`/search/code?q=${enc(term)}`)}
                >
                  Search code for “<span className="font-mono">{term}</span>”
                </Item>
              </CommandGroup>
            )}

            {searching && (
              <div role="status" className="flex items-center gap-2 px-4 py-3 text-sm text-muted-foreground">
                <Loader2 aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
                Searching…
              </div>
            )}
            {term !== "" && search.isError && !searching && (
              <div role="alert" className="flex items-center gap-2 px-4 py-3 text-sm">
                <CircleAlert aria-hidden="true" className="size-4 shrink-0" />
                Search failed: {errorMessage(search.error)}
              </div>
            )}
          </CommandList>
          <div
            aria-hidden="true"
            className="hidden items-center gap-4 border-t px-3 py-2 text-xs text-muted-foreground sm:flex"
          >
            <span className="flex items-center gap-1">
              <Kbd>↑</Kbd>
              <Kbd>↓</Kbd> navigate
            </span>
            <span className="flex items-center gap-1">
              <Kbd>↵</Kbd> open
            </span>
            <span className="flex items-center gap-1">
              <Kbd>esc</Kbd> close
            </span>
          </div>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
