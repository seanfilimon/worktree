import { Check, ImageIcon } from "lucide-react";
import { useMemo, type ReactNode } from "react";
import { Link } from "react-router";
import { detectLanguage, tokenize } from "@/lib/code";
import { splitLines } from "@/lib/files";
import { cn } from "@/lib/utils";
import { CodeTokens } from "./CodeView";
import { parseMarkdown, safeHref, type Block, type Inline } from "./markdown";

interface MarkdownViewProps {
  source: string;
  /** Router location for a relative link (e.g. "docs/intro.md"), or null to render it as text. */
  resolveLink: (target: string) => string | null;
  className?: string;
}

function InlineNodes({ nodes, resolveLink }: { nodes: Inline[]; resolveLink: MarkdownViewProps["resolveLink"] }): ReactNode {
  return nodes.map((n, i) => {
    switch (n.type) {
      case "text":
        return n.text;
      case "break":
        return <br key={i} />;
      case "code":
        return (
          <code key={i} className="rounded-sm border bg-muted px-1 py-0.5 font-mono text-[0.85em]">
            {n.text}
          </code>
        );
      case "strong":
        return (
          <strong key={i} className="font-semibold">
            <InlineNodes nodes={n.children} resolveLink={resolveLink} />
          </strong>
        );
      case "em":
        return (
          <em key={i}>
            <InlineNodes nodes={n.children} resolveLink={resolveLink} />
          </em>
        );
      case "del":
        return (
          <del key={i}>
            <InlineNodes nodes={n.children} resolveLink={resolveLink} />
          </del>
        );
      case "image":
        // Images are not fetched (privacy / auth); show a labelled placeholder link.
        return (
          <MdLink key={i} href={n.src} resolveLink={resolveLink}>
            <span className="inline-flex items-center gap-1">
              <ImageIcon aria-hidden="true" className="size-3.5" />
              {n.alt || "image"}
            </span>
          </MdLink>
        );
      case "link":
        return (
          <MdLink key={i} href={n.href} resolveLink={resolveLink}>
            <InlineNodes nodes={n.children} resolveLink={resolveLink} />
          </MdLink>
        );
    }
  });
}

const linkClass = "font-medium underline underline-offset-4 hover:no-underline";

function MdLink({ href, resolveLink, children }: { href: string; resolveLink: MarkdownViewProps["resolveLink"]; children: ReactNode }) {
  const target = safeHref(href);
  if (target?.kind === "external")
    return (
      <a href={target.href} target="_blank" rel="noopener noreferrer" className={linkClass}>
        {children}
      </a>
    );
  if (target?.kind === "anchor")
    return (
      <a href={target.href} className={linkClass}>
        {children}
      </a>
    );
  const to = target?.kind === "relative" ? resolveLink(target.path) : null;
  if (to)
    return (
      <Link to={to} className={linkClass}>
        {children}
      </Link>
    );
  return <span className="underline decoration-dotted underline-offset-4">{children}</span>;
}

const HEADING_CLASS = [
  "mt-6 mb-3 border-b pb-2 text-2xl font-bold first:mt-0",
  "mt-6 mb-3 border-b pb-1.5 text-xl font-bold first:mt-0",
  "mt-5 mb-2 text-lg font-semibold first:mt-0",
  "mt-4 mb-2 text-base font-semibold first:mt-0",
  "mt-4 mb-2 text-sm font-semibold first:mt-0",
  "mt-4 mb-2 text-sm font-semibold text-muted-foreground first:mt-0",
] as const;

function Blocks({
  blocks,
  resolveLink,
  tight = false,
}: {
  blocks: Block[];
  resolveLink: MarkdownViewProps["resolveLink"];
  tight?: boolean;
}): ReactNode {
  return blocks.map((b, i) => {
    switch (b.type) {
      case "heading": {
        const H = `h${b.level}` as const;
        return (
          <H key={i} id={b.id} className={HEADING_CLASS[b.level - 1]}>
            <InlineNodes nodes={b.content} resolveLink={resolveLink} />
          </H>
        );
      }
      case "paragraph":
        return tight ? (
          <InlineNodes key={i} nodes={b.content} resolveLink={resolveLink} />
        ) : (
          <p key={i} className="my-3 leading-relaxed first:mt-0 last:mb-0">
            <InlineNodes nodes={b.content} resolveLink={resolveLink} />
          </p>
        );
      case "code":
        return <FencedCode key={i} lang={b.lang} text={b.text} />;
      case "quote":
        return (
          <blockquote key={i} className="my-3 border-l-4 pl-4 text-muted-foreground">
            <Blocks blocks={b.children} resolveLink={resolveLink} />
          </blockquote>
        );
      case "rule":
        return <hr key={i} className="my-6 border-t-2" />;
      case "list": {
        const items = b.items.map((item, j) => {
          const single = item.children.length === 1 && item.children[0]?.type === "paragraph";
          return (
            <li key={j} className={cn("my-1", item.checked !== null && "flex list-none items-start gap-2")}>
              {item.checked !== null && (
                <span
                  role="img"
                  aria-label={item.checked ? "Done" : "Not done"}
                  className="mt-1 inline-flex size-3.5 shrink-0 items-center justify-center rounded-sm border border-foreground"
                >
                  {item.checked ? <Check aria-hidden="true" className="size-3" strokeWidth={3} /> : null}
                </span>
              )}
              <div className="min-w-0">
                <Blocks blocks={item.children} resolveLink={resolveLink} tight={single} />
              </div>
            </li>
          );
        });
        return b.ordered ? (
          <ol key={i} start={b.start} className="my-3 list-decimal pl-6">
            {items}
          </ol>
        ) : (
          <ul key={i} className="my-3 list-disc pl-6">
            {items}
          </ul>
        );
      }
      case "table":
        return (
          <div key={i} className="my-4 overflow-x-auto">
            <table className="w-full border-collapse text-sm">
              <thead>
                <tr>
                  {b.head.map((cell, c) => (
                    <th key={c} style={{ textAlign: b.align[c] ?? undefined }} className="border bg-muted px-3 py-1.5 font-semibold">
                      <InlineNodes nodes={cell} resolveLink={resolveLink} />
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {b.rows.map((row, r) => (
                  <tr key={r}>
                    {row.map((cell, c) => (
                      <td key={c} style={{ textAlign: b.align[c] ?? undefined }} className="border px-3 py-1.5">
                        <InlineNodes nodes={cell} resolveLink={resolveLink} />
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
    }
  });
}

function FencedCode({ lang, text }: { lang: string; text: string }) {
  const lines = useMemo(() => tokenize(splitLines(text), lang ? detectLanguage(`file.${lang.toLowerCase()}`) : null), [lang, text]);
  return (
    <pre className="my-3 overflow-x-auto rounded-md border bg-muted/50 p-3 font-mono text-[12.5px] leading-5">
      <code>
        {lines.map((l, i) => (
          <span key={i} className="block">
            {l.length ? <CodeTokens tokens={l} /> : " "}
          </span>
        ))}
      </code>
    </pre>
  );
}

/** Renders Markdown safely (React elements only; raw HTML shows as text). */
export function MarkdownView({ source, resolveLink, className }: MarkdownViewProps) {
  const blocks = useMemo(() => parseMarkdown(source), [source]);
  return (
    <article className={cn("max-w-none p-6 text-sm break-words", className)}>
      <Blocks blocks={blocks} resolveLink={resolveLink} />
    </article>
  );
}
