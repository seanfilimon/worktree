import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import * as fx from "@/test/fixtures";
import * as v6 from "@/test/fixtures.v06";
import { apiError } from "@/test/mockFetch";
import { renderApp } from "@/test/renderApp";
import type { CodeIndexStatus, SymbolSearchResult } from "./searchApi";
import { branchSelection, groupSymbols, qualifiedName, setBranchSelection, treeRefOf } from "./symbols";

// Code search v2 (§15.9): symbol mode, kind filter, branch selector and the
// admin index status panel.

const symbolResults: SymbolSearchResult = {
  symbols: [
    {
      tenant: "acme",
      tree: "web",
      branch: "main",
      snapshot_id: "snap-3",
      path: "src/config.ts",
      line: 12,
      name: "parse",
      kind: "function",
      signature: "export function parse(input: string)",
      language: "typescript",
      match: "exact",
    },
    {
      tenant: "acme",
      tree: "web",
      branch: "feature/login",
      snapshot_id: "snap-4",
      path: "src/login.ts",
      line: 3,
      name: "parseToken",
      kind: "method",
      container: "Session",
      signature: "parseToken(raw: string): Token",
      language: "typescript",
      match: "prefix",
    },
    {
      tenant: "acme",
      tree: "api",
      branch: "main",
      snapshot_id: "snap-9",
      path: "server/parse.go",
      line: 7,
      name: "MustParse",
      kind: "function",
      signature: "func MustParse(s string) Config",
      language: "go",
      match: "substring",
    },
  ],
  truncated: true,
  searched_trees: 2,
};

const indexStatus: CodeIndexStatus = {
  format_version: 1,
  persistent: true,
  memory_bytes: 2048,
  memory_limit: 268435456,
  loaded_segments: 1,
  evictions: 0,
  disk_bytes: 8192,
  max_branches: 10,
  builds: 0,
  disk_loads: 1,
  trees: [
    {
      tree_id: "tree-web",
      tenant: "acme",
      tree: "web",
      default_branch: "main",
      state: "ready",
      files: 120,
      bytes: 4000,
      symbols: 830,
      trigrams: 9000,
      built_at: new Date().toISOString(),
      branch_count: 2,
      segments: 1,
      memory_bytes: 2048,
      disk_bytes: 8192,
      branches: [],
    },
  ],
};

describe("symbol search", () => {
  it("searches symbols from the URL with kind and branch scope", async () => {
    const { fetchMock } = renderApp({
      route: "/search/code?q=parse&kind=symbol&symbol_kind=function&branches=all",
      api: { "GET /api/v1/search/symbols": symbolResults },
    });
    const web = await screen.findByRole("region", { name: "Symbols in acme/web" });
    expect(within(web).getByRole("link", { name: "parse at src/config.ts line 12" })).toHaveAttribute(
      "href",
      "/trees/acme/web/files?snapshot=snap-3&path=src%2Fconfig.ts#L12",
    );
    expect(within(web).getByText("Session.", { exact: false })).toBeInTheDocument();
    expect(within(web).getByText("feature/login")).toBeInTheDocument(); // branch shown for branches=all
    expect(within(web).getAllByText("Function").length).toBeGreaterThan(0);
    expect(screen.getByRole("region", { name: "Symbols in acme/api" })).toHaveTextContent("server/parse.go:7");
    expect(screen.getByText(/searched 2 trees \(all branches\)/)).toBeInTheDocument();
    expect(screen.getByRole("alert", { name: "Results truncated" })).toHaveTextContent("Only the best matches are shown.");

    const q = fetchMock.find("GET /api/v1/search/symbols")[0]!.url.searchParams;
    expect(q.get("q")).toBe("parse");
    expect(q.get("kind")).toBe("function");
    expect(q.get("branches")).toBe("all");
    expect(q.has("branch")).toBe(false);
    expect(fetchMock.find("GET /api/v1/search/code")).toHaveLength(0);
  });

  it("switches to symbol mode, picks a kind and a branch of the chosen tree", async () => {
    const { user, router, fetchMock } = renderApp({
      route: "/search/code",
      api: {
        "GET /api/v1/search/symbols": symbolResults,
        "GET /api/v1/trees/acme/web/branches": fx.branches,
      },
    });
    const form = await screen.findByRole("search", { name: "Code search" });
    await user.click(within(form).getByRole("radio", { name: "Symbols" }));
    expect(within(form).queryByRole("button", { name: "Regular expression" })).not.toBeInTheDocument();
    await user.type(within(form).getByLabelText("Search symbols"), "p");
    await user.type(within(form).getByLabelText("Tree"), "acme/web");
    await waitFor(() => expect(fetchMock.find("GET /api/v1/trees/acme/web/branches")).toHaveLength(1));
    await user.click(within(form).getByRole("combobox", { name: "Kind" }));
    await user.click(await screen.findByRole("option", { name: "Method" }));
    await user.click(within(form).getByRole("combobox", { name: "Branch" }));
    await user.click(await screen.findByRole("option", { name: "feature/login" }));
    await user.click(within(form).getByRole("button", { name: "Search" }));
    await waitFor(() => expect(router.state.location.search).toBe("?q=p&kind=symbol&tree=acme%2Fweb&symbol_kind=method&branch=feature%2Flogin"));
    await waitFor(() => expect(fetchMock.find("GET /api/v1/search/symbols")).toHaveLength(1));
    const q = fetchMock.find("GET /api/v1/search/symbols")[0]!.url.searchParams;
    // A one-character symbol query is searched (code search needs three).
    expect([q.get("q"), q.get("kind"), q.get("tree"), q.get("branch")]).toEqual(["p", "method", "acme/web", "feature/login"]);
  });

  it("sends the branch scope with code searches", async () => {
    const { user, router, fetchMock } = renderApp({ route: "/search/code?q=greeting", api: { "GET /api/v1/search/code": v6.codeResults } });
    await screen.findByRole("region", { name: "Results in acme/web" });
    expect(fetchMock.find("GET /api/v1/search/code")[0]!.url.searchParams.has("branches")).toBe(false);
    const form = screen.getByRole("search", { name: "Code search" });
    await user.click(within(form).getByRole("combobox", { name: "Branch" }));
    await user.click(await screen.findByRole("option", { name: "All branches" }));
    await user.click(within(form).getByRole("button", { name: "Search" }));
    await waitFor(() => expect(router.state.location.search).toBe("?q=greeting&branches=all"));
    await waitFor(() => expect(fetchMock.find("GET /api/v1/search/code")).toHaveLength(2));
    expect(fetchMock.find("GET /api/v1/search/code")[1]!.url.searchParams.get("branches")).toBe("all");
  });

  it("explains when symbol search is not available", async () => {
    renderApp({ route: "/search/code?q=x&kind=symbol", api: { "GET /api/v1/search/symbols": apiError(501, "not_implemented", "no index") } });
    expect(await screen.findByText("Symbol search is not available on this server")).toBeInTheDocument();
  });
});

