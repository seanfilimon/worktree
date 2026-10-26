import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Archive, CalendarClock, Cloud, Download, HardDrive, Plus, ShieldX, TerminalSquare, Trash2 } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import {
  backupAuthor,
  backupFileName,
  backupKeys,
  backupTargetText,
  MAX_BACKUP_NOTE,
  useBackupsApi,
  useBackupStatus,
  type BackupInfo,
  type BackupStatus,
  type BackupStoreInfo,
} from "@/api/backups";
import { isNotImplemented } from "@/api/v05ops";
import { useAuth } from "@/auth/context";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/dashboard/states";
import { PageHeader } from "@/components/PageHeader";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { FormError } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { downloadBytes } from "@/lib/files";
import { absoluteTime, formatBytes, formatNumber, relativeTime } from "@/lib/format";
import { OutboxCard } from "@/pages/backups/OutboxCard";
import { RetentionCard } from "@/pages/backups/RetentionCard";
import { NotImplementedNotice } from "@/pages/tenants/opsShared";
import { useToast } from "@/toast/store";

const breadcrumbs = [{ label: "Backups" }];

/** "every 24h0m0s" / "off". */
function scheduleText(s: BackupStatus): string {
  if (!s.config.interval_seconds) return "Off";
  return `Every ${s.config.interval}`;
}

function storeRows(store: BackupStoreInfo): [string, string][] {
  const compression = store.compress ? "zstd (WTZ1) when it saves ≥ 10 %" : "off";
  if (store.kind === "s3") {
    return [
      ["Endpoint", `${store.secure ? "https" : "http"}://${store.endpoint ?? ""}`],
      ["Bucket", store.bucket || "—"],
      ["Key prefix", store.prefix || "(none)"],
      ["Region", store.region || "auto"],
      ["Compression", compression],
    ];
  }
  if (store.kind === "disk") {
    return [
      ["Directory", store.root ? `${store.root}/objects` : "—"],
      ["Compression", compression],
    ];
  }
  return [["Compression", compression]];
}

function StoreCard({ status }: { status: BackupStatus }) {
  const { store, config } = status;
  const s3 = store.kind === "s3";
  const Icon = s3 ? Cloud : HardDrive;
  const rows: [string, string][] = [
    ...storeRows(store),
    ["Backup target", backupTargetText(config)],
    ["Schedule", scheduleText(status)],
    ["Keep", config.keep ? `${formatNumber(config.keep)} scheduled backups` : "All"],
    ["Max age", config.max_age_seconds ? config.max_age : "None"],
  ];
  if (status.next_run_at) rows.push(["Next backup", `${relativeTime(status.next_run_at)} (${absoluteTime(status.next_run_at)})`]);
  return (
    <Card className="gap-0 pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle className="flex items-center gap-2">
          <Icon aria-hidden="true" className="size-4" />
          Object store
        </CardTitle>
        <CardDescription>
          Set with <span className="font-mono">--object-store</span>, <span className="font-mono">--s3-*</span>,{" "}
          <span className="font-mono">--backup-target</span>, <span className="font-mono">--backup-s3-prefix</span>,{" "}
          <span className="font-mono">--backup-interval</span>, <span className="font-mono">--backup-keep</span> and{" "}
          <span className="font-mono">--backup-max-age</span>.
        </CardDescription>
        <CardAction>
          <Badge variant="outline" className="font-mono uppercase">
            {store.kind || "unknown"}
          </Badge>
        </CardAction>
      </CardHeader>
      <dl aria-label="Backup configuration" className="divide-y text-sm">
        {rows.map(([k, v]) => (
          <div key={k} className="flex items-center justify-between gap-4 px-6 py-2.5">
            <dt className="text-muted-foreground">{k}</dt>
            <dd className="truncate text-right font-mono text-xs" title={v}>
              {v}
            </dd>
          </div>
        ))}
      </dl>
    </Card>
  );
}

