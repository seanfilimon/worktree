/**
 * Language detection by file name and a small, dependency-free tokenizer for
 * monochrome syntax emphasis: keywords bold, comments italic + muted, strings
 * regular. It is deliberately approximate — it only needs to be right enough
 * to guide the eye, never to parse.
 */

export type TokenKind = "plain" | "keyword" | "comment" | "string";

export interface Token {
  kind: TokenKind;
  text: string;
}

export interface Language {
  id: string;
  /** Display name, e.g. "TypeScript". */
  name: string;
  keywords: ReadonlySet<string>;
  lineComments: readonly string[];
  blockComment?: readonly [open: string, close: string];
  /** Single-line string delimiters (backslash escapes). */
  quotes: readonly string[];
  /** Delimiters of strings that may span lines (e.g. ``` ` ``` or `"""`). */
  multilineQuotes: readonly string[];
  /** Keywords match regardless of case (SQL). */
  caseInsensitive?: boolean;
}

const words = (s: string): ReadonlySet<string> => new Set(s.split(/\s+/).filter(Boolean));

const C_LIKE_COMMENTS = { lineComments: ["//"], blockComment: ["/*", "*/"] as const };

const JS_KEYWORDS =
  "abstract as async await break case catch class const constructor continue debugger declare default delete do else enum export extends false finally for from function get if implements import in infer instanceof interface is keyof let namespace new null of private protected public readonly return satisfies set static super switch this throw true try type typeof undefined var void while with yield";

type LanguageSpec = Omit<Language, "keywords" | "lineComments" | "quotes" | "multilineQuotes"> &
  Partial<Pick<Language, "lineComments" | "quotes" | "multilineQuotes">> & { keywords: string; extensions: string[]; names?: string[] };

