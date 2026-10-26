import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Download, FileArchive, FlaskConical, Package, PencilLine, Plus, ShieldCheck, Tag as TagIcon, Trash2, Upload } from "lucide-react";
import { useId, useRef, useState, type FormEvent } from "react";
import { Link } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { keys, useTags } from "@/api/queries";
import { isNotImplemented, useReleases, useTreeV05Api, v05Keys, type Release, type ReleaseAsset } from "@/api/v05tree";
import { ConfirmDelete } from "@/components/files/ConfirmDelete";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { formatBytes, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";
import { Markdown } from "./history/Markdown";
import { RevisionSelect } from "./history/RevisionSelect";
import { EmptyState, LoadingRows, QueryError, RelativeTime } from "./history/shared";
import { SBOM_ASSET, type TrustedAsset, type TrustedRelease } from "./trust";
import { ChecksumsButton, GenerateSbomButton, ShortChecksum, VerifiedBadge } from "./TrustBadges";
import { FeatureUnavailable } from "./v05/shared";
import { saveBlob, useCanWrite } from "./v05/util";

/** Largest asset the server accepts (§13.5). */
const MAX_ASSET_BYTES = 64 * 1024 * 1024;

function useInvalidateReleases() {
  const { tenant, tree } = useTreeContext();
  const queryClient = useQueryClient();
  return () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: v05Keys.releases(tenant, tree) }),
      queryClient.invalidateQueries({ queryKey: keys.tags(tenant, tree) }),
    ]);
}

function tagNameError(name: string): string | null {
  if (!name) return "Enter a tag name.";
  if (/\s/.test(name)) return "Tag names cannot contain spaces.";
  if (name.includes("/")) return "Tag names cannot contain /.";
  if (name.startsWith("-") || name.includes("..")) return "Tag names cannot start with - or contain ..";
  return null;
}

interface ReleaseForm {
  tag: string;
  name: string;
  notes: string;
  draft: boolean;
  prerelease: boolean;
  snapshot: string;
}