function RestoreCard() {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <TerminalSquare aria-hidden="true" className="size-4" />
          Restoring
        </CardTitle>
        <CardDescription>
          Restore is offline: stop the server, then restore into an empty data directory. Every object is verified against its
          hash and the database against the manifest.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <pre className="overflow-x-auto rounded-md border bg-muted px-3 py-2 font-mono text-xs">
          {"worktree-server restore --data /var/lib/worktree <FILE|ID>\nworktree-server restore --list"}
        </pre>
        <p className="text-xs text-muted-foreground">
          Restore from an archive file or by backup id: an ID restores from the backup target (
          <span className="font-mono">&lt;data&gt;/backups</span> or the S3 bucket); <span className="font-mono">--list</span>{" "}
          shows the backups there.
        </p>
        <p className="text-xs text-muted-foreground">
          Pass the same <span className="font-mono">--object-store</span> / <span className="font-mono">--s3-*</span> /{" "}
          <span className="font-mono">--backup-target</span> flags as the server to restore objects into S3 or read backups from
          the bucket, and <span className="font-mono">--force</span> to restore over existing data.
        </p>
      </CardContent>
    </Card>
  );
}

function CreateBackupDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const api = useBackupsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [note, setNote] = useState("");

  const create = useMutation({
    mutationFn: () => api.create(note.trim() ? { note: note.trim() } : {}),
    meta: { inlineError: true },
    onSuccess: async (b) => {
      toast.success("Backup created", `${b.id} · ${formatNumber(b.objects)} objects · ${formatBytes(b.size_bytes)}`);
      change(false);
      await queryClient.invalidateQueries({ queryKey: backupKeys.status });
    },
  });

  function change(next: boolean) {
    if (!next) {
      setNote("");
      create.reset();
    }
    onOpenChange(next);
  }

  const tooLong = note.trim().length > MAX_BACKUP_NOTE;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (tooLong) return;
    create.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create a backup</DialogTitle>
          <DialogDescription>
            Snapshots the metadata database and copies every object into one archive on the server. Large servers can take a
            while; pushes keep working meanwhile.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={`${id}-note`}>Note (optional)</Label>
            <Input
              id={`${id}-note`}
              value={note}
              onChange={(e) => setNote(e.target.value)}
              placeholder="Before upgrading to 0.8"
              aria-invalid={tooLong || undefined}
            />
            {tooLong && <p className="text-xs text-destructive">At most {MAX_BACKUP_NOTE} characters.</p>}
          </div>
          {isNotImplemented(create.error) ? <NotImplementedNotice feature="Backup" /> : <FormError error={create.error} />}
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={create.isPending || tooLong}>
              {create.isPending ? "Backing up…" : "Create backup"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function BackupsTable({ status }: { status: BackupStatus }) {
  const api = useBackupsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<BackupInfo | null>(null);

  const download = useMutation({
    mutationFn: async (b: BackupInfo) => ({ b, bytes: await api.download(b.id) }),
    onSuccess: ({ b, bytes }) => downloadBytes(bytes, backupFileName(b.id)),
  });
  const remove = useMutation({
    mutationFn: (b: BackupInfo) => api.remove(b.id),
    onSuccess: async (_, b) => {
      toast.success("Backup deleted", b.id);
      await queryClient.invalidateQueries({ queryKey: backupKeys.status });
    },
  });

  const backups = status.backups;
  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle className="flex items-center gap-2">
          <Archive aria-hidden="true" className="size-4" />
          Backups
        </CardTitle>
        <CardDescription>
          {backups.length
            ? `${formatNumber(backups.length)} on the server, newest first.`
            : "Archives of the metadata database and every object."}
          {status.running && " A backup is running."}
        </CardDescription>
        <CardAction>
          <Button size="sm" disabled={!status.supported || status.running} onClick={() => setCreating(true)}>
            <Plus aria-hidden="true" />
            Create backup
          </Button>
        </CardAction>
      </CardHeader>
      {!status.supported && (
        <p className="border-b px-6 py-3 text-sm text-muted-foreground">
          This server's stores cannot be backed up (the metadata store must be the bbolt database and the object store must be
          able to list its objects).
        </p>
      )}
      {status.last_error && (
        <p role="alert" className="border-b px-6 py-3 text-sm">
          <span className="font-medium">Last scheduled backup failed:</span>{" "}
          <span className="font-mono text-xs text-muted-foreground">{status.last_error}</span>
        </p>
      )}
      {backups.length === 0 ? (
        <EmptyState icon={Archive} title="No backups yet">
          Create one now, or schedule them with <span className="font-mono">--backup-interval</span>.
        </EmptyState>
      ) : (
        <div className="max-h-[32rem] overflow-auto">
          <Table aria-label="Backups">
            <TableHeader className="sticky top-0 bg-card">
              <TableRow className="hover:bg-transparent">
                <TableHead className="pl-6">Backup</TableHead>
                <TableHead>Created</TableHead>
                <TableHead className="hidden sm:table-cell">By</TableHead>
                <TableHead className="text-right">Size</TableHead>
                <TableHead className="hidden text-right md:table-cell">Objects</TableHead>
                <TableHead className="hidden lg:table-cell">Note</TableHead>
                <TableHead className="pr-6 text-right">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {backups.map((b) => (
                <TableRow key={b.id}>
                  <TableCell className="pl-6 font-mono text-xs" title={b.path}>
                    {b.id}
                  </TableCell>
                  <TableCell>
                    <time dateTime={b.created_at} title={absoluteTime(b.created_at)}>
                      {relativeTime(b.created_at)}
                    </time>
                  </TableCell>
                  <TableCell className="hidden sm:table-cell">
                    {b.created_by === "system" ? (
                      <Badge variant="outline">
                        <CalendarClock aria-hidden="true" />
                        schedule
                      </Badge>
                    ) : (
                      backupAuthor(b)
                    )}
                  </TableCell>
                  <TableCell className="text-right font-mono text-xs tabular-nums">{formatBytes(b.size_bytes)}</TableCell>
                  <TableCell className="hidden text-right font-mono text-xs tabular-nums md:table-cell">
                    {b.objects ? formatNumber(b.objects) : "—"}
                  </TableCell>
                  <TableCell className="hidden max-w-64 truncate text-muted-foreground lg:table-cell" title={b.note}>
                    {b.note || "—"}
                  </TableCell>
                  <TableCell className="pr-6">
                    <div className="flex justify-end gap-1">
                      <Button
                        variant="ghost"
                        size="icon"
                        aria-label={`Download ${b.id}`}
                        disabled={download.isPending && download.variables?.id === b.id}
                        onClick={() => download.mutate(b)}
                      >
                        <Download aria-hidden="true" />
                      </Button>
                      <Button variant="ghost" size="icon" aria-label={`Delete ${b.id}`} onClick={() => setDeleting(b)}>
                        <Trash2 aria-hidden="true" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
      <CreateBackupDialog open={creating} onOpenChange={setCreating} />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title="Delete this backup?"
        description={
          <>
            <span className="font-mono">{deleting?.id}</span> is removed from the server permanently. Download it first if you
            want to keep a copy.
          </>
        }
        confirmLabel="Delete backup"
        pending={remove.isPending}
        onConfirm={() => (deleting ? remove.mutateAsync(deleting) : Promise.resolve())}
      />
    </Card>
  );
}

/**
 * Server backups: list, create, download, delete; object store and backup
 * target info; the email outbox and data retention (server administrators).
 */
export function BackupsPage() {
  const { isAdmin } = useAuth();
  const status = useBackupStatus(isAdmin);

  if (!isAdmin) {
    return (
      <>
        <PageHeader title="Backups" breadcrumbs={breadcrumbs} />
        <Card className="py-0">
          <EmptyState icon={ShieldX} title="Server administrators only">
            Backups contain every tree and account on the server and are restricted to server administrators.
          </EmptyState>
        </Card>
      </>
    );
  }

  return (
    <>
      <PageHeader
        title="Backups"
        breadcrumbs={breadcrumbs}
        description="Point-in-time archives of the metadata database and every object, the email outbox and data retention."
      />
      <div className="flex flex-col gap-6">
        {isNotImplemented(status.error) ? (
          <NotImplementedNotice feature="Backup" />
        ) : status.isError ? (
          <ErrorState className="m-0" error={status.error} onRetry={() => void status.refetch()} />
        ) : status.isPending ? (
          <Card className="py-0">
            <SkeletonRows rows={5} label="Loading backups" />
          </Card>
        ) : (
          <>
            <BackupsTable status={status.data} />
            <div className="grid gap-4 lg:grid-cols-2">
              <StoreCard status={status.data} />
              <RestoreCard />
            </div>
          </>
        )}
        {/* v0.8 server operations (§16.11); each card handles older servers itself. */}
        <OutboxCard enabled={isAdmin} />
        <RetentionCard enabled={isAdmin} />
      </div>
    </>
  );
}