describe("code index panel", () => {
  it("shows the index status to admins on demand and rebuilds a tree", async () => {
    const { user, fetchMock } = renderApp({
      route: "/search/code",
      api: {
        "GET /api/v1/admin/code-index": indexStatus,
        "POST /api/v1/admin/code-index/rebuild": { rebuilt: 1, trees: indexStatus.trees },
      },
    });
    const toggle = await screen.findByRole("button", { name: "Index status" });
    expect(fetchMock.find("GET /api/v1/admin/code-index")).toHaveLength(0); // loads when opened
    await user.click(toggle);
    const table = await screen.findByRole("table", { name: "Indexed trees" });
    expect(within(table).getByText("acme/web")).toBeInTheDocument();
    expect(within(table).getByText("830")).toBeInTheDocument();
    expect(screen.getByText(/0 built, 1 loaded from disk since start/)).toBeInTheDocument();
    await user.click(within(table).getByRole("button", { name: "Rebuild acme/web" }));
    await waitFor(() => expect(fetchMock.find("POST /api/v1/admin/code-index/rebuild")).toHaveLength(1));
    expect(fetchMock.find("POST /api/v1/admin/code-index/rebuild")[0]!.url.searchParams.get("tree")).toBe("acme/web");
    expect(await screen.findByText("Code index rebuilt")).toBeInTheDocument();
  });

  it("is hidden from non-admins", async () => {
    renderApp({ route: "/search/code", api: { "GET /api/v1/auth/me": fx.memberMe } });
    await screen.findByText("Search code across your trees");
    expect(screen.queryByRole("button", { name: "Index status" })).not.toBeInTheDocument();
  });
});

describe("symbol helpers", () => {
  it("groups, names and encodes branch selections", () => {
    const groups = groupSymbols(symbolResults.symbols);
    expect(groups.map((g) => [g.tree, g.symbols.length])).toEqual([
      ["acme/web", 2],
      ["acme/api", 1],
    ]);
    expect(qualifiedName(symbolResults.symbols[1]!)).toBe("Session.parseToken");
    expect(qualifiedName(symbolResults.symbols[0]!)).toBe("parse");

    expect(branchSelection(new URLSearchParams("branches=all&branch=x"))).toBe("*");
    expect(branchSelection(new URLSearchParams("branch=dev"))).toBe("dev");
    expect(branchSelection(new URLSearchParams(""))).toBe("");
    const p = new URLSearchParams("q=x&branch=old");
    setBranchSelection(p, "*");
    expect(p.toString()).toBe("q=x&branches=all");
    setBranchSelection(p, "dev");
    expect(p.toString()).toBe("q=x&branch=dev");
    setBranchSelection(p, "");
    expect(p.toString()).toBe("q=x");

    expect(treeRefOf("", "acme/web")).toEqual({ tenant: "acme", tree: "web" });
    expect(treeRefOf("acme", "web")).toEqual({ tenant: "acme", tree: "web" });
    expect(treeRefOf("", "web")).toBeNull();
    expect(treeRefOf("acme", "")).toBeNull();
  });
});
