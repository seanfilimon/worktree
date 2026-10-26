#!/usr/bin/env node
/**
 * Internal link checker for the website.
 *
 * Builds the set of routes from app/ (static pages) and content/ (MDX
 * collections), collects heading anchors the same way Fumadocs does
 * (github-slugger over the heading text), then verifies every internal link
 * in MDX content, TSX sources and the redirect table. Exits 1 on any broken
 * link so it can gate `npm run lint`.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import GithubSlugger from "github-slugger";
import { legacyDocRedirects } from "../redirects.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));

/** Recursively list files under `dir` whose name matches `pattern`. */
function walk(dir, pattern) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) out.push(...walk(full, pattern));
    else if (pattern.test(name)) out.push(full);
  }
  return out;
}

const toPosix = (p) => p.split(sep).join("/");

/** Remove fenced code blocks so their contents are not parsed as headings or links. */
function stripCodeFences(source) {
  return source.replace(/^(```|~~~)[^\n]*\n[\s\S]*?^\1[ \t]*$/gm, "");
}

/** Anchors Fumadocs generates for the headings of an MDX document. */
function headingAnchors(source) {
  const slugger = new GithubSlugger();
  const anchors = new Set();
  for (const match of stripCodeFences(source).matchAll(/^#{1,6}[ \t]+(.+?)[ \t]*#*[ \t]*$/gm)) {
    let text = match[1];
    const custom = text.match(/\[#([^\]]+)\]$/);
    if (custom) {
      anchors.add(custom[1]);
      continue;
    }
    text = text
      .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1") // links and images -> label
      .replace(/`([^`]*)`/g, "$1") // inline code -> content
      .replace(/<[^>]+>/g, "") // inline JSX/HTML tags
      .replace(/(\*\*|__|\*|_|~~)(.+?)\1/g, "$2"); // emphasis
    anchors.add(slugger.slug(text));
  }
  return anchors;
}

/** route -> { anchors: Set<string> | null (null = anchors not checked), file } */
const routes = new Map();

function addRoute(route, anchors, file) {
  routes.set(route === "" ? "/" : route, { anchors, file });
}

// Static app routes (dynamic segments are covered by content collections).
for (const file of walk(join(root, "app"), /^page\.tsx$/)) {
  const rel = toPosix(relative(join(root, "app"), file))
    .replace(/\/?page\.tsx$/, "")
    // Route groups such as (site) do not appear in URLs.
    .split("/")
    .filter((segment) => !/^\(.*\)$/.test(segment))
    .join("/");
  if (rel.includes("[")) continue;
  const source = readFileSync(file, "utf8");
  const ids = new Set(
    [...source.matchAll(/\bid(?:=|:\s*)["'`]([\w-]+)["'`]/g)].map((m) => m[1]),
  );
  addRoute(`/${rel}`, ids, file);
}

// Content collections.
const collections = [
  ["content/docs", "/docs"],
  ["content/guides", "/guides"],
  ["content/maintainers", "/maintainers"],
  ["content/articles", "/articles"],
];
for (const [dir, base] of collections) {
  addRoute(base, null, null);
  for (const file of walk(join(root, dir), /\.mdx$/)) {
    const rel = toPosix(relative(join(root, dir), file))
      .replace(/\.mdx$/, "")
      .replace(/(^|\/)index$/, "");
    const route = rel ? `${base}/${rel}` : base;
    addRoute(route, headingAnchors(readFileSync(file, "utf8")), file);
  }
}

const failures = [];
let checked = 0;

function check(href, from) {
  if (!href.startsWith("/") || href.startsWith("//")) return;
  if (/^\/(api|_next)\//.test(href) || /\.[a-z0-9]+$/i.test(href.split("#")[0])) return;
  checked++;
  const [pathAndQuery, anchor] = href.split("#");
  const path = pathAndQuery.split("?")[0].replace(/\/$/, "") || "/";
  const target = routes.get(path);
  if (!target) {
    failures.push(`${from}: broken link ${href}`);
    return;
  }
  if (anchor && target.anchors && !target.anchors.has(anchor)) {
    failures.push(`${from}: missing anchor #${anchor} on ${path}`);
  }
}

// Links inside MDX content (markdown links and JSX href props).
for (const [dir] of collections) {
  for (const file of walk(join(root, dir), /\.mdx$/)) {
    const source = stripCodeFences(readFileSync(file, "utf8")).replace(/`[^`\n]*`/g, "");
    const from = toPosix(relative(root, file));
    for (const m of source.matchAll(/\]\((\/[^)\s]*)(?:\s+"[^"]*")?\)/g)) check(m[1], from);
    for (const m of source.matchAll(/\bhref=["'](\/[^"']*)["']/g)) check(m[1], from);
  }
}

// Links in TSX sources (JSX href props and `href: "/..."` data entries).
for (const dir of ["app", "components", "lib"]) {
  for (const file of walk(join(root, dir), /\.tsx?$/)) {
    const source = readFileSync(file, "utf8");
    const from = toPosix(relative(root, file));
    for (const m of source.matchAll(/\bhref(?:=|:\s*)["'](\/[^"']*)["']/g)) check(m[1], from);
    for (const m of source.matchAll(/\burl:\s*["'](\/[^"']*)["']/g)) check(m[1], from);
  }
}

// Redirect destinations must exist; sources must not shadow live pages.
for (const { source, destination } of legacyDocRedirects) {
  check(destination, `redirects.mjs (${source})`);
  if (!source.includes(":") && routes.has(source)) {
    failures.push(`redirects.mjs: ${source} redirects away from an existing page`);
  }
}

if (failures.length > 0) {
  console.error(failures.join("\n"));
  console.error(`\n${failures.length} broken internal link(s).`);
  process.exit(1);
}
console.log(`check-links: ${checked} internal links across ${routes.size} routes resolve.`);
