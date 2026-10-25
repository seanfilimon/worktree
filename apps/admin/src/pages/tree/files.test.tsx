import { act, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { detectLanguage, tokenize } from "@/lib/code";
import { licenseGlobMatch, spdxError } from "@/components/files/license";
import { parseMarkdown } from "@/components/files/markdown";
import { filterFiles } from "@/components/files/tree";
import { FakeEventSource } from "@/test/fakeEventSource";
import * as fx from "@/test/fixtures";
import * as ffx from "@/test/fixtures.files";
import { json, noContent } from "@/test/mockFetch";
import { renderApp, treeRoutes } from "@/test/renderApp";

const T = "/api/v1/trees/acme/web";

beforeAll(() => {
  // jsdom lacks the layout APIs Radix / cmdk touch.
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  const proto = Element.prototype as unknown as Record<string, unknown>;
  for (const method of ["scrollIntoView", "setPointerCapture", "releasePointerCapture"]) proto[method] ??= () => {};
  proto.hasPointerCapture ??= () => false;
});

const fileRoutes = (extra = {}) =>
  treeRoutes({
    [`GET ${T}/snapshots/snap-3`]: ffx.filesDetail,
    [`GET ${T}/blob/snap-3/src/app.ts`]: () => new Response(ffx.appSource),
    [`GET ${T}/blob/snap-3/README.md`]: () => new Response(ffx.readmeSource),
    [`GET ${T}/history`]: ffx.appHistory,
    ...extra,
  });

describe("lib/code", () => {
  it("detects languages and tokenizes keywords, strings and comments", () => {
    const ts = detectLanguage("src/app.ts");
    expect(ts?.name).toBe("TypeScript");
    expect(detectLanguage("Dockerfile")?.id).toBe("dockerfile");
    expect(detectLanguage("notes.unknown")).toBeNull();
    const [line1, line2] = tokenize(["const s = 'a // b'; // note", "/* open", "still */ return"], ts);
    expect(line1).toEqual([
      { kind: "keyword", text: "const" },
      { kind: "plain", text: " s = " },
      { kind: "string", text: "'a // b'" },
      { kind: "plain", text: "; " },
      { kind: "comment", text: "// note" },
    ]);
    expect(line2).toEqual([{ kind: "comment", text: "/* open" }]);
    expect(tokenize(["/* x", "still */ return"], ts)[1]).toEqual([
      { kind: "comment", text: "still */" },
      { kind: "plain", text: " " },
      { kind: "keyword", text: "return" },
    ]);
  });

  it("matches license globs like the server and validates SPDX expressions", () => {
    expect(licenseGlobMatch("vendor", "vendor/a/b.go")).toBe(true);
    expect(licenseGlobMatch("**/*.rs", "a.rs")).toBe(true);
    expect(licenseGlobMatch("**/*.rs", "src/x/a.rs")).toBe(true);
    expect(licenseGlobMatch("*.rs", "src/a.rs")).toBe(false);
    expect(licenseGlobMatch("services/*", "services/billing/main.rs")).toBe(true);
    expect(spdxError("MIT OR (Apache-2.0 WITH LLVM-exception)")).toBeUndefined();
    expect(spdxError("MIT or Apache-2.0")).toMatch(/upper case/);
    expect(spdxError("MIT AND")).toBeDefined();
  });

  it("ranks fuzzy file matches by base name", () => {
    const files = ffx.filesDetail.files ?? [];
    expect(filterFiles(files, "utl")[0]?.file.path).toBe("src/lib/util.ts");
    expect(filterFiles(files, "zzz")).toEqual([]);
  });

  it("parses markdown without accepting dangerous links", () => {
    const [h, p] = parseMarkdown("# Title\n\n[x](javascript:alert(1))");
    expect(h).toMatchObject({ type: "heading", level: 1, id: "title" });
    expect(p).toMatchObject({ type: "paragraph", content: [{ type: "link", href: "javascript:alert(1)" }] });
  });
});

describe("files tab", () => {
  it("filters and navigates the file tree and anchors code lines", async () => {
    const { user, router } = renderApp({ route: "/trees/acme/web/files", api: fileRoutes() });

    const nav = await screen.findByRole("navigation", { name: "File tree" });
    expect(await screen.findByRole("table", { name: "Files in root" })).toBeInTheDocument();
    const src = within(nav).getByRole("button", { name: "src" });
    expect(src).toHaveAttribute("aria-expanded", "false");
    expect(within(nav).queryByRole("link", { name: "app.ts" })).not.toBeInTheDocument();

    await user.click(src);
    expect(src).toHaveAttribute("aria-expanded", "true");
    await user.click(within(nav).getByRole("link", { name: "app.ts" }));
    await waitFor(() => expect(router.state.location.search).toBe("?path=src%2Fapp.ts"));
    expect(within(nav).getByRole("link", { name: "app.ts" })).toHaveAttribute("aria-current", "page");

    const code = await screen.findByRole("region", { name: "Contents of src/app.ts" });
    expect(within(code).getByText("const")).toHaveClass("font-bold");
    expect(within(code).getByText("// entry point")).toHaveClass("italic");
    expect(screen.getByText("3 lines")).toBeInTheDocument();
    expect(screen.getByText("TypeScript")).toBeInTheDocument();

    await user.click(within(code).getByRole("link", { name: "Line 2" }));
    await waitFor(() => expect(router.state.location.hash).toBe("#L2"));
    expect(router.state.location.search).toBe("?path=src%2Fapp.ts");
    expect(code.querySelector("#L2")).toHaveAttribute("data-highlighted");
    expect(code.querySelector("#L1")).not.toHaveAttribute("data-highlighted");

    await user.type(within(nav).getByRole("searchbox", { name: "Filter files" }), "utl");
    const results = within(nav).getByRole("list", { name: "Matching files" });
    expect(within(results).getAllByRole("link")[0]).toHaveTextContent("src/lib/util.ts");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(router.state.location.search).toBe("?path=src%2Flib%2Futil.ts"));
  });

  it("highlights a line range from the URL and honours ?snapshot=", async () => {
    const { fetchMock } = renderApp({ route: "/trees/acme/web/files?snapshot=snap-3&path=src/app.ts#L1-L2", api: fileRoutes() });
    const code = await screen.findByRole("region", { name: "Contents of src/app.ts" });
    expect(code.querySelector("#L1")).toHaveAttribute("data-highlighted");
    expect(code.querySelector("#L2")).toHaveAttribute("data-highlighted");
    expect(code.querySelector("#L3")).not.toHaveAttribute("data-highlighted");
    expect(screen.getByText(/Pinned to snapshot/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Show branch tip" })).toHaveAttribute("href", "/trees/acme/web/files?path=src%2Fapp.ts");
    expect(fetchMock.find(`GET ${T}/blob/snap-3/src/app.ts`)[0]?.headers.get("Authorization")).toBe("Bearer test-token");
  });

  it("lists the snapshots that touched a file", async () => {
    const { user, fetchMock } = renderApp({ route: "/trees/acme/web/files?path=src/app.ts", api: fileRoutes() });
    await screen.findByRole("region", { name: "Contents of src/app.ts" });
    await user.click(screen.getByRole("tab", { name: "History" }));

    const list = await screen.findByRole("list", { name: "History of src/app.ts" });
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(within(items[0]!).getByText("modified")).toBeInTheDocument();
    expect(within(items[0]!).getByText(/viewing/)).toBeInTheDocument();
    expect(within(items[1]!).getByText("added")).toBeInTheDocument();
    expect(within(items[1]!).getByRole("link", { name: "Initial import" })).toHaveAttribute("href", "/trees/acme/web/snapshots/snap-1");
    expect(within(items[1]!).getByRole("link", { name: "View file" })).toHaveAttribute(
      "href",
      "/trees/acme/web/files?snapshot=snap-1&path=src%2Fapp.ts",
    );
    const q = fetchMock.find(`GET ${T}/history`)[0]?.url.searchParams;
    expect([q?.get("path"), q?.get("branch")]).toEqual(["src/app.ts", "main"]);
  });

  it("renders markdown safely with a preview tab", async () => {
    renderApp({ route: "/trees/acme/web/files?path=README.md", api: fileRoutes() });
    expect(await screen.findByRole("heading", { name: "Hello", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("link", { name: "guide" })).toHaveAttribute("href", "/trees/acme/web/files?path=docs%2Fguide.md");
    expect(screen.queryByRole("link", { name: "click me" })).not.toBeInTheDocument();
    expect(screen.getByText(/<script>alert\(1\)<\/script>/)).toBeInTheDocument();
    expect(document.querySelector("article script")).toBeNull();
    expect(screen.getByRole("img", { name: "Done" })).toBeInTheDocument();
  });

  it("previews images through a blob URL", async () => {
    const createObjectURL = vi.fn(() => "blob:logo");
    Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() }); // jsdom has no blob URLs
    renderApp({
      route: "/trees/acme/web/files?path=assets/logo.png",
      api: fileRoutes({ [`GET ${T}/blob/snap-3/assets/logo.png`]: () => new Response(new Uint8Array([0x89, 0x50, 0x4e, 0x47])) }),
    });
    expect(await screen.findByRole("img", { name: "logo.png" })).toHaveAttribute("src", "blob:logo");
    expect(screen.queryByRole("tab", { name: "Code" })).not.toBeInTheDocument();
    expect(createObjectURL).toHaveBeenCalledWith(expect.any(Blob));
  });

  it("switches branches with the branch combobox", async () => {
    const { user, router } = renderApp({
      route: "/trees/acme/web/files?path=src/app.ts",
      api: fileRoutes({
        [`GET ${T}/snapshots/snap-4`]: {
          ...ffx.filesDetail,
          snapshot: { ...ffx.filesDetail.snapshot, id: "snap-4", branch_name: "feature/login" },
        },
      }),
    });
    await screen.findByRole("region", { name: "Contents of src/app.ts" });
    await user.click(screen.getByRole("combobox", { name: "Branch" }));
    await user.type(screen.getByRole("combobox", { name: "Find a branch" }), "login");
    expect(screen.queryByRole("option", { name: /^main/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: /feature\/login/ }));
    await waitFor(() => expect(router.state.location.search).toBe("?branch=feature%2Flogin&path=src%2Fapp.ts"));
    expect(await screen.findByRole("combobox", { name: "Branch" })).toHaveTextContent("feature/login");
  });

  it("shows a placeholder for binary files", async () => {
    renderApp({
      route: "/trees/acme/web/files?path=scripts/build.sh",
      api: fileRoutes({ [`GET ${T}/blob/snap-3/scripts/build.sh`]: () => new Response(new Uint8Array([0x7f, 0x45, 0x00, 0x01])) }),
    });
    expect(await screen.findByText("Binary file")).toBeInTheDocument();
    expect(screen.getByText("executable")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeEnabled();
  });
});

describe("staged tab", () => {
  it("emphasizes files staged by several authors and refreshes live", async () => {
    const { fetchMock } = renderApp({
      route: "/trees/acme/web/staged",
      api: treeRoutes({
        [`GET ${T}/staged`]: () => json(ffx.stagedWithOverlap),
        [`GET ${T}/staged/summary`]: () => json(ffx.stagedSummary),
      }),
    });

    const overlaps = await screen.findByRole("list", { name: "Overlapping files" });
    expect(within(overlaps).getByText("src/auth.ts")).toHaveClass("font-bold");
    expect(within(overlaps).getByText(/bob, carol/)).toBeInTheDocument();

    const cards = screen.getByRole("list", { name: "Staged activity by branch" });
    const authors = within(cards).getByRole("list", { name: "Authors on feature/login" });
    expect(within(authors).getAllByRole("listitem")).toHaveLength(2);
    expect(within(cards).getByText("1 overlapping file")).toBeInTheDocument();

    const bobFiles = await screen.findByRole("list", { name: "Files staged by bob on feature/login" });
    expect(within(bobFiles).getByText("src/auth.ts").closest("li")).toHaveClass("font-bold");
    expect(within(bobFiles).getByText("src/login.tsx").closest("li")).not.toHaveClass("font-bold");

    const before = fetchMock.find(`GET ${T}/staged/summary`).length;
    act(() => {
      const es = FakeEventSource.latest();
      es.open();
      es.emit("snapshot.staged", {
        type: "snapshot.staged",
        tenant_id: fx.tenant.id,
        tree_id: "tree-web",
        actor: "carol",
        at: new Date().toISOString(),
      });
    });
    await waitFor(() => expect(fetchMock.find(`GET ${T}/staged/summary`).length).toBeGreaterThan(before));
  });
});

describe("protections tab", () => {
  it("previews matching branches and saves the rule", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/protections",
      api: treeRoutes({ [`PUT ${T}/protections`]: (req) => json(req.body) }),
    });
    await user.click(await screen.findByRole("button", { name: "Add protection" }));
    const form = await screen.findByRole("form", { name: "Add protection" });

    await user.type(within(form).getByLabelText("Branch pattern"), "feature/*");
    const matches = within(form).getByRole("list", { name: "Matching branches" });
    expect(within(matches).getByText("feature/login")).toBeInTheDocument();
    expect(within(matches).queryByText("main")).not.toBeInTheDocument();

    await user.clear(within(form).getByLabelText("Required approvals"));
    await user.type(within(form).getByLabelText("Required approvals"), "3");
    await user.type(within(form).getByLabelText("Allowed pushers"), "alice{Enter}bob,");
    expect(within(form).getByRole("button", { name: "Remove bob" })).toBeInTheDocument();
    await user.click(within(form).getByRole("switch", { name: "Prevent deletion" }));
    await user.click(screen.getByRole("button", { name: "Add protection" }));

    await waitFor(() => expect(fetchMock.find(`PUT ${T}/protections`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/protections`)[0]?.body).toEqual({
      tree_id: "tree-web",
      pattern: "feature/*",
      no_direct_push: true,
      required_approvals: 3,
      allowed_pushers: ["alice", "bob"],
      no_delete: false,
    });
  });

  it("rejects malformed patterns", async () => {
    const { user, fetchMock } = renderApp({ route: "/trees/acme/web/protections", api: treeRoutes() });
    await user.click(await screen.findByRole("button", { name: "Add protection" }));
    const form = await screen.findByRole("form", { name: "Add protection" });
    await user.type(within(form).getByLabelText("Branch pattern"), "release/[[1");
    await user.click(screen.getByRole("button", { name: "Add protection" }));
    expect(await within(form).findByText("Unclosed [ in pattern.")).toBeInTheDocument();
    expect(fetchMock.find(`PUT ${T}/protections`)).toHaveLength(0);
  });
});

describe("licenses tab", () => {
  it("previews covered files and submits the rule", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/licenses",
      api: treeRoutes({
        [`GET ${T}/licenses`]: [ffx.licenseRule],
        [`GET ${T}/snapshots/snap-3`]: ffx.filesDetail,
        [`PUT ${T}/licenses`]: (req) => json(req.body),
      }),
    });
    const table = await screen.findByRole("table", { name: "License rules" });
    expect(within(table).getByText("vendor/**")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Add rule" }));
    const form = await screen.findByRole("form", { name: "Add license rule" });
    await user.type(within(form).getByLabelText("Path glob"), "src/**");
    const matches = await within(form).findByRole("list", { name: "Matching files" });
    expect(
      within(matches)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(["src/app.ts", "src/lib/util.ts"]);

    await user.click(within(form).getByRole("combobox", { name: "SPDX license" }));
    await user.type(screen.getByRole("combobox", { name: "Search licenses" }), "apache");
    await user.click(await screen.findByRole("option", { name: /Apache-2\.0/ }));
    expect(within(form).getByRole("combobox", { name: "SPDX license" })).toHaveTextContent("Apache-2.0");
    await user.click(within(form).getByRole("radio", { name: /Modify/ }));
    await user.click(screen.getByRole("button", { name: "Add rule" }));

    await waitFor(() => expect(fetchMock.find(`PUT ${T}/licenses`)).toHaveLength(1));
    expect(fetchMock.find(`PUT ${T}/licenses`)[0]?.body).toEqual({
      tree_id: "tree-web",
      path_glob: "src/**",
      spdx: "Apache-2.0",
      grant: "modify",
    });
  });
});

describe("tree settings", () => {
  it("only deletes the tree after its exact name is typed", async () => {
    const { user, fetchMock, router } = renderApp({
      route: "/trees/acme/web/settings",
      api: treeRoutes({ [`DELETE ${T}`]: noContent() }),
    });
    await user.click(await screen.findByRole("button", { name: "Delete tree" }));
    const dialog = await screen.findByRole("alertdialog");
    const confirm = within(dialog).getByRole("button", { name: "Delete this tree" });
    const input = within(dialog).getByLabelText(/to confirm/);
    expect(confirm).toBeDisabled();
    await user.type(input, "Web");
    expect(confirm).toBeDisabled();
    await user.clear(input);
    await user.type(input, "web");
    expect(confirm).toBeEnabled();
    await user.click(confirm);

    await waitFor(() => expect(fetchMock.find(`DELETE ${T}`)).toHaveLength(1));
    await waitFor(() => expect(router.state.location.pathname).toBe("/trees"));
  });

  it("saves only the changed settings", async () => {
    const { user, fetchMock } = renderApp({
      route: "/trees/acme/web/settings",
      api: treeRoutes({ [`PATCH ${T}`]: (req) => json({ ...fx.summary.tree, ...(req.body as object) }) }),
    });
    const description = await screen.findByLabelText("Description");
    expect(screen.queryByRole("region", { name: "Unsaved changes" })).not.toBeInTheDocument();
    await user.clear(description);
    await user.type(description, "New site");
    const bar = screen.getByRole("region", { name: "Unsaved changes" });
    await user.click(within(bar).getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(fetchMock.find(`PATCH ${T}`)[0]?.body).toEqual({ description: "New site" }));
  });
});
