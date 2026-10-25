/**
 * A small CommonMark-ish parser producing a plain AST that is rendered with
 * React elements (never innerHTML), so file content cannot inject markup.
 * Supports headings, paragraphs, fenced/indented code, block quotes, nested
 * lists (incl. task items), GFM tables, rules, and inline code, emphasis,
 * strike-through, links, images and autolinks. Raw HTML is shown as text.
 */

export type Inline =
  | { type: "text"; text: string }
  | { type: "code"; text: string }
  | { type: "strong" | "em" | "del"; children: Inline[] }
  | { type: "link"; href: string; children: Inline[] }
  | { type: "image"; src: string; alt: string }
  | { type: "break" };

export type Align = "left" | "center" | "right" | null;

export type Block =
  | { type: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; content: Inline[]; id: string }
  | { type: "paragraph"; content: Inline[] }
  | { type: "code"; lang: string; text: string }
  | { type: "quote"; children: Block[] }
  | { type: "list"; ordered: boolean; start: number; items: { checked: boolean | null; children: Block[] }[] }
  | { type: "table"; align: Align[]; head: Inline[][]; rows: Inline[][][] }
  | { type: "rule" };

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^`\s]*)/;
const HEADING = /^ {0,3}(#{1,6})(?:\s+(.*?))?(?:\s+#+)?\s*$/;
const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const QUOTE = /^ {0,3}> ?/;
const LIST_ITEM = /^( {0,3})([-*+]|\d{1,9}[.)])(\s+|$)/;
const TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

/** GitHub-style heading anchor: "Getting Started!" -> "getting-started". */
export function slugify(text: string): string {
  return text
    .toLowerCase()
    .trim()
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .replace(/\s+/g, "-");
}

function plainText(nodes: Inline[]): string {
  return nodes
    .map((n) =>
      n.type === "text" || n.type === "code" ? n.text : n.type === "image" ? n.alt : "children" in n ? plainText(n.children) : " ",
    )
    .join("");
}

function splitRow(line: string): string[] {
  let s = line.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  return s.split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, "|"));
}

function startsBlock(line: string): boolean {
  return FENCE.test(line) || HEADING.test(line) || RULE.test(line) || QUOTE.test(line) || LIST_ITEM.test(line);
}

const indentOf = (line: string) => line.length - line.trimStart().length;

export function parseMarkdown(source: string): Block[] {
  const slugs = new Map<string, number>();
  const uniqueSlug = (text: string) => {
    const base = slugify(text) || "section";
    const n = slugs.get(base) ?? 0;
    slugs.set(base, n + 1);
    return n === 0 ? base : `${base}-${n}`;
  };
  return parseBlocks(source.replace(/\r\n?/g, "\n").split("\n"), uniqueSlug);
}

function parseBlocks(lines: string[], uniqueSlug: (text: string) => string): Block[] {
  const blocks: Block[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i] ?? "";
    if (!line.trim()) {
      i++;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const marker = fence[1] ?? "```";
      const body: string[] = [];
      i++;
      while (i < lines.length && !(lines[i] ?? "").trim().startsWith(marker)) body.push(lines[i++] ?? "");
      i++; // closing fence (or end of input)
      blocks.push({ type: "code", lang: fence[2] ?? "", text: body.join("\n") });
      continue;
    }

    if (indentOf(line) >= 4) {
      const body: string[] = [];
      while (i < lines.length && (indentOf(lines[i] ?? "") >= 4 || !(lines[i] ?? "").trim())) body.push((lines[i++] ?? "").slice(4));
      while (body.length && !body[body.length - 1]?.trim()) body.pop();
      blocks.push({ type: "code", lang: "", text: body.join("\n") });
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      const content = parseInline(heading[2] ?? "");
      blocks.push({
        type: "heading",
        level: (heading[1] ?? "#").length as 1 | 2 | 3 | 4 | 5 | 6,
        content,
        id: uniqueSlug(plainText(content)),
      });
      i++;
      continue;
    }

    if (RULE.test(line)) {
      blocks.push({ type: "rule" });
      i++;
      continue;
    }

    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && (lines[i] ?? "").trim() && (QUOTE.test(lines[i] ?? "") || !startsBlock(lines[i] ?? ""))) {
        body.push((lines[i] ?? "").replace(QUOTE, ""));
        i++;
      }
      blocks.push({ type: "quote", children: parseBlocks(body, uniqueSlug) });
      continue;
    }

    const item = LIST_ITEM.exec(line);
    if (item) {
      const ordered = /\d/.test(item[2] ?? "");
      const start = ordered ? Number.parseInt(item[2] ?? "1", 10) : 1;
      const items: { checked: boolean | null; children: Block[] }[] = [];
      while (i < lines.length) {
        const m = LIST_ITEM.exec(lines[i] ?? "");
        if (!m || /\d/.test(m[2] ?? "") !== ordered) break;
        const width = m[0].length;
        const body = [(lines[i] ?? "").slice(width)];
        i++;
        while (i < lines.length) {
          const next = lines[i] ?? "";
          if (!next.trim()) {
            // A blank line continues the item only if indented content follows.
            const after = lines[i + 1] ?? "";
            if (after.trim() && indentOf(after) >= width) {
              body.push("");
              i++;
              continue;
            }
            break;
          }
          if (indentOf(next) >= width) body.push(next.slice(width));
          else if (!startsBlock(next)) body.push(next.trimStart());
          else break;
          i++;
        }
        let checked: boolean | null = null;
        const task = /^\[([ xX])\]\s+/.exec(body[0] ?? "");
        if (task) {
          checked = task[1] !== " ";
          body[0] = (body[0] ?? "").slice(task[0].length);
        }
        items.push({ checked, children: parseBlocks(body, uniqueSlug) });
        if (!(lines[i] ?? "").trim() && LIST_ITEM.test(lines[i + 1] ?? "")) i++;
      }
      blocks.push({ type: "list", ordered, start, items });
      continue;
    }

    if (line.includes("|") && TABLE_SEP.test(lines[i + 1] ?? "") && (lines[i + 1] ?? "").includes("-")) {
      const head = splitRow(line);
      const align: Align[] = splitRow(lines[i + 1] ?? "").map((c) =>
        c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : c.startsWith(":") ? "left" : null,
      );
      i += 2;
      const rows: Inline[][][] = [];
      while (i < lines.length && (lines[i] ?? "").includes("|") && (lines[i] ?? "").trim()) {
        const cells = splitRow(lines[i] ?? "");
        rows.push(head.map((_, c) => parseInline(cells[c] ?? "")));
        i++;
      }
      blocks.push({ type: "table", align, head: head.map((h) => parseInline(h)), rows });
      continue;
    }

    const para: string[] = [];
    while (i < lines.length && (lines[i] ?? "").trim() && (para.length === 0 || !startsBlock(lines[i] ?? ""))) {
      const next = lines[i + 1] ?? "";
      // Setext headings: "Title\n====" / "Title\n----".
      if (/^ {0,3}(=+|-+)\s*$/.test(next) && para.length === 0) {
        const content = parseInline((lines[i] ?? "").trim());
        blocks.push({ type: "heading", level: next.trim().startsWith("=") ? 1 : 2, content, id: uniqueSlug(plainText(content)) });
        i += 2;
        break;
      }
      para.push(lines[i++] ?? "");
    }
    if (para.length) blocks.push({ type: "paragraph", content: parseInline(para.join("\n")) });
  }
  return blocks;
}