const SPECS: LanguageSpec[] = [
  {
    id: "typescript",
    name: "TypeScript",
    extensions: ["ts", "tsx", "mts", "cts"],
    keywords: JS_KEYWORDS,
    ...C_LIKE_COMMENTS,
    quotes: ['"', "'"],
    multilineQuotes: ["`"],
  },
  {
    id: "javascript",
    name: "JavaScript",
    extensions: ["js", "jsx", "mjs", "cjs"],
    keywords: JS_KEYWORDS,
    ...C_LIKE_COMMENTS,
    quotes: ['"', "'"],
    multilineQuotes: ["`"],
  },
  {
    id: "go",
    name: "Go",
    extensions: ["go"],
    keywords:
      "break case chan const continue default defer else fallthrough false for func go goto if import interface iota map nil package range return select struct switch true type var",
    ...C_LIKE_COMMENTS,
    quotes: ['"', "'"],
    multilineQuotes: ["`"],
  },
  {
    id: "rust",
    name: "Rust",
    extensions: ["rs"],
    keywords:
      "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while",
    ...C_LIKE_COMMENTS,
    quotes: ['"'],
  },
  {
    id: "python",
    name: "Python",
    extensions: ["py", "pyi"],
    keywords:
      "False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield",
    lineComments: ["#"],
    quotes: ['"', "'"],
    multilineQuotes: ['"""', "'''"],
  },
  {
    id: "shell",
    name: "Shell",
    extensions: ["sh", "bash", "zsh", "fish"],
    names: [".bashrc", ".zshrc", ".profile"],
    keywords: "case do done elif else esac exit export fi for function if in local readonly return set shift then unset until while",
    lineComments: ["#"],
    quotes: ['"', "'"],
  },
  {
    id: "powershell",
    name: "PowerShell",
    extensions: ["ps1", "psm1"],
    keywords:
      "begin break catch class continue do else elseif end exit filter finally for foreach function if param process return switch throw try until while",
    lineComments: ["#"],
    blockComment: ["<#", "#>"],
    quotes: ['"', "'"],
    caseInsensitive: true,
  },
  { id: "json", name: "JSON", extensions: ["json", "jsonc", "json5"], keywords: "true false null", ...C_LIKE_COMMENTS, quotes: ['"'] },
  {
    id: "yaml",
    name: "YAML",
    extensions: ["yml", "yaml"],
    keywords: "true false null yes no on off",
    lineComments: ["#"],
    quotes: ['"', "'"],
  },
  {
    id: "toml",
    name: "TOML",
    extensions: ["toml"],
    names: ["Cargo.lock"],
    keywords: "true false",
    lineComments: ["#"],
    quotes: ['"', "'"],
    multilineQuotes: ['"""', "'''"],
  },
  {
    id: "ini",
    name: "INI",
    extensions: ["ini", "cfg", "conf", "env"],
    names: [".editorconfig", ".env"],
    keywords: "true false",
    lineComments: ["#", ";"],
  },
  {
    id: "css",
    name: "CSS",
    extensions: ["css", "scss", "sass", "less"],
    keywords: "@apply @import @layer @media @supports @theme @keyframes @font-face @tailwind !important",
    blockComment: ["/*", "*/"],
    quotes: ['"', "'"],
  },
  {
    id: "html",
    name: "HTML",
    extensions: ["html", "htm", "xml", "svg", "vue", "svelte", "xaml", "plist"],
    keywords: "",
    blockComment: ["<!--", "-->"],
    quotes: ['"', "'"],
  },
  { id: "markdown", name: "Markdown", extensions: ["md", "markdown", "mdx"], keywords: "" },
  {
    id: "sql",
    name: "SQL",
    extensions: ["sql"],
    keywords:
      "add all alter and as asc begin between by case check column commit constraint create default delete desc distinct drop else end exists foreign from group having if in index inner insert into is join key left like limit not null on or order outer primary references right rollback select set table then transaction union unique update values view when where with",
    lineComments: ["--"],
    blockComment: ["/*", "*/"],
    quotes: ["'", '"'],
    caseInsensitive: true,
  },
  {
    id: "c",
    name: "C/C++",
    extensions: ["c", "h", "cc", "cpp", "cxx", "hpp", "hh"],
    keywords:
      "auto bool break case catch char class const constexpr continue default delete do double else enum extern false float for friend goto if inline int long namespace new nullptr operator private protected public register return short signed sizeof static struct switch template this throw true try typedef typename union unsigned using virtual void volatile while #include #define #ifdef #ifndef #endif #pragma #if #else",
    ...C_LIKE_COMMENTS,
    quotes: ['"', "'"],
  },
  {
    id: "java",
    name: "Java/Kotlin",
    extensions: ["java", "kt", "kts", "scala", "groovy", "gradle"],
    keywords:
      "abstract boolean break byte case catch char class const continue data default do double else enum extends false final finally float for fun if implements import in instanceof int interface is long native new null object override package private protected public return short static super switch synchronized this throw throws true try val var void volatile when while",
    ...C_LIKE_COMMENTS,
    quotes: ['"', "'"],
    multilineQuotes: ['"""'],
  },
  {
    id: "csharp",
    name: "C#",
    extensions: ["cs"],
    keywords:
      "abstract as async await base bool break case catch class const continue default delegate do else enum event false finally for foreach get if in interface internal is namespace new null out override private protected public readonly record ref return sealed set static string struct switch this throw true try using var virtual void while",
    ...C_LIKE_COMMENTS,
    quotes: ['"', "'"],
  },
  {
    id: "swift",
    name: "Swift",
    extensions: ["swift"],
    keywords:
      "as break case catch class continue default defer do else enum extension false fileprivate for func guard if import in init internal let nil private protocol public return self static struct switch throw throws true try var where while",
    ...C_LIKE_COMMENTS,
    quotes: ['"'],
    multilineQuotes: ['"""'],
  },
  {
    id: "ruby",
    name: "Ruby",
    extensions: ["rb", "rake", "gemspec"],
    names: ["Gemfile", "Rakefile"],
    keywords:
      "alias and begin break case class def defined? do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield",
    lineComments: ["#"],
    quotes: ['"', "'"],
  },
  {
    id: "php",
    name: "PHP",
    extensions: ["php"],
    keywords:
      "abstract and array as break case catch class const continue default do echo else elseif extends false final finally fn for foreach function if implements interface namespace new null private protected public return static switch throw trait true try use var while",
    lineComments: ["//", "#"],
    blockComment: ["/*", "*/"],
    quotes: ['"', "'"],
  },
  {
    id: "lua",
    name: "Lua",
    extensions: ["lua"],
    keywords: "and break do else elseif end false for function goto if in local nil not or repeat return then true until while",
    lineComments: ["--"],
    quotes: ['"', "'"],
  },
  {
    id: "protobuf",
    name: "Protocol Buffers",
    extensions: ["proto"],
    keywords: "enum import map message oneof option optional package repeated reserved returns rpc service stream syntax",
    ...C_LIKE_COMMENTS,
    quotes: ['"', "'"],
  },
  {
    id: "dockerfile",
    name: "Dockerfile",
    extensions: ["dockerfile"],
    names: ["Dockerfile", "Containerfile"],
    keywords: "ADD ARG AS CMD COPY ENTRYPOINT ENV EXPOSE FROM HEALTHCHECK LABEL ONBUILD RUN SHELL STOPSIGNAL USER VOLUME WORKDIR",
    lineComments: ["#"],
    quotes: ['"', "'"],
  },
  {
    id: "makefile",
    name: "Makefile",
    extensions: ["mk"],
    names: ["Makefile", "GNUmakefile"],
    keywords: "define endef ifeq ifneq ifdef ifndef else endif include export .PHONY",
    lineComments: ["#"],
  },
  { id: "text", name: "Plain text", extensions: ["txt", "log", "csv", "tsv"], names: ["LICENSE", "COPYING", "AUTHORS"], keywords: "" },
];

