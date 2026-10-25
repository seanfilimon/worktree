import {
  File,
  FileArchive,
  FileCode,
  FileCog,
  FileImage,
  FileJson,
  FileLock,
  FileTerminal,
  FileText,
  Folder,
  FolderOpen,
  type LucideIcon,
} from "lucide-react";
import { createElement } from "react";
import { detectLanguage, extensionOf, imageMimeType } from "@/lib/code";
import { cn } from "@/lib/utils";

const ARCHIVES = new Set(["zip", "gz", "tgz", "tar", "bz2", "xz", "7z", "rar", "zst"]);
const LOCKS = new Set(["lock", "sum"]);
const CONFIG = new Set(["yaml", "toml", "ini", "dockerfile", "makefile"]);
const SCRIPTS = new Set(["shell", "powershell"]);
const PROSE = new Set(["markdown", "text"]);

function iconFor(path: string, executable: boolean): LucideIcon {
  if (imageMimeType(path) || extensionOf(path) === "svg") return FileImage;
  const ext = extensionOf(path);
  if (ARCHIVES.has(ext)) return FileArchive;
  if (LOCKS.has(ext) || /(^|\/)package-lock\.json$/.test(path)) return FileLock;
  const lang = detectLanguage(path);
  if (lang?.id === "json") return FileJson;
  if ((lang && SCRIPTS.has(lang.id)) || executable) return FileTerminal;
  if (lang && CONFIG.has(lang.id)) return FileCog;
  if (lang && PROSE.has(lang.id)) return FileText;
  if (lang) return FileCode;
  return File;
}

/** Monochrome file-type icon (decorative). */
export function FileIcon({ path, executable = false, className }: { path: string; executable?: boolean; className?: string }) {
  return createElement(iconFor(path, executable), {
    "aria-hidden": true,
    className: cn("size-4 shrink-0 text-muted-foreground", className),
  });
}

export function FolderIcon({ open = false, className }: { open?: boolean; className?: string }) {
  const Icon = open ? FolderOpen : Folder;
  return <Icon aria-hidden="true" className={cn("size-4 shrink-0 text-foreground", className)} />;
}
