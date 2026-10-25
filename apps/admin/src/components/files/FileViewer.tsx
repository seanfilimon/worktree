import { useMutation } from "@tanstack/react-query";
import { Binary, Check, Copy, Download, FileWarning, WrapText } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { useLocation } from "react-router";
import { useApi } from "@/api/context";
import { useBlob } from "@/api/queries";
import type { SnapshotFile } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Toggle } from "@/components/ui/toggle";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { detectLanguage, imageMimeType, isMarkdown } from "@/lib/code";
import { baseName, decodeText, downloadBytes, splitLines } from "@/lib/files";
import { formatBytes, formatNumber } from "@/lib/format";
import { useToast } from "@/toast/store";
import { CodeView } from "./CodeView";
import { FileHistory } from "./FileHistory";
import { ImagePreview } from "./ImagePreview";
import { parseLineHash } from "./lines";
import { MarkdownView } from "./MarkdownView";
import { EmptyState, ErrorState, LoadingRows } from "./states";

/** Larger files are offered as a download instead of being fetched for preview. */
const PREVIEW_LIMIT = 1024 * 1024;

type TabId = "preview" | "code" | "history";

interface FileViewerProps {
  tenant: string;
  tree: string;
  snapshot: string;
  /** Branch used for the history tab ("" = default branch). */
  branch: string;
  file: SnapshotFile;
  /** Router location of a file or directory, for relative Markdown links. */
  resolveLink: (target: string) => string | null;
}

function IconButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button variant="outline" size="icon-sm" aria-label={label} onClick={onClick} disabled={disabled}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

export function FileViewer({ tenant, tree, snapshot, branch, file, resolveLink }: FileViewerProps) {
  const api = useApi();
  const toast = useToast();
  const location = useLocation();
  const previewable = file.size <= PREVIEW_LIMIT;
  const blob = useBlob(tenant, tree, snapshot, file.path, previewable);
  const text = useMemo(() => (blob.data ? decodeText(blob.data) : null), [blob.data]);
  const lines = useMemo(() => (text === null ? [] : splitLines(text)), [text]);
  const language = detectLanguage(file.path);
  const imageType = imageMimeType(file.path);
  const markdown = isMarkdown(file.path);
  const hasPreview = previewable && (imageType !== null || markdown);
  /** Images have no code view unless they are too large to preview. */
  const hasCode = !hasPreview || imageType === null;
  const [tab, setTab] = useState<TabId>(() => (hasPreview && (!hasCode || !parseLineHash(location.hash)) ? "preview" : "code"));
  const [wrap, setWrap] = useState(false);
  const [copied, setCopied] = useState(false);

  const download = useMutation({
    mutationFn: async () => blob.data ?? (await api.trees.blob(tenant, tree, snapshot, file.path)),
    onSuccess: (bytes) => downloadBytes(bytes, baseName(file.path)),
  });

  const copy = useMutation({
    mutationFn: (value: string) => navigator.clipboard.writeText(value),
    onSuccess: () => {
      setCopied(true);
      toast.success("Copied to clipboard", file.path);
      window.setTimeout(() => setCopied(false), 2000);
    },
  });

  const empty = text !== null && lines.length === 1 && lines[0] === "";

  let content: ReactNode;
  if (!previewable) {
    content = (
      <EmptyState
        icon={FileWarning}
        title="File too large to preview"
        action={
          <Button size="sm" onClick={() => download.mutate()} disabled={download.isPending}>
            <Download aria-hidden="true" />
            Download {formatBytes(file.size)}
          </Button>
        }
      >
        Files over {formatBytes(PREVIEW_LIMIT)} are not loaded in the browser.
      </EmptyState>
    );
  } else if (blob.isPending) {
    content = <LoadingRows rows={8} label="Loading file" />;
  } else if (blob.isError) {
    content = <ErrorState error={blob.error} onRetry={() => void blob.refetch()} />;
  } else if (text === null) {
    content = (
      <EmptyState icon={Binary} title="Binary file">
        {formatBytes(file.size)} of binary data cannot be shown inline. Download it to inspect.
      </EmptyState>
    );
  } else if (empty) {
    content = <EmptyState title="Empty file" />;
  } else {
    content = <CodeView lines={lines} language={language} wrap={wrap} label={`Contents of ${file.path}`} />;
  }

  let preview: ReactNode = null;
  if (hasPreview && blob.data) {
    if (imageType) preview = <ImagePreview bytes={blob.data} type={imageType} alt={baseName(file.path)} />;
    else if (text !== null) preview = <MarkdownView source={text} resolveLink={resolveLink} />;
  } else if (hasPreview) {
    preview = blob.isError ? (
      <ErrorState error={blob.error} onRetry={() => void blob.refetch()} />
    ) : (
      <LoadingRows rows={6} label="Loading preview" />
    );
  }

  return (
    <TooltipProvider>
      <section aria-label={`File ${file.path}`} className="overflow-hidden rounded-xl border bg-card">
        <Tabs value={tab} onValueChange={(v) => setTab(v as TabId)} className="gap-0">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b px-3 py-2">
            <TabsList>
              {hasPreview && <TabsTrigger value="preview">Preview</TabsTrigger>}
              {hasCode && <TabsTrigger value="code">Code</TabsTrigger>}
              <TabsTrigger value="history">History</TabsTrigger>
            </TabsList>
            <dl
              className="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1 px-1 text-xs text-muted-foreground"
              aria-label="File details"
            >
              <div className="flex gap-1">
                <dt className="sr-only">Size</dt>
                <dd className="tabular-nums">{formatBytes(file.size)}</dd>
              </div>
              {text !== null && !empty && (
                <div className="flex gap-1">
                  <dt className="sr-only">Lines</dt>
                  <dd className="tabular-nums">{formatNumber(lines.length)} lines</dd>
                </div>
              )}
              {language && (
                <div className="flex gap-1">
                  <dt className="sr-only">Language</dt>
                  <dd>{language.name}</dd>
                </div>
              )}
              <div className="flex gap-1">
                <dt>hash</dt>
                <dd className="font-mono text-foreground" title={file.hash}>
                  {file.hash.slice(0, 12)}
                </dd>
              </div>
              {file.executable && (
                <div>
                  <dt className="sr-only">Mode</dt>
                  <dd>
                    <Badge variant="outline" className="font-mono">
                      executable
                    </Badge>
                  </dd>
                </div>
              )}
            </dl>
            <div className="flex items-center gap-1.5">
              {tab === "code" && text !== null && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Toggle variant="outline" size="sm" pressed={wrap} onPressedChange={setWrap} aria-label="Wrap lines">
                      <WrapText aria-hidden="true" />
                    </Toggle>
                  </TooltipTrigger>
                  <TooltipContent>Wrap lines</TooltipContent>
                </Tooltip>
              )}
              {text !== null && (
                <IconButton label={copied ? "Copied" : "Copy contents"} onClick={() => copy.mutate(text)} disabled={copy.isPending}>
                  {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
                </IconButton>
              )}
              <IconButton label="Download" onClick={() => download.mutate()} disabled={download.isPending}>
                <Download aria-hidden="true" />
              </IconButton>
            </div>
          </div>
          {hasPreview && <TabsContent value="preview">{preview}</TabsContent>}
          {hasCode && <TabsContent value="code">{content}</TabsContent>}
          <TabsContent value="history">
            <FileHistory tenant={tenant} tree={tree} path={file.path} branch={branch} current={snapshot} />
          </TabsContent>
        </Tabs>
      </section>
    </TooltipProvider>
  );
}