/** Create (no `release`) or edit a release. */
function ReleaseDialog({ release, onClose }: { release?: Release; onClose: () => void }) {
  const { tenant, tree, summary } = useTreeContext();
  const api = useTreeV05Api();
  const tags = useTags(tenant, tree);
  const toast = useToast();
  const invalidate = useInvalidateReleases();
  const id = useId();
  const editing = release !== undefined;
  const [form, setForm] = useState<ReleaseForm>({
    tag: release?.tag ?? "",
    name: release?.name ?? "",
    notes: release?.notes ?? "",
    draft: release?.draft ?? false,
    prerelease: release?.prerelease ?? false,
    snapshot: summary.tree.default_branch,
  });
  const [touched, setTouched] = useState(false);
  const set = <K extends keyof ReleaseForm>(k: K, v: ReleaseForm[K]) => setForm((f) => ({ ...f, [k]: v }));

  const tag = form.tag.trim();
  const existingTag = (tags.data ?? []).find((t) => t.name === tag);

  const save = useMutation({
    mutationFn: () =>
      editing
        ? api.releases.update(tenant, tree, release.tag, {
            name: form.name.trim(),
            notes: form.notes,
            draft: form.draft,
            prerelease: form.prerelease,
          })
        : api.releases.create(tenant, tree, {
            tag,
            name: form.name.trim() || tag,
            notes: form.notes,
            draft: form.draft,
            prerelease: form.prerelease,
            snapshot: existingTag ? undefined : form.snapshot,
          }),
    meta: { inlineError: true },
    onSuccess: async (r) => {
      toast.success(editing ? "Release updated" : "Release created", r.name || r.tag);
      onClose();
      await invalidate();
    },
  });

  const tagError = !editing && touched ? tagNameError(tag) : null;
  const serverError =
    save.error &&
    (isApiError(save.error) && save.error.status === 409 ? `A release for tag “${tag}” already exists.` : errorMessage(save.error));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (!editing && tagNameError(tag)) return;
    save.mutate();
  };

  return (
    <DialogContent className="sm:max-w-lg">
      <form onSubmit={submit} aria-label={editing ? "Edit release" : "Create release"} className="grid gap-4" noValidate>
        <DialogHeader>
          <DialogTitle>{editing ? `Edit release ${release.tag}` : "New release"}</DialogTitle>
          <DialogDescription>
            {editing ? "The tag and its snapshot cannot change." : "A release names and annotates a tag; the tag is created when it does not exist."}
          </DialogDescription>
        </DialogHeader>
        {!editing && (
          <div className="grid gap-2">
            <Label htmlFor={`${id}-tag`}>Tag</Label>
            <Input
              id={`${id}-tag`}
              value={form.tag}
              autoFocus
              autoComplete="off"
              spellCheck={false}
              placeholder="v1.2.0"
              className="font-mono"
              list={`${id}-tags`}
              aria-invalid={tagError ? true : undefined}
              aria-describedby={`${id}-tag-hint`}
              onChange={(e) => set("tag", e.target.value)}
            />
            <datalist id={`${id}-tags`}>
              {(tags.data ?? []).map((t) => (
                <option key={t.name} value={t.name} />
              ))}
            </datalist>
            <p id={`${id}-tag-hint`} className={tagError ? "text-xs font-medium" : "text-xs text-muted-foreground"}>
              {tagError ??
                (existingTag
                  ? `Existing tag → snapshot ${shortId(existingTag.target)}.`
                  : tag
                    ? "New tag: created at the revision below."
                    : "An existing tag, or a new one to create.")}
            </p>
          </div>
        )}
        {!editing && !existingTag && (
          <RevisionSelect
            tenant={tenant}
            tree={tree}
            branch={summary.tree.default_branch}
            label="Create tag at"
            value={form.snapshot}
            onChange={(v) => set("snapshot", v)}
          />
        )}
        <div className="grid gap-2">
          <Label htmlFor={`${id}-name`}>
            Name {!editing && <span className="font-normal text-muted-foreground">(defaults to the tag)</span>}
          </Label>
          <Input id={`${id}-name`} value={form.name} autoComplete="off" onChange={(e) => set("name", e.target.value)} />
        </div>
        <div className="grid gap-2">
          <Label htmlFor={`${id}-notes`}>
            Release notes <span className="font-normal text-muted-foreground">(Markdown)</span>
          </Label>
          <Textarea id={`${id}-notes`} value={form.notes} rows={6} className="font-mono text-xs" onChange={(e) => set("notes", e.target.value)} />
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          <div className="flex items-center justify-between gap-3 rounded-md border p-3">
            <Label htmlFor={`${id}-draft`}>Draft</Label>
            <Switch id={`${id}-draft`} checked={form.draft} onCheckedChange={(v) => set("draft", v)} />
          </div>
          <div className="flex items-center justify-between gap-3 rounded-md border p-3">
            <Label htmlFor={`${id}-pre`}>Pre-release</Label>
            <Switch id={`${id}-pre`} checked={form.prerelease} onCheckedChange={(v) => set("prerelease", v)} />
          </div>
        </div>
        {serverError && (
          <Alert>
            <AlertDescription className="text-foreground">{serverError}</AlertDescription>
          </Alert>
        )}
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" disabled={save.isPending}>
            {editing ? "Save release" : save.isPending ? "Creating…" : "Create release"}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  );
}

function UploadAssetButton({ release }: { release: Release }) {
  const { tenant, tree } = useTreeContext();
  const api = useTreeV05Api();
  const toast = useToast();
  const invalidate = useInvalidateReleases();
  const input = useRef<HTMLInputElement>(null);

  const upload = useMutation({
    mutationFn: async (files: File[]) => {
      for (const f of files) {
        if (f.size > MAX_ASSET_BYTES) throw new Error(`${f.name} is larger than 64 MiB.`);
      }
      for (const f of files) await api.releases.uploadAsset(tenant, tree, release.tag, f, f.name);
      return files;
    },
    onSuccess: async (files) => {
      toast.success(files.length === 1 ? "Asset uploaded" : `${files.length} assets uploaded`, files.map((f) => f.name).join(", "));
      await invalidate();
    },
    onSettled: () => {
      if (input.current) input.current.value = "";
    },
  });

  return (
    <>
      <input
        ref={input}
        type="file"
        multiple
        className="sr-only"
        tabIndex={-1}
        aria-label={`Upload assets to ${release.tag}`}
        onChange={(e) => {
          const files = [...(e.target.files ?? [])];
          if (files.length) upload.mutate(files);
        }}
      />
      <Button variant="outline" size="sm" disabled={upload.isPending} onClick={() => input.current?.click()}>
        <Upload aria-hidden="true" />
        {upload.isPending ? "Uploading…" : "Upload assets"}
      </Button>
    </>
  );
}

