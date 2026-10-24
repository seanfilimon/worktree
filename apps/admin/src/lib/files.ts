import type { SnapshotFile } from "@/api/types";

export interface DirEntry {
  name: string;
  /** Full '/'-separated path of the directory. */
  path: string;
  fileCount: number;
  size: number;
}

export interface DirListing {
  dirs: DirEntry[];
  files: SnapshotFile[];
}

/** Normalises "a/b/" and "/a/b" to "a/b"; "" is the root. */
export function normalizeDir(dir: string): string {
  return dir.split("/").filter(Boolean).join("/");
}

/** Direct children of `dir` in a flat manifest, directories first, sorted. */
export function listDirectory(files: readonly SnapshotFile[], dir: string): DirListing {
  const prefix = normalizeDir(dir);
  const base = prefix ? `${prefix}/` : "";
  const dirs = new Map<string, DirEntry>();
  const direct: SnapshotFile[] = [];

  for (const f of files) {
    if (!f.path.startsWith(base)) continue;
    const rest = f.path.slice(base.length);
    const slash = rest.indexOf("/");
    if (slash === -1) {
      direct.push(f);
      continue;
    }
    const name = rest.slice(0, slash);
    const entry = dirs.get(name) ?? { name, path: base + name, fileCount: 0, size: 0 };
    entry.fileCount += 1;
    entry.size += f.size;
    dirs.set(name, entry);
  }

  const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name);
  return {
    dirs: [...dirs.values()].sort(byName),
    files: direct
      .map((f) => ({ f, name: f.path.slice(base.length) }))
      .sort(byName)
      .map(({ f }) => f),
  };
}

export function baseName(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}

export function parentDir(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? "" : path.slice(0, i);
}

/**
 * Decodes bytes as UTF-8 text, or returns null when the content looks
 * binary (NUL bytes in the first 8 KiB, or invalid UTF-8).
 */
export function decodeText(bytes: ArrayBuffer): string | null {
  const view = new Uint8Array(bytes);
  const sniff = Math.min(view.length, 8192);
  for (let i = 0; i < sniff; i++) if (view[i] === 0) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(view);
  } catch {
    return null;
  }
}

/** Splits text into lines, dropping the empty line after a trailing newline. */
export function splitLines(text: string): string[] {
  const lines = text.split(/\r?\n/);
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/** Triggers a browser download of `bytes` as `filename`. */
export function downloadBytes(bytes: ArrayBuffer, filename: string): void {
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
