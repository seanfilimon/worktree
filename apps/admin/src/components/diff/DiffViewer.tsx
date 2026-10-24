import type { FileDiff } from "@/api/types";

/**
 * Renders a list of file diffs (unified patches) from the compare endpoint.
 * STUB: the tree-history workstream replaces this with the full viewer; the
 * props are the contract.
 */
export function DiffViewer({ files }: { files: FileDiff[] }) {
  return (
    <div className="space-y-4">
      {files.map((f) => (
        <pre key={f.path} className="overflow-x-auto rounded-md border p-3 font-mono text-xs">
          {f.path}
          {"\n"}
          {f.patch ?? (f.binary ? "Binary file" : "")}
        </pre>
      ))}
    </div>
  );
}