/** Link destination: no spaces; one level of balanced parentheses allowed. */
const DEST = String.raw`\(\s*<?((?:[^()\s<>]|\([^()\s]*\))*)>?(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)`;
const LINK = new RegExp(String.raw`^\[((?:[^[\]\\]|\\.|\[[^\]]*\])*)\]` + DEST);
const IMAGE = new RegExp(String.raw`^!\[([^\]]*)\]` + DEST);
const AUTOLINK = /^<((?:https?:\/\/|mailto:)[^\s>]+)>/i;
const BARE_URL = /^https?:\/\/[^\s<]*[^\s<.,;:!?)\]'"]/i;

/** Parses inline markup. Unmatched delimiters are kept as text. */
export function parseInline(src: string): Inline[] {
  const out: Inline[] = [];
  let text = "";
  const flush = () => {
    if (text) out.push({ type: "text", text });
    text = "";
  };
  let i = 0;
  while (i < src.length) {
    const c = src.charAt(i);
    const rest = src.slice(i);

    if (c === "\\" && i + 1 < src.length) {
      const next = src.charAt(i + 1);
      if (next === "\n") {
        flush();
        out.push({ type: "break" });
      } else if (/[!-/:-@[-`{-~]/.test(next)) text += next;
      else text += c + next;
      i += 2;
      continue;
    }
    if (c === "\n") {
      if (text.endsWith("  ")) {
        text = text.trimEnd();
        flush();
        out.push({ type: "break" });
      } else text += " ";
      i++;
      continue;
    }
    if (c === "`") {
      const ticks = /^`+/.exec(rest)?.[0] ?? "`";
      const end = src.indexOf(ticks, i + ticks.length);
      if (end !== -1) {
        flush();
        out.push({
          type: "code",
          text:
            src
              .slice(i + ticks.length, end)
              .replace(/\n/g, " ")
              .trim() || " ",
        });
        i = end + ticks.length;
        continue;
      }
      text += ticks;
      i += ticks.length;
      continue;
    }
    if (c === "!") {
      const m = IMAGE.exec(rest);
      if (m) {
        flush();
        out.push({ type: "image", alt: m[1] ?? "", src: m[2] ?? "" });
        i += m[0].length;
        continue;
      }
    }
    if (c === "[") {
      const m = LINK.exec(rest);
      if (m) {
        flush();
        out.push({ type: "link", href: m[2] ?? "", children: parseInline(m[1] ?? "") });
        i += m[0].length;
        continue;
      }
    }
    if (c === "<") {
      const m = AUTOLINK.exec(rest);
      if (m) {
        flush();
        out.push({ type: "link", href: m[1] ?? "", children: [{ type: "text", text: (m[1] ?? "").replace(/^mailto:/i, "") }] });
        i += m[0].length;
        continue;
      }
    }
    if ((c === "h" || c === "H") && !/[\w/]/.test(src.charAt(i - 1))) {
      const m = BARE_URL.exec(rest);
      if (m) {
        flush();
        out.push({ type: "link", href: m[0], children: [{ type: "text", text: m[0] }] });
        i += m[0].length;
        continue;
      }
    }
    if (c === "*" || c === "_" || c === "~") {
      const double = src.charAt(i + 1) === c;
      const delim = double ? c + c : c;
      const wordy = c === "_" && /\w/.test(src.charAt(i - 1));
      const opens = !wordy && !/\s/.test(src.charAt(i + delim.length)) && i + delim.length < src.length;
      if (opens && (c !== "~" || double)) {
        const end = findCloser(src, i + delim.length, delim);
        if (end !== -1) {
          flush();
          const children = parseInline(src.slice(i + delim.length, end));
          out.push({ type: c === "~" ? "del" : double ? "strong" : "em", children });
          i = end + delim.length;
          continue;
        }
      }
      text += delim;
      i += delim.length;
      continue;
    }
    text += c;
    i++;
  }
  flush();
  return out;
}

/** Position of a closing delimiter not preceded by whitespace, skipping code spans. */
function findCloser(src: string, from: number, delim: string): number {
  for (let j = from; j < src.length; j++) {
    const ch = src.charAt(j);
    if (ch === "\\") {
      j++;
      continue;
    }
    if (ch === "`") {
      const end = src.indexOf("`", j + 1);
      if (end === -1) return -1;
      j = end;
      continue;
    }
    if (src.startsWith(delim, j) && j > from && !/\s/.test(src.charAt(j - 1))) {
      // A single delimiter must not be half of a double one.
      if (delim.length === 1 && src.charAt(j + 1) === delim) {
        j++;
        continue;
      }
      if (delim === "_" && /\w/.test(src.charAt(j + 1))) continue;
      return j;
    }
  }
  return -1;
}

export type SafeHref = { kind: "external"; href: string } | { kind: "anchor"; href: string } | { kind: "relative"; path: string } | null;

/** Classifies a link target; dangerous schemes (javascript:, data:, ...) yield null. */
export function safeHref(href: string): SafeHref {
  const h = href.trim();
  if (!h) return null;
  if (h.startsWith("#")) return { kind: "anchor", href: h };
  if (/^(https?:|mailto:)/i.test(h)) return { kind: "external", href: h };
  if (/^[a-z][a-z0-9+.-]*:/i.test(h) || h.startsWith("//")) return null;
  return { kind: "relative", path: h };
}

/** Resolves a relative link against the directory of the current file (POSIX semantics, no escaping above root). */
export function resolveRelative(fromDir: string, target: string): string {
  const clean = target.split(/[?#]/, 1)[0] ?? "";
  const parts = clean.startsWith("/") ? [] : fromDir.split("/").filter(Boolean);
  for (const seg of clean.split("/")) {
    if (!seg || seg === ".") continue;
    if (seg === "..") parts.pop();
    else parts.push(decodeURIComponentSafe(seg));
  }
  return parts.join("/");
}

function decodeURIComponentSafe(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}
