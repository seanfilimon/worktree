import { ChevronRight, Search } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { Link, useNavigate } from "react-router";
import type { SnapshotFile } from "@/api/types";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { FileIcon, FolderIcon } from "./FileIcon";
import { buildTree, filterFiles, type TreeNode } from "./tree";

interface FileTreeProps {
  files: readonly SnapshotFile[];
  /** Path of the selected file or directory ("" = root). */
  selected: string;
  expanded: ReadonlySet<string>;
  onToggle: (dir: string) => void;
  /** Router location that opens `path` (a file or a directory). */
  hrefFor: (path: string) => string;
  className?: string;
}

const rowClass =
  "flex w-full min-w-0 items-center gap-1.5 rounded-sm py-1 pr-2 text-left font-mono text-[13px] outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50";

/** Bolds the characters of `text` at `indices`. */
function Highlighted({ text, indices }: { text: string; indices: readonly number[] }) {
  const marks = new Set(indices);
  const parts: ReactNode[] = [];
  let run = "";
  let bold = false;
  const flush = (key: number) => {
    if (!run) return;
    parts.push(
      bold ? (
        <strong key={key} className="font-bold text-foreground underline underline-offset-2">
          {run}
        </strong>
      ) : (
        run
      ),
    );
    run = "";
  };
  for (let i = 0; i < text.length; i++) {
    const m = marks.has(i);
    if (m !== bold) {
      flush(i);
      bold = m;
    }
    run += text.charAt(i);
  }
  flush(text.length);
  return <>{parts}</>;
}

function Nodes({ nodes, depth, ...props }: { nodes: TreeNode[]; depth: number } & Omit<FileTreeProps, "files" | "className">) {
  const { selected, expanded, onToggle, hrefFor } = props;
  return (
    <ul className="flex flex-col">
      {nodes.map((n) => {
        const pad = { paddingLeft: `${depth * 12 + 6}px` };
        if (n.kind === "dir") {
          const open = expanded.has(n.path);
          return (
            <li key={n.path}>
              <button
                type="button"
                aria-expanded={open}
                onClick={() => onToggle(n.path)}
                className={cn(rowClass, selected === n.path && "bg-accent font-semibold")}
                style={pad}
              >
                <ChevronRight
                  aria-hidden="true"
                  className={cn("size-3.5 shrink-0 text-muted-foreground transition-transform", open && "rotate-90")}
                />
                <FolderIcon open={open} />
                <span className="truncate">{n.name}</span>
              </button>
              {open && <Nodes nodes={n.children} depth={depth + 1} {...props} />}
            </li>
          );
        }
        const current = selected === n.path;
        return (
          <li key={n.path}>
            <Link
              to={hrefFor(n.path)}
              aria-current={current ? "page" : undefined}
              className={cn(rowClass, current && "bg-primary font-semibold text-primary-foreground hover:bg-primary/90")}
              style={{ paddingLeft: `${depth * 12 + 26}px` }}
            >
              <FileIcon path={n.path} executable={n.file.executable} className={cn(current && "text-primary-foreground")} />
              <span className="truncate">{n.name}</span>
            </Link>
          </li>
        );
      })}
    </ul>
  );
}

/** Collapsible directory tree of a snapshot with a fuzzy "go to file" filter. */
export function FileTree({ files, className, ...props }: FileTreeProps) {
  const [query, setQuery] = useState("");
  const navigate = useNavigate();
  const tree = useMemo(() => buildTree(files), [files]);
  const matches = useMemo(() => (query.trim() ? filterFiles(files, query) : null), [files, query]);

  return (
    <nav aria-label="File tree" className={cn("flex min-h-0 flex-col", className)}>
      <div className="relative border-b p-2">
        <Search aria-hidden="true" className="pointer-events-none absolute top-1/2 left-4 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          type="search"
          aria-label="Filter files"
          placeholder="Go to file…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            const first = matches?.[0];
            if (e.key === "Enter" && first) {
              e.preventDefault();
              void navigate(props.hrefFor(first.file.path));
            } else if (e.key === "Escape" && query) {
              e.preventDefault();
              setQuery("");
            }
          }}
          className="h-8 pl-8 text-[13px]"
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-1">
        {matches ? (
          matches.length === 0 ? (
            <p className="px-3 py-6 text-center text-sm text-muted-foreground">No files match “{query.trim()}”.</p>
          ) : (
            <ul aria-label="Matching files" className="flex flex-col">
              {matches.map((m) => (
                <li key={m.file.path}>
                  <Link
                    to={props.hrefFor(m.file.path)}
                    aria-current={props.selected === m.file.path ? "page" : undefined}
                    className={cn(rowClass, "pl-2 text-muted-foreground", props.selected === m.file.path && "bg-accent")}
                  >
                    <FileIcon path={m.file.path} executable={m.file.executable} />
                    <span className="truncate">
                      <Highlighted text={m.file.path} indices={m.indices} />
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          )
        ) : files.length === 0 ? (
          <p className="px-3 py-6 text-center text-sm text-muted-foreground">No files.</p>
        ) : (
          <Nodes nodes={tree} depth={0} {...props} />
        )}
      </div>
    </nav>
  );
}
