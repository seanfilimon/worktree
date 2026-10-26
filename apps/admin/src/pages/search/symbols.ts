import type { SymbolHit, SymbolKind } from "./searchApi";

export const SYMBOL_KIND_LABEL: Record<SymbolKind, string> = {
  function: "Function",
  method: "Method",
  class: "Class",
  struct: "Struct",
  interface: "Interface",
  trait: "Trait",
  enum: "Enum",
  type: "Type",
  const: "Constant",
};

/** `Container.name` for methods, the bare name otherwise. */
export function qualifiedName(s: Pick<SymbolHit, "name" | "container">): string {
  return s.container ? `${s.container}.${s.name}` : s.name;
}

/** Symbols grouped by tree in server (rank) order. */
export function groupSymbols(symbols: readonly SymbolHit[]): { tree: string; tenant: string; name: string; symbols: SymbolHit[] }[] {
  const groups = new Map<string, { tree: string; tenant: string; name: string; symbols: SymbolHit[] }>();
  for (const s of symbols) {
    const key = `${s.tenant}/${s.tree}`;
    let g = groups.get(key);
    if (!g) {
      g = { tree: key, tenant: s.tenant, name: s.tree, symbols: [] };
      groups.set(key, g);
    }
    g.symbols.push(s);
  }
  return [...groups.values()];
}

/** The branch selection encoded in the page URL: "" (default), "*" (all) or a name. */
export function branchSelection(params: URLSearchParams): string {
  if (params.get("branches") === "all") return "*";
  return params.get("branch") ?? "";
}

/** Writes a branch selection into URL parameters. */
export function setBranchSelection(params: URLSearchParams, selection: string): void {
  params.delete("branch");
  params.delete("branches");
  if (selection === "*") params.set("branches", "all");
  else if (selection) params.set("branch", selection);
}

/** Splits `TENANT/TREE` (or a tree name with a tenant) into its parts. */
export function treeRefOf(tenant: string, tree: string): { tenant: string; tree: string } | null {
  const t = tree.trim();
  const slash = t.indexOf("/");
  if (slash > 0 && slash < t.length - 1) return { tenant: t.slice(0, slash), tree: t.slice(slash + 1) };
  if (t && tenant) return { tenant, tree: t };
  return null;
}