const BY_EXTENSION = new Map<string, Language>();
const BY_NAME = new Map<string, Language>();
for (const spec of SPECS) {
  const { extensions, names = [], keywords, ...rest } = spec;
  const lang: Language = {
    lineComments: [],
    quotes: [],
    multilineQuotes: [],
    ...rest,
    keywords: spec.caseInsensitive ? words(keywords.toLowerCase()) : words(keywords),
  };
  for (const ext of extensions) BY_EXTENSION.set(ext, lang);
  for (const n of names) BY_NAME.set(n.toLowerCase(), lang);
}

function baseNameOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}

/** Lower-case extension without the dot ("" when there is none). */
export function extensionOf(path: string): string {
  const name = baseNameOf(path);
  const dot = name.lastIndexOf(".");
  return dot <= 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/** The language of a file by its name, or null when unknown. */
export function detectLanguage(path: string): Language | null {
  const name = baseNameOf(path).toLowerCase();
  return (
    BY_NAME.get(name) ?? BY_EXTENSION.get(extensionOf(path)) ?? (name.startsWith("dockerfile") ? (BY_NAME.get("dockerfile") ?? null) : null)
  );
}

const IMAGE_TYPES: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  ico: "image/x-icon",
  avif: "image/avif",
};

/**
 * MIME type of raster images that are safe to preview via a blob URL. SVG is
 * intentionally excluded: it can carry script, so it is shown as code.
 */
export function imageMimeType(path: string): string | null {
  return IMAGE_TYPES[extensionOf(path)] ?? null;
}

export function isMarkdown(path: string): boolean {
  return detectLanguage(path)?.id === "markdown";
}