function AssetRow({ release, asset, canWrite, canDelete }: { release: Release; asset: ReleaseAsset; canWrite: boolean; canDelete: boolean }) {
  const { tenant, tree } = useTreeContext();
  const api = useTreeV05Api();
  const toast = useToast();
  const invalidate = useInvalidateReleases();

  const download = useMutation({
    mutationFn: () => api.releases.downloadAsset(tenant, tree, release.tag, asset.name),
    onSuccess: (blob) => saveBlob(blob, asset.name),
  });
  const remove = useMutation({
    mutationFn: () => api.releases.removeAsset(tenant, tree, release.tag, asset.name),
    onSuccess: async () => {
      toast.success("Asset deleted", asset.name);
      await invalidate();
    },
  });

  return (
    <TableRow>
      <TableCell>
        <span className="flex items-center gap-1.5 font-mono text-xs font-medium">
          <FileArchive aria-hidden="true" className="size-3.5 text-muted-foreground" />
          {asset.name}
        </span>
      </TableCell>
      <TableCell className="text-right text-xs tabular-nums">{formatBytes(asset.size)}</TableCell>
      <TableCell className="hidden font-mono text-xs text-muted-foreground md:table-cell">{asset.content_type}</TableCell>
      <TableCell className="hidden text-xs text-muted-foreground sm:table-cell">
        <RelativeTime value={asset.uploaded_at} />
      </TableCell>
      {/* v0.7 trust: recorded checksum; verified when the release's tag is. */}
      <TableCell className="hidden lg:table-cell">
        <span className="inline-flex items-center gap-1.5">
          <ShortChecksum value={(asset as TrustedAsset).sha256} />
          {(release as TrustedRelease).verified && (asset as TrustedAsset).sha256 && (
            <ShieldCheck aria-label="Checksum of a verified release" className="size-3.5 text-muted-foreground" />
          )}
        </span>
      </TableCell>
      <TableCell className="text-right whitespace-nowrap">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Download ${asset.name}`}
          disabled={download.isPending}
          onClick={() => download.mutate()}
        >
          <Download aria-hidden="true" />
        </Button>
        {canWrite && canDelete && (
          <ConfirmDelete
            label={`Delete asset ${asset.name}`}
            title={`Delete ${asset.name}?`}
            onConfirm={() => remove.mutate()}
            pending={remove.isPending}
          >
            The file is removed from release {release.tag}.
          </ConfirmDelete>
        )}
      </TableCell>
    </TableRow>
  );
}

function ReleaseCard({ release: r, canWrite, canDelete, onEdit }: { release: Release; canWrite: boolean; canDelete: boolean; onEdit: () => void }) {
  const { tenant, tree } = useTreeContext();
  const api = useTreeV05Api();
  const toast = useToast();
  const invalidate = useInvalidateReleases();
  const remove = useMutation({
    mutationFn: () => api.releases.remove(tenant, tree, r.tag),
    onSuccess: async () => {
      toast.success("Release deleted", r.tag);
      await invalidate();
    },
  });
  // v0.7 trust (§15.8): verified releases, checksums and the SBOM.
  const trust = r as TrustedRelease;
  const sbom = r.assets.find((a) => a.name === SBOM_ASSET);
  const downloadSbom = useMutation({
    mutationFn: () => api.releases.downloadAsset(tenant, tree, r.tag, SBOM_ASSET),
    onSuccess: (blob) => saveBlob(blob, SBOM_ASSET),
  });

  return (
    <article aria-label={`Release ${r.name || r.tag}`} className="rounded-xl border bg-card">
      <header className="flex flex-wrap items-start justify-between gap-3 border-b px-4 py-3">
        <div className="flex min-w-0 flex-col gap-1">
          <h3 className="flex flex-wrap items-center gap-2 text-base font-semibold">
            <span className="break-words">{r.name || r.tag}</span>
            {r.draft && (
              <Badge variant="outline" className="border-dashed">
                draft
              </Badge>
            )}
            {r.prerelease && (
              <Badge variant="secondary">
                <FlaskConical aria-hidden="true" />
                pre-release
              </Badge>
            )}
            <VerifiedBadge verified={trust.verified} signer={trust.signer} keyId={trust.signing_key_id} />
          </h3>
          <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
            <span className="inline-flex items-center gap-1 font-mono text-foreground">
              <TagIcon aria-hidden="true" className="size-3" />
              {r.tag}
            </span>
            {r.snapshot_id && (
              <Link
                to={treePath(tenant, tree, `snapshots/${encodeURIComponent(r.snapshot_id)}`)}
                className="font-mono hover:underline"
                title={r.snapshot_id}
              >
                {shortId(r.snapshot_id)}
              </Link>
            )}
            <span>
              by <span className="text-foreground">{r.author}</span> <RelativeTime value={r.published_at ?? r.created_at} />
            </span>
          </p>
        </div>
        {canWrite && (
          <div className="flex flex-wrap gap-1">
            <UploadAssetButton release={r} />
            <Button variant="ghost" size="icon-sm" aria-label={`Edit release ${r.tag}`} onClick={onEdit}>
              <PencilLine aria-hidden="true" />
            </Button>
            {canDelete && (
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button variant="ghost" size="icon-sm" aria-label={`Delete release ${r.tag}`} disabled={remove.isPending}>
                    <Trash2 aria-hidden="true" />
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Delete release {r.tag}?</AlertDialogTitle>
                    <AlertDialogDescription>
                      The release and its {r.assets.length} {r.assets.length === 1 ? "asset are" : "assets are"} deleted. The tag is kept.
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction onClick={() => remove.mutate()}>
                      <Trash2 aria-hidden="true" />
                      Delete release
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            )}
          </div>
        )}
      </header>
      <div className="flex flex-col gap-4 px-4 py-3">
        {r.notes?.trim() ? (
          <div className="text-sm">
            <Markdown source={r.notes} />
          </div>
        ) : (
          <p className="text-sm text-muted-foreground italic">No release notes.</p>
        )}
        {r.assets.length > 0 ? (
          <Table aria-label={`Assets of ${r.tag}`}>
            <TableHeader>
              <TableRow>
                <TableHead>Asset</TableHead>
                <TableHead className="text-right">Size</TableHead>
                <TableHead className="hidden md:table-cell">Type</TableHead>
                <TableHead className="hidden sm:table-cell">Uploaded</TableHead>
                <TableHead className="hidden lg:table-cell">SHA-256</TableHead>
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {r.assets.map((a) => (
                <AssetRow key={a.name} release={r} asset={a} canWrite={canWrite} canDelete={canDelete} />
              ))}
            </TableBody>
          </Table>
        ) : (
          <p className="text-xs text-muted-foreground">No assets.</p>
        )}
        <div className="flex flex-wrap items-center gap-1" aria-label={`Integrity of ${r.tag}`} role="group">
          {r.assets.length > 0 && <ChecksumsButton tenant={tenant} tree={tree} tag={r.tag} />}
          {sbom && (
            <Button variant="ghost" size="sm" disabled={downloadSbom.isPending} onClick={() => downloadSbom.mutate()}>
              <Download aria-hidden="true" />
              Download SBOM
            </Button>
          )}
          {canWrite && (
            <GenerateSbomButton
              tenant={tenant}
              tree={tree}
              tag={r.tag}
              exists={sbom !== undefined}
              onDone={async () => {
                toast.success("SBOM attached", SBOM_ASSET);
                await invalidate();
              }}
            />
          )}
        </div>
      </div>
    </article>
  );
}

export function ReleasesTab() {
  const { tenant, tree, canManage } = useTreeContext();
  const releases = useReleases(tenant, tree);
  const canWrite = useCanWrite();
  const [dialog, setDialog] = useState<{ release?: Release } | null>(null);

  if (releases.isError && isNotImplemented(releases.error)) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Releases</CardTitle>
        </CardHeader>
        <CardContent>
          <FeatureUnavailable feature="Releases" />
        </CardContent>
      </Card>
    );
  }

  const list = [...(releases.data ?? [])].sort((a, b) => b.created_at.localeCompare(a.created_at));

  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle>Releases</CardTitle>
        <CardDescription>Named, annotated versions of this tree with downloadable assets. Newest first.</CardDescription>
        {canWrite && (
          <CardAction>
            <Button size="sm" onClick={() => setDialog({})}>
              <Plus aria-hidden="true" />
              New release
            </Button>
          </CardAction>
        )}
      </CardHeader>
      <CardContent>
        {releases.isPending ? (
          <LoadingRows label="Loading releases" rows={4} />
        ) : releases.isError ? (
          <QueryError error={releases.error} onRetry={() => void releases.refetch()} />
        ) : list.length === 0 ? (
          <EmptyState title="No releases" icon={<Package aria-hidden="true" />}>
            Create one here or with <code className="font-mono">wt release create TAG</code>.
          </EmptyState>
        ) : (
          <section aria-label="Releases" className="flex flex-col gap-4">
            {list.map((r) => (
              <ReleaseCard key={r.tag} release={r} canWrite={canWrite} canDelete={canManage} onEdit={() => setDialog({ release: r })} />
            ))}
          </section>
        )}
      </CardContent>
      <Dialog open={dialog !== null} onOpenChange={(open) => !open && setDialog(null)}>
        {dialog && <ReleaseDialog key={dialog.release?.tag ?? "new"} release={dialog.release} onClose={() => setDialog(null)} />}
      </Dialog>
    </Card>
  );
}
