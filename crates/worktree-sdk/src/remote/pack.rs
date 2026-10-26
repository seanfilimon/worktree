//! Pack transfer (ENGINEERING_PLAN §16.1, `specs/sync/Packs.md`).
//!
//! Push uploads the missing objects as one pack (`UploadPack`, deltas
//! against the server-known parent snapshot), resuming an interrupted
//! upload once from the offset `QueryUpload` reports. Fetch, pull and
//! clone download the hinted objects as one pack (`DownloadPack`, deltas
//! against the base snapshots the client names in `have`). Servers without
//! the pack RPCs answer `Unimplemented`; the client then remembers that for
//! the session and transfers whole objects (`UploadObjects` /
//! `DownloadObjects`). `WT_NO_PACK=1` (or `wt push --no-pack`) skips packs.

use std::collections::{HashMap, HashSet};
use std::io::{BufWriter, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Status};
use worktree_protocol::pack::{self, PackReader, PackWriter, WriterOptions, MAX_CHUNK};
use worktree_protocol::proto::sync as wire;

use super::client::{ObjectKind, SyncClient, MAX_OBJECT_SIZE};
use super::convert::{tree_id, wire_hash};
use crate::error::{Result, SdkError};
use crate::model::{ManifestEntry, Snapshot};
use crate::repo::Repository;
use crate::store::{ObjectHash, ObjectStore};

/// Environment variable that disables pack transfers (`1`, `true`, `yes`).
pub const NO_PACK_ENV: &str = "WT_NO_PACK";
/// Testing hook: abort the first pack upload stream once this many bytes
/// were sent (chunks shrink to 64 KiB so the cut lands inside the pack),
/// so the resume path runs against a real server.
pub const TEST_INTERRUPT_ENV: &str = "WT_PACK_TEST_INTERRUPT_AFTER";
/// Most `have` hashes sent with one `DownloadPack`.
const MAX_HAVES: usize = 100_000;
/// Chunk size while the interrupt hook is active.
const TEST_CHUNK: usize = 64 * 1024;
/// How often a resume waits for the server to release the interrupted
/// upload (`Aborted: already in progress`).
const BUSY_RETRIES: usize = 20;

/// Whether `WT_NO_PACK` disables packs.
pub fn packs_disabled_by_env() -> bool {
    std::env::var(NO_PACK_ENV)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn test_interrupt_after() -> Option<u64> {
    std::env::var(TEST_INTERRUPT_ENV)
        .ok()
        .and_then(|v| v.trim().parse().ok())
}

/// What one object transfer (a push upload or a fetch download) sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferStats {
    /// The objects travelled in a pack (false: whole objects).
    pub pack: bool,
    pub objects: usize,
    /// Objects sent as deltas.
    pub deltas: usize,
    /// Total size of the objects (what a whole-object transfer sends
    /// before gRPC compression).
    pub object_bytes: u64,
    /// Pack bytes for pack transfers, else equal to `object_bytes`.
    pub wire_bytes: u64,
    /// An interrupted pack upload was resumed.
    pub resumed: bool,
}

impl TransferStats {
    /// Add another transfer's numbers.
    pub fn absorb(&mut self, other: &TransferStats) {
        self.pack |= other.pack;
        self.objects += other.objects;
        self.deltas += other.deltas;
        self.object_bytes += other.object_bytes;
        self.wire_bytes += other.wire_bytes;
        self.resumed |= other.resumed;
    }
}

fn kind_byte(kind: ObjectKind) -> u8 {
    wire::ObjectKind::from(kind) as i32 as u8
}

fn pack_err(e: pack::PackError) -> SdkError {
    SdkError::Remote(format!("pack transfer failed: {e}"))
}

// ---------------------------------------------------------------------
// Base selection
// ---------------------------------------------------------------------

/// One object of an upload with its delta base candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UploadItem {
    pub hash: ObjectHash,
    pub kind: ObjectKind,
    pub base: Option<ObjectHash>,
}

