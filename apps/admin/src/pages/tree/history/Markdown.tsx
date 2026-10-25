import type { ReactNode } from "react";

/**
 * Dependency-free renderer for the common Markdown subset found in READMEs:
 * ATX headings, paragraphs, lists, block quotes, fenced code, rules and
 * inline code / bold / italic / http(s) links. Output is React elements only
 * (no raw HTML), so untrusted content is safe.
 */

const INLINE_RE = /(`[^`]+`)|(\*\*[^*]+\*\*|__[^_]+__)|(\[[^\]]+\]\([^)\s]+\))|(\*[^*\s][^*]*\*|_[^_\s][^_]*_)/g;

function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE_RE)) {
    const i = m.index;
    if (i > last) out.push(text.slice(last, i));
    const [tok] = m;
    if (m[1]) {
      out.push(
        <code key={i} className="rounded bg-muted px-1 py-0.5 font-mono text-[0.85em]">
          {tok.slice(1, -1)}
        </code>,
      );
    } else if (m[2]) {
      out.push(<strong key={i}>{inline(tok.slice(2, -2))}</strong>);
    } else if (m[3]) {
      const close = tok.indexOf("](");
      const label = tok.slice(1, close);
      const href = tok.slice(close + 2, -1);
      out.push(
        /^https?:\/\//i.test(href) ? (
          <a key={i} href={href} target="_blank" rel="noreferrer noopener" className="underline underline-offset-2">
            {inline(label)}
          </a>
        ) : (
          <span key={i} className="underline decoration-dotted underline-offset-2">
            {inline(label)}
          </span>
        ),
      );
    } else {
      out.push(<em key={i}>{inline(tok.slice(1, -1))}</em>);
    }
    last = i + tok.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

const HEADING_CLASS = ["text-xl font-semibold", "text-lg font-semibold", "text-base font-semibold", "text-sm font-semibold", "text-sm font-medium", "text-sm font-medium"];

export function Markdown({ source }: { source: string }) {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i]!;
    const key = blocks.length;

    if (line.trim() === "") {
      i += 1;
      continue;
    }

    const fence = /^\s*(```|~~~)/.exec(line);
    if (fence) {
      const code: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i]!.trimStart().startsWith(fence[1]!)) code.push(lines[i++]!);
      i += 1;
      blocks.push(
        <pre key={key} className="overflow-x-auto rounded-md border bg-muted/50 p-3 font-mono text-xs leading-5">
          <code>{code.join("\n")}</code>
        </pre>,
      );
      continue;
    }

    const heading = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(line);
    if (heading) {
      const level = heading[1]!.length;
      // Demoted two levels: the page already has h1 / h2.
      const Tag = `h${Math.min(level + 2, 6)}` as "h3" | "h4" | "h5" | "h6";
      blocks.push(
        <Tag key={key} className={HEADING_CLASS[level - 1]}>
          {inline(heading[2]!)}
        </Tag>,
      );
      i += 1;
      continue;
    }

    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      blocks.push(<hr key={key} className="border-border" />);
      i += 1;
      continue;
    }

    if (/^\s*>/.test(line)) {
      const quote: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i]!)) quote.push(lines[i++]!.replace(/^\s*>\s?/, ""));
      blocks.push(
        <blockquote key={key} className="border-l-2 border-foreground/30 pl-3 text-muted-foreground">
          <Markdown source={quote.join("\n")} />
        </blockquote>,
      );
      continue;
    }

    const listMatch = /^\s*([-*+]|\d+[.)])\s+/.exec(line);
    if (listMatch) {
      const ordered = /\d/.test(listMatch[1]!);
      const items: string[] = [];
      while (i < lines.length) {
        const m = /^\s*([-*+]|\d+[.)])\s+(.*)$/.exec(lines[i]!);
        if (m) items.push(m[2]!);
        else if (lines[i]!.trim() !== "" && /^\s{2,}/.test(lines[i]!) && items.length) items[items.length - 1] += ` ${lines[i]!.trim()}`;
        else break;
        i += 1;
      }
      const List = ordered ? "ol" : "ul";
      blocks.push(
        <List key={key} className={ordered ? "list-decimal pl-5" : "list-disc pl-5"}>
          {items.map((it, n) => (
            <li key={n}>{inline(it)}</li>
          ))}
        </List>,
      );
      continue;
    }

    const para: string[] = [];
    while (i < lines.length && lines[i]!.trim() !== "" && !/^\s*(```|~~~|#{1,6}\s|>|[-*+]\s|\d+[.)]\s)/.test(lines[i]!)) {
      para.push(lines[i++]!.trim());
    }
    if (para.length === 0) para.push(lines[i++]!.trim());
    blocks.push(<p key={key}>{inline(para.join(" "))}</p>);
  }

  return <div className="flex flex-col gap-3 text-sm leading-relaxed">{blocks}</div>;
}
