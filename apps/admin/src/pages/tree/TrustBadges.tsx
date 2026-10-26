/**
 * v0.7 trust (docs/ENGINEERING_PLAN.md §15.8): verified badges, checksum
 * display and SBOM generation shared by the Tags and Releases tabs.
 */
import { useMutation, useQuery } from "@tanstack/react-query";
import { Check, Copy, Download, FileCheck2, ScrollText, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { errorMessage } from "@/api/client";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { trustKeys, useTrustApi, type TrustedAsset } from "./trust";
import { saveBlob } from "./v05/util";

/** "Verified" badge for a signed tag or a release of one; nothing otherwise. */
export function VerifiedBadge({ verified, signer, keyId, className }: { verified?: boolean; signer?: string; keyId?: string; className?: string }) {
  if (!verified) return null;
  const who = signer ? `Signed by ${signer}` : "Signed";
  const title = keyId ? `${who} (key ${keyId}); verified by the server` : `${who}; verified by the server`;
  return (
    <Badge variant="outline" className={cn("gap-1", className)} title={title} aria-label={`Verified: ${who.toLowerCase()}`}>
      <ShieldCheck aria-hidden="true" />
      Verified
    </Badge>
  );
}

/** First 12 hex digits of a checksum (the full value in the title). */
export function ShortChecksum({ value }: { value?: string }) {
  if (!value) return <span className="text-muted-foreground">—</span>;
  return (
    <code className="font-mono text-xs" title={`sha256:${value}`} aria-label={`SHA-256 ${value}`}>
      {value.slice(0, 12)}
    </code>
  );
}

/** Dialog showing (and copying/downloading) a release's SHA256SUMS. */
export function ChecksumsButton({ tenant, tree, tag }: { tenant: string; tree: string; tag: string }) {
  const api = useTrustApi();
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const sums = useQuery({
    queryKey: trustKeys.checksums(tenant, tree, tag),
    queryFn: () => api.checksums(tenant, tree, tag),
    enabled: open,
  });
  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  };
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="ghost" size="sm" aria-label={`Checksums of ${tag}`}>
          <FileCheck2 aria-hidden="true" />
          Checksums
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Checksums of {tag}</DialogTitle>
          <DialogDescription>
            SHA-256 of every asset. Check downloads with <code className="font-mono">wt release verify {tag} -d DIR</code> or{" "}
            <code className="font-mono">sha256sum -c SHA256SUMS</code>.
          </DialogDescription>
        </DialogHeader>
        {sums.isPending ? (
          <p className="text-sm text-muted-foreground">Loading checksums…</p>
        ) : sums.isError ? (
          <p className="text-sm">{errorMessage(sums.error)}</p>
        ) : sums.data.trim() === "" ? (
          <p className="text-sm text-muted-foreground">No assets.</p>
        ) : (
          <>
            <pre aria-label="SHA256SUMS" className="max-h-80 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs break-all whitespace-pre-wrap">
              {sums.data}
            </pre>
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => void copy(sums.data)}>
                {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
                {copied ? "Copied" : "Copy"}
              </Button>
              <Button variant="outline" size="sm" onClick={() => saveBlob(new Blob([sums.data], { type: "text/plain" }), "SHA256SUMS")}>
                <Download aria-hidden="true" />
                Download
              </Button>
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Generate (or regenerate) the release's SPDX SBOM. */
export function GenerateSbomButton({
  tenant,
  tree,
  tag,
  exists,
  onDone,
}: {
  tenant: string;
  tree: string;
  tag: string;
  exists: boolean;
  onDone: (asset: TrustedAsset) => void | Promise<unknown>;
}) {
  const api = useTrustApi();
  const generate = useMutation({
    mutationFn: () => api.generateSbom(tenant, tree, tag),
    onSuccess: (asset) => onDone(asset),
  });
  return (
    <Button variant="ghost" size="sm" disabled={generate.isPending} onClick={() => generate.mutate()}>
      <ScrollText aria-hidden="true" />
      {generate.isPending ? "Generating SBOM…" : exists ? "Regenerate SBOM" : "Generate SBOM"}
    </Button>
  );
}