/// Order `to_send` for a pack (snapshot by snapshot, oldest first: entry
/// objects, then the manifest) and pick each object's base: the object at
/// the same path in the first parent's manifest (for chunks: the chunk at
/// the same index of that path's chunk list), and the parent manifest for
/// a manifest.
pub(crate) fn upload_items(
    repo: &Repository,
    chain: &[Snapshot],
    to_send: &[(ObjectHash, ObjectKind)],
) -> Vec<UploadItem> {
    let store = repo.store();
    let wanted: HashMap<ObjectHash, ObjectKind> = to_send.iter().copied().collect();
    let mut added = HashSet::with_capacity(to_send.len());
    let mut items = Vec::with_capacity(to_send.len());
    let mut push = |items: &mut Vec<UploadItem>, hash: ObjectHash, base: Option<ObjectHash>| {
        if let Some(&kind) = wanted.get(&hash) {
            if added.insert(hash) {
                items.push(UploadItem {
                    hash,
                    kind,
                    base: base.filter(|b| *b != hash),
                });
            }
        }
    };
    for snap in chain {
        let Ok(manifest) = repo.read_manifest(&snap.manifest) else {
            continue;
        };
        let parent = snap
            .parents
            .first()
            .and_then(|p| repo.get_snapshot(p).ok())
            .and_then(|s| {
                repo.read_manifest(&s.manifest)
                    .ok()
                    .map(|m| (s.manifest, m))
            });
        let by_path: HashMap<&str, &ManifestEntry> = parent
            .as_ref()
            .map(|(_, m)| m.entries.iter().map(|e| (e.path.as_str(), e)).collect())
            .unwrap_or_default();
        for e in &manifest.entries {
            let prev = by_path.get(e.path.as_str()).filter(|b| b.hash != e.hash);
            if e.chunked {
                if let Ok(list) = store.read_chunk_list_local(&e.hash) {
                    let prev_chunks = prev
                        .filter(|b| b.chunked)
                        .and_then(|b| store.read_chunk_list_local(&b.hash).ok());
                    for (i, c) in list.chunks.iter().enumerate() {
                        let base = prev_chunks
                            .as_ref()
                            .and_then(|l| l.chunks.get(i))
                            .map(|c| c.hash);
                        push(&mut items, c.hash, base);
                    }
                }
                push(
                    &mut items,
                    e.hash,
                    prev.filter(|b| b.chunked).map(|b| b.hash),
                );
            } else {
                push(
                    &mut items,
                    e.hash,
                    prev.filter(|b| !b.chunked).map(|b| b.hash),
                );
            }
        }
        push(&mut items, snap.manifest, parent.map(|(h, _)| h));
    }
    for &(hash, _) in to_send {
        push(&mut items, hash, None);
    }
    items
}

/// Objects the server is known to store besides those negotiated: the
/// manifests, entries and chunks of the server-known parents of `chain`
/// (valid delta bases for an upload).
pub(crate) fn server_known_objects(
    repo: &Repository,
    chain: &[Snapshot],
    known: &HashSet<String>,
) -> HashSet<ObjectHash> {
    let store = repo.store();
    let mut out = HashSet::new();
    let parents: HashSet<&str> = chain
        .iter()
        .flat_map(|s| s.parents.iter())
        .filter(|p| known.contains(p.as_str()))
        .map(String::as_str)
        .collect();
    for parent in parents {
        let Ok(snap) = repo.get_snapshot(parent) else {
            continue;
        };
        let Ok(manifest) = repo.read_manifest(&snap.manifest) else {
            continue;
        };
        out.insert(snap.manifest);
        for e in &manifest.entries {
            out.insert(e.hash);
            if e.chunked {
                if let Ok(list) = store.read_chunk_list_local(&e.hash) {
                    out.extend(list.chunks.iter().map(|c| c.hash));
                }
            }
        }
    }
    out
}