const IDENT_START = /[A-Za-z_$@#.!]/;
const IDENT_PART = /[\w$?-]/;

/** Appends text to the line, merging with the previous token of the same kind. */
function push(line: Token[], kind: TokenKind, text: string): void {
  if (!text) return;
  const last = line[line.length - 1];
  if (last && last.kind === kind) last.text += text;
  else line.push({ kind, text });
}

/** Index just past the closing `quote` starting the scan at `from`, honouring backslash escapes; -1 if not found. */
function findClose(text: string, from: number, quote: string, escapes: boolean): number {
  for (let i = from; i < text.length; i++) {
    if (escapes && text[i] === "\\") {
      i++;
      continue;
    }
    if (text.startsWith(quote, i)) return i + quote.length;
  }
  return -1;
}

function isKeyword(lang: Language, word: string): boolean {
  return lang.keywords.has(lang.caseInsensitive ? word.toLowerCase() : word);
}

/**
 * Splits `text` into lines of tokens. Block comments and multi-line strings
 * carry over between lines. Unknown languages (null) yield plain lines.
 */
export function tokenize(lines: readonly string[], lang: Language | null): Token[][] {
  if (!lang || (lang.keywords.size === 0 && !lang.lineComments.length && !lang.blockComment && !lang.quotes.length)) {
    return lines.map((l) => (l ? [{ kind: "plain", text: l }] : []));
  }
  const out: Token[][] = [];
  /** Open multi-line construct carried from the previous line. */
  let open: { kind: "comment"; close: string } | { kind: "string"; close: string } | null = null;

  for (const text of lines) {
    const line: Token[] = [];
    let i = 0;

    if (open) {
      const end = open.kind === "comment" ? text.indexOf(open.close) : findClose(text, 0, open.close, open.close === "`");
      if (end === -1) {
        push(line, open.kind, text);
        out.push(line);
        continue;
      }
      const stop = open.kind === "comment" ? end + open.close.length : end;
      push(line, open.kind, text.slice(0, stop));
      i = stop;
      open = null;
    }

    let plainStart = i;
    const flush = (to: number) => {
      push(line, "plain", text.slice(plainStart, to));
    };

    while (i < text.length) {
      const lc = lang.lineComments.find((p) => text.startsWith(p, i));
      if (lc) {
        flush(i);
        push(line, "comment", text.slice(i));
        i = text.length;
        plainStart = i;
        break;
      }
      if (lang.blockComment && text.startsWith(lang.blockComment[0], i)) {
        flush(i);
        const close = lang.blockComment[1];
        const end = text.indexOf(close, i + lang.blockComment[0].length);
        if (end === -1) {
          push(line, "comment", text.slice(i));
          open = { kind: "comment", close };
          i = text.length;
        } else {
          push(line, "comment", text.slice(i, end + close.length));
          i = end + close.length;
        }
        plainStart = i;
        continue;
      }
      const mq = lang.multilineQuotes.find((q) => text.startsWith(q, i));
      if (mq) {
        flush(i);
        const end = findClose(text, i + mq.length, mq, mq === "`");
        if (end === -1) {
          push(line, "string", text.slice(i));
          open = { kind: "string", close: mq };
          i = text.length;
        } else {
          push(line, "string", text.slice(i, end));
          i = end;
        }
        plainStart = i;
        continue;
      }
      const q = lang.quotes.find((d) => text.startsWith(d, i));
      if (q) {
        flush(i);
        const end = findClose(text, i + q.length, q, true);
        const stop = end === -1 ? text.length : end;
        push(line, "string", text.slice(i, stop));
        i = stop;
        plainStart = i;
        continue;
      }
      const c = text.charAt(i);
      const prev = i > 0 ? text.charAt(i - 1) : "";
      if (IDENT_START.test(c) && !/[\w$]/.test(prev)) {
        let j = i + 1;
        while (j < text.length && IDENT_PART.test(text.charAt(j))) j++;
        const word = text.slice(i, j);
        if (isKeyword(lang, word)) {
          flush(i);
          push(line, "keyword", word);
          plainStart = j;
        }
        i = j;
        continue;
      }
      i++;
    }
    flush(i);
    out.push(line);
  }
  return out;
}