/// `have` hashes for a download: the manifests of the local `snapshots`
/// first (the server looks for base manifests there), then their entries
/// and chunks that are stored locally; at most [`MAX_HAVES`].
pub(crate) fn download_haves(repo: &Repository, snapshots: &[&str]) -> Vec<ObjectHash> {
    let store = repo.store();
    let mut manifests = Vec::new();
    for id in snapshots {
        if let Ok(s) = repo.get_snapshot(id) {
            if store.has(&s.manifest) && !manifests.contains(&s.manifest) {
                manifests.push(s.manifest);
            }
        }
    }
    let mut seen: HashSet<ObjectHash> = manifests.iter().copied().collect();
    let mut out = manifests.clone();
    'outer: for mh in &manifests {
        let Ok(m) = repo.read_manifest(mh) else {
            continue;
        };
        for e in &m.entries {
            let mut hashes = vec![e.hash];
            if e.chunked {
                if let Ok(list) = store.read_chunk_list_local(&e.hash) {
                    hashes.extend(list.chunks.iter().map(|c| c.hash));
                }
            }
            for h in hashes {
                if out.len() >= MAX_HAVES {
                    break 'outer;
                }
                if store.has(&h) && seen.insert(h) {
                    out.push(h);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------
// Upload
// ---------------------------------------------------------------------

/// Removes the temporary pack file.
struct TempPack(PathBuf);

impl Drop for TempPack {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Write the pack of `items` to `path`. A base is used only when the
/// server has it (`server_has`) or it is an earlier entry, and when it is
/// stored locally.
fn build_pack(
    store: &ObjectStore,
    items: &[UploadItem],
    server_has: &HashSet<ObjectHash>,
    path: &Path,
) -> Result<pack::PackStats> {
    let file = std::fs::File::create(path)?;
    let mut w = PackWriter::new(BufWriter::new(file), WriterOptions::default())?;
    for it in items {
        let data = store.get(&it.hash)?;
        if data.len() > MAX_OBJECT_SIZE {
            return Err(SdkError::Remote(format!(
                "object {} is {} bytes, larger than the 64 MiB transfer limit",
                it.hash,
                data.len()
            )));
        }
        let base = it
            .base
            .filter(|b| server_has.contains(b) || w.contains(b.as_bytes()))
            .and_then(|b| store.get_local(&b).ok().map(|d| (b, d)));
        w.add(
            *it.hash.as_bytes(),
            kind_byte(it.kind),
            &data,
            base.as_ref().map(|(h, d)| (h.as_bytes(), d.as_slice())),
        )?;
    }
    let (_, stats) = w.finish()?;
    Ok(stats)
}

enum Attempt {
    Done(wire::UploadPackResponse),
    Failed(Status),
    /// The testing hook cut the stream.
    Interrupted,
}

/// Codes after which a resume may succeed.
fn resumable(s: &Status) -> bool {
    matches!(
        s.code(),
        Code::Unavailable
            | Code::Aborted
            | Code::Cancelled
            | Code::DeadlineExceeded
            | Code::Internal
            | Code::Unknown
            | Code::FailedPrecondition
    )
}

/// Stream `path[offset..len]` as one UploadPack call.
async fn send_from(
    client: &SyncClient,
    path: &Path,
    len: u64,
    upload_id: &str,
    tree: &str,
    offset: u64,
    interrupt_after: Option<u64>,
) -> Result<Attempt> {
    let (tx, rx) = mpsc::channel::<wire::PackChunk>(4);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (path, id, tree) = (path.to_path_buf(), upload_id.to_string(), tree.to_string());
    let chunk = if interrupt_after.is_some() {
        TEST_CHUNK
    } else {
        MAX_CHUNK
    };
    let producer = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut f = std::fs::File::open(&path)?;
        f.seek(SeekFrom::Start(offset))?;
        let mut off = offset;
        let mut first = true;
        let mut stop_tx = Some(stop_tx);
        // Always at least one chunk: a resume at the very end still needs
        // a message to name the upload.
        while first || off < len {
            let n = usize::try_from(len - off).unwrap_or(usize::MAX).min(chunk);
            let mut data = vec![0u8; n];
            f.read_exact(&mut data)?;
            let msg = wire::PackChunk {
                data,
                upload_id: id.clone(),
                offset: off,
                tree_id: first.then(|| tree_id(&tree)),
            };
            first = false;
            if tx.blocking_send(msg).is_err() {
                return Ok(()); // the RPC ended; its status explains why
            }
            off += n as u64;
            if interrupt_after.is_some_and(|limit| off >= limit) && off < len {
                // Give the server time to spool what it got, then cut the
                // connection (the RPC future is dropped, the stream reset).
                std::thread::sleep(Duration::from_millis(300));
                if let Some(stop) = stop_tx.take() {
                    let _ = stop.send(());
                }
                while !tx.is_closed() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                return Ok(());
            }
        }
        Ok(())
    });
    let mut inner = client.inner.clone();
    let outcome = tokio::select! {
        r = inner.upload_pack(ReceiverStream::new(rx)) => match r {
            Ok(resp) => Attempt::Done(resp.into_inner()),
            Err(s) => Attempt::Failed(s),
        },
        Ok(()) = stop_rx => Attempt::Interrupted,
    };
    producer
        .await
        .map_err(|e| SdkError::Other(format!("pack upload task failed: {e}")))??;
    Ok(outcome)
}

/// Upload `items` as a pack. `Ok(None)` when the server (or `WT_NO_PACK`)
/// rules packs out: the caller uploads whole objects instead. Bases must
/// be in `server_has` — objects of snapshots reachable from the server's
/// branch tips (objects merely stored there, e.g. by a rejected push, are
/// refused as bases) — or earlier entries.
pub(crate) async fn upload(
    client: &mut SyncClient,
    store: &ObjectStore,
    tree: &str,
    items: Vec<UploadItem>,
    server_has: HashSet<ObjectHash>,
) -> Result<Option<PackUpload>> {
    if client.no_packs || packs_disabled_by_env() || items.is_empty() {
        return Ok(None);
    }
    let kinds: HashMap<ObjectHash, ObjectKind> = items.iter().map(|i| (i.hash, i.kind)).collect();
    let path = std::env::temp_dir().join(format!("wt-pack-{}.pack", uuid::Uuid::new_v4().simple()));
    let _guard = TempPack(path.clone());
    let built = {
        let (store, path) = (store.clone(), path.clone());
        tokio::task::spawn_blocking(move || build_pack(&store, &items, &server_has, &path))
            .await
            .map_err(|e| SdkError::Other(format!("pack build task failed: {e}")))??
    };
    let len = built.pack_bytes;
    let upload_id = uuid::Uuid::new_v4().simple().to_string();

    let mut resumed = false;
    let resp = match send_from(
        client,
        &path,
        len,
        &upload_id,
        tree,
        0,
        test_interrupt_after(),
    )
    .await?
    {
        Attempt::Done(r) => r,
        Attempt::Failed(s) if s.code() == Code::Unimplemented => {
            tracing::debug!(server = %client.server(), "server has no pack RPCs; uploading whole objects");
            client.no_packs = true;
            return Ok(None);
        }
        Attempt::Failed(s) if !resumable(&s) => return Err(client.status(s)),
        first => {
            if let Attempt::Failed(s) = &first {
                tracing::info!(code = ?s.code(), message = s.message(), "pack upload interrupted; resuming");
            }
            // Resume once, from what the server kept.
            let offset = match client
                .inner
                .clone()
                .query_upload(wire::QueryUploadRequest {
                    upload_id: upload_id.clone(),
                })
                .await
            {
                Ok(r) => {
                    let r = r.into_inner();
                    if r.found && r.offset <= len {
                        r.offset
                    } else {
                        0
                    }
                }
                Err(s) => return Err(client.status(s)),
            };
            resumed = offset > 0;
            let mut tries = 0;
            loop {
                match send_from(client, &path, len, &upload_id, tree, offset, None).await? {
                    Attempt::Done(r) => break r,
                    Attempt::Failed(s) if s.code() == Code::Aborted && tries < BUSY_RETRIES => {
                        // The server has not noticed the cut stream yet.
                        tries += 1;
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Attempt::Failed(s) => return Err(client.status(s)),
                    Attempt::Interrupted => {
                        return Err(SdkError::Remote("pack upload interrupted twice".into()))
                    }
                }
            }
        }
    };
    // Entries the server could not use (typically a delta whose base it
    // does not let us use) go again as whole objects; the caller uploads
    // them with UploadObjects, which reports real corruption.
    let mut retry = Vec::new();
    for h in &resp.rejected {
        let hash = super::convert::hash_from_wire(Some(h))?;
        let kind = kinds.get(&hash).copied().ok_or_else(|| {
            SdkError::Remote(format!(
                "server rejected object {hash} that was not uploaded"
            ))
        })?;
        retry.push((hash, kind));
    }
    if !retry.is_empty() {
        tracing::debug!(
            count = retry.len(),
            "server rejected packed objects; resending them whole"
        );
    }
    Ok(Some(PackUpload {
        stats: TransferStats {
            pack: true,
            objects: built.entries as usize,
            deltas: built.deltas as usize,
            object_bytes: built.object_bytes,
            wire_bytes: len,
            resumed,
        },
        retry,
    }))
}

/// Result of [`upload`].
pub(crate) struct PackUpload {
    pub stats: TransferStats,
    /// Objects the server rejected: upload them whole.
    pub retry: Vec<(ObjectHash, ObjectKind)>,
}

// ---------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------

/// A blocking `Read` over the chunks the async side receives.
struct ChannelReader {
    rx: mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos == self.buf.len() {
            match self.rx.blocking_recv() {
                Some(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                None => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Download `want` (plus the chunks of wanted chunk lists) as a pack into
/// `store`, offering `have` as delta bases. Entries that cannot be used
/// are skipped (the caller re-checks completeness). `Ok(None)` when packs
/// are ruled out.
pub(crate) async fn download(
    client: &mut SyncClient,
    store: &ObjectStore,
    tree: &str,
    want: &[ObjectHash],
    have: &[ObjectHash],
) -> Result<Option<TransferStats>> {
    if client.no_packs || packs_disabled_by_env() || want.is_empty() {
        return Ok(None);
    }
    // One pack per batch keeps requests well under the server's hash and
    // message limits.
    let mut total = TransferStats::default();
    for batch in want.chunks(WANT_BATCH) {
        match download_batch(client, store, tree, batch, have).await? {
            Some(stats) => total.absorb(&stats),
            None => return Ok(None),
        }
    }
    Ok(Some(total))
}

/// Most `want` hashes per `DownloadPack`.
const WANT_BATCH: usize = 200_000;

async fn download_batch(
    client: &mut SyncClient,
    store: &ObjectStore,
    tree: &str,
    want: &[ObjectHash],
    have: &[ObjectHash],
) -> Result<Option<TransferStats>> {
    let req = wire::DownloadPackRequest {
        tree_id: Some(tree_id(tree)),
        want: want.iter().map(wire_hash).collect(),
        have: have.iter().map(wire_hash).collect(),
        allow_deltas: true,
    };
    let mut stream = match client.inner.clone().download_pack(req).await {
        Ok(r) => r.into_inner(),
        Err(s) if s.code() == Code::Unimplemented => {
            client.no_packs = true;
            return Ok(None);
        }
        Err(s) => return Err(client.status(s)),
    };
    let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
    let reader = {
        let store = store.clone();
        tokio::task::spawn_blocking(move || -> Result<TransferStats> {
            let mut pr = PackReader::new(ChannelReader {
                rx,
                buf: Vec::new(),
                pos: 0,
            })
            .map_err(pack_err)?;
            let mut stats = TransferStats {
                pack: true,
                ..TransferStats::default()
            };
            while let Some(obj) = pr
                .next_object(|h| store.get_local(&ObjectHash::from_bytes(*h)).ok())
                .map_err(pack_err)?
            {
                let hash = ObjectHash::from_bytes(obj.hash);
                match obj.data {
                    Ok(data) => {
                        store.put_hashed(&hash, &data)?;
                        stats.objects += 1;
                        stats.object_bytes += data.len() as u64;
                        stats.deltas += usize::from(obj.base.is_some());
                    }
                    Err(e) => tracing::warn!(%hash, error = %e, "skipping unusable pack entry"),
                }
            }
            stats.wire_bytes = pr.consumed();
            Ok(stats)
        })
    };
    let mut next = 0u64;
    let mut failure: Option<Status> = None;
    loop {
        match stream.message().await {
            Ok(Some(chunk)) => {
                if chunk.offset != next || chunk.data.len() > MAX_CHUNK {
                    failure = Some(Status::data_loss(format!(
                        "pack chunk at offset {} (expected {next})",
                        chunk.offset
                    )));
                    break;
                }
                next += chunk.data.len() as u64;
                if tx.send(chunk.data).await.is_err() {
                    break; // the reader failed; its error explains why
                }
            }
            Ok(None) => break,
            Err(s) => {
                failure = Some(s);
                break;
            }
        }
    }
    drop(tx);
    let result = reader
        .await
        .map_err(|e| SdkError::Other(format!("pack download task failed: {e}")))?;
    if let Some(s) = failure {
        if s.code() == Code::Unimplemented && next == 0 {
            client.no_packs = true;
            return Ok(None);
        }
        return Err(client.status(s));
    }
    result.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_switch() {
        // Only the parsing; the variable itself is process-global.
        for (v, off) in [
            ("1", true),
            ("true", true),
            ("YES", true),
            ("0", false),
            ("", false),
        ] {
            let parsed = matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
            assert_eq!(parsed, off, "{v}");
        }
    }

    #[test]
    fn stats_absorb() {
        let mut a = TransferStats {
            objects: 1,
            object_bytes: 10,
            wire_bytes: 10,
            ..TransferStats::default()
        };
        a.absorb(&TransferStats {
            pack: true,
            objects: 2,
            deltas: 1,
            object_bytes: 100,
            wire_bytes: 20,
            resumed: true,
        });
        assert_eq!(
            a,
            TransferStats {
                pack: true,
                objects: 3,
                deltas: 1,
                object_bytes: 110,
                wire_bytes: 30,
                resumed: true
            }
        );
    }

    #[test]
    fn resumable_codes() {
        assert!(resumable(&Status::unavailable("x")));
        assert!(resumable(&Status::failed_precondition("incomplete")));
        assert!(!resumable(&Status::permission_denied("x")));
        assert!(!resumable(&Status::resource_exhausted("quota")));
        assert!(!resumable(&Status::invalid_argument("corrupt")));
    }

    #[test]
    fn channel_reader_concatenates_chunks() {
        let (tx, rx) = mpsc::channel(4);
        tx.try_send(b"hello ".to_vec()).unwrap();
        tx.try_send(Vec::new()).unwrap();
        tx.try_send(b"world".to_vec()).unwrap();
        drop(tx);
        let mut r = ChannelReader {
            rx,
            buf: Vec::new(),
            pos: 0,
        };
        let mut s = String::new();
        r.read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello world");
    }
}
