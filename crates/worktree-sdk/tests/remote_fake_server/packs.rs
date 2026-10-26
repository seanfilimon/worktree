//! pack-delta (ENGINEERING_PLAN §16.1): the fake server's UploadPack,
//! QueryUpload and DownloadPack, enabled with [`FakeServer::start_with_packs`]
//! (the default fake answers `Unimplemented`, like a pre-v0.8 server).

use std::collections::{HashMap, HashSet};

use prost::Message;
use tonic::{Request, Response, Status, Streaming};
use worktree_protocol::pack::{PackError, PackReader, PackWriter, WriterOptions, MAX_CHUNK};
use worktree_protocol::proto::sync as wire;

use super::{lock, tree_of, FakeServer, Svc};

impl FakeServer {
    /// A fake that serves the pack RPCs.
    pub async fn start_with_packs() -> Self {
        let server = Self::start().await;
        server.state().packs = true;
        server
    }
}

fn unimplemented(what: &str) -> Status {
    Status::unimplemented(format!("{what} is not supported by the fake server"))
}

pub(super) async fn upload_pack(
    svc: &Svc,
    req: Request<Streaming<wire::PackChunk>>,
) -> Result<Response<wire::UploadPackResponse>, Status> {
    let caller = svc.require(&req)?;
    if !lock(&svc.state).packs {
        return Err(unimplemented("UploadPack"));
    }
    lock(&svc.state).pack_upload_calls += 1;
    let mut stream = req.into_inner();
    let mut id: Option<String> = None;
    while let Some(chunk) = stream.message().await? {
        let mut st = lock(&svc.state);
        if chunk.data.len() > MAX_CHUNK {
            return Err(Status::invalid_argument("chunk too large"));
        }
        let first = id.is_none();
        let id = id.get_or_insert_with(|| chunk.upload_id.clone()).clone();
        if id.is_empty() {
            return Err(Status::invalid_argument("upload_id is required"));
        }
        let entry = st
            .pack_spool
            .entry(id.clone())
            .or_insert_with(|| (caller.username.clone(), Vec::new()));
        if entry.0 != caller.username {
            return Err(Status::permission_denied("upload of another account"));
        }
        if first {
            if chunk.offset as usize > entry.1.len() {
                return Err(Status::failed_precondition("offset beyond received bytes"));
            }
            entry.1.truncate(chunk.offset as usize);
            if chunk.offset > 0 {
                st.pack_resumes += 1;
            }
        }
        let entry = st.pack_spool.get_mut(&id).expect("spool entry");
        if chunk.offset as usize != entry.1.len() {
            return Err(Status::invalid_argument("chunk offset gap"));
        }
        entry.1.extend_from_slice(&chunk.data);
        let received = entry.1.len();
        if st.fail_upload_after.is_some_and(|n| received >= n) {
            st.fail_upload_after = None;
            return Err(Status::unavailable("fake server lost the connection"));
        }
    }
    let id = id.ok_or_else(|| Status::invalid_argument("empty pack upload"))?;
    let mut st = lock(&svc.state);
    let data = st
        .pack_spool
        .get(&id)
        .map(|e| e.1.clone())
        .unwrap_or_default();
    let mut fresh: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
    let mut resp = wire::UploadPackResponse {
        bytes_received: data.len() as u64,
        ..Default::default()
    };
    let parsed = (|| -> Result<usize, PackError> {
        let mut pr = PackReader::new(data.as_slice())?;
        let mut deltas = 0;
        while let Some(obj) = pr.next_object(|h| {
            fresh
                .get(h.as_slice())
                .or_else(|| st.objects.get(h.as_slice()))
                .cloned()
        })? {
            match obj.data {
                Ok(d) => {
                    deltas += usize::from(obj.base.is_some());
                    if st.objects.contains_key(obj.hash.as_slice())
                        || fresh.contains_key(obj.hash.as_slice())
                    {
                        resp.objects_already_present += 1;
                    } else {
                        resp.objects_stored += 1;
                    }
                    fresh.insert(obj.hash.to_vec(), d);
                }
                Err(_) => resp.rejected.push(super::hash(&obj.hash)),
            }
        }
        Ok(deltas)
    })();
    match parsed {
        Ok(deltas) => {
            st.last_upload_deltas = deltas;
            st.objects.extend(fresh);
            st.pack_spool.remove(&id);
            Ok(Response::new(resp))
        }
        Err(PackError::Truncated) => Err(Status::failed_precondition("pack upload incomplete")),
        Err(e) => {
            st.pack_spool.remove(&id);
            Err(Status::invalid_argument(e.to_string()))
        }
    }
}

pub(super) async fn query_upload(
    svc: &Svc,
    req: Request<wire::QueryUploadRequest>,
) -> Result<Response<wire::QueryUploadResponse>, Status> {
    let caller = svc.require(&req)?;
    let st = lock(&svc.state);
    if !st.packs {
        return Err(unimplemented("QueryUpload"));
    }
    let id = req.into_inner().upload_id;
    Ok(Response::new(match st.pack_spool.get(&id) {
        Some((owner, data)) if *owner == caller.username => wire::QueryUploadResponse {
            found: true,
            offset: data.len() as u64,
        },
        _ => wire::QueryUploadResponse::default(),
    }))
}

pub(super) type DownloadPackStream =
    tokio_stream::Iter<std::vec::IntoIter<Result<wire::PackChunk, Status>>>;

const KIND_BLOB: u8 = 1;
const KIND_MANIFEST: u8 = 2;
const KIND_CHUNK_LIST: u8 = 3;

pub(super) async fn download_pack(
    svc: &Svc,
    req: Request<wire::DownloadPackRequest>,
) -> Result<Response<DownloadPackStream>, Status> {
    svc.caller(&req)?;
    let mut st = lock(&svc.state);
    if !st.packs {
        return Err(unimplemented("DownloadPack"));
    }
    st.pack_download_calls += 1;
    let r = req.into_inner();
    let tree = tree_of(&r.tree_id);
    st.tree(&tree)?;
    let withheld = st.withheld(&tree);

    // Object kinds, from the manifests of the tree's snapshots.
    let mut kinds: HashMap<Vec<u8>, u8> = HashMap::new();
    for rec in st.snapshots.values() {
        if rec.tree_id.as_ref().map(|t| t.value.as_str()) != Some(tree.as_str()) {
            continue;
        }
        let mh = rec
            .manifest_hash
            .clone()
            .map(|h| h.value)
            .unwrap_or_default();
        let Ok(m) = st.manifest(&mh) else { continue };
        kinds.insert(mh, KIND_MANIFEST);
        for e in &m.entries {
            let h = e.blob_hash.clone().map(|h| h.value).unwrap_or_default();
            kinds.insert(
                h.clone(),
                if e.chunked {
                    KIND_CHUNK_LIST
                } else {
                    KIND_BLOB
                },
            );
            for c in st.entry_closure(e).into_iter().skip(1) {
                kinds.entry(c).or_insert(KIND_BLOB);
            }
        }
    }
    let have: HashSet<Vec<u8>> = r.have.iter().map(|h| h.value.clone()).collect();
    let chunks_of = |h: &[u8]| -> Vec<Vec<u8>> {
        st.objects
            .get(h)
            .and_then(|d| wire::ChunkList::decode(d.as_slice()).ok())
            .map(|l| {
                l.chunks
                    .into_iter()
                    .filter_map(|c| c.hash.map(|h| h.value))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut sendable: HashMap<Vec<u8>, u8> = HashMap::new();
    let mut order: Vec<Vec<u8>> = Vec::new();
    let mut add = |h: Vec<u8>, k: u8, order: &mut Vec<Vec<u8>>| {
        if have.contains(&h) || withheld.contains(&h) || !st.objects.contains_key(&h) {
            return;
        }
        if sendable.insert(h.clone(), k).is_none() {
            order.push(h);
        }
    };
    for w in &r.want {
        let Some(&k) = kinds.get(&w.value) else {
            continue;
        };
        if k == KIND_CHUNK_LIST {
            for c in chunks_of(&w.value) {
                add(c, KIND_BLOB, &mut order);
            }
        }
        add(w.value.clone(), k, &mut order);
    }

    // Base candidates: same path in the previous manifest.
    let mut path_base: HashMap<String, Vec<u8>> = HashMap::new();
    let mut prev_manifest: Option<Vec<u8>> = None;
    for h in r.have.iter().take(16) {
        if kinds.get(&h.value) != Some(&KIND_MANIFEST) {
            continue;
        }
        let Ok(m) = st.manifest(&h.value) else {
            continue;
        };
        prev_manifest.get_or_insert_with(|| h.value.clone());
        for e in m.entries {
            path_base
                .entry(e.path)
                .or_insert_with(|| e.blob_hash.map(|h| h.value).unwrap_or_default());
        }
    }
    let mut plan: Vec<(Vec<u8>, u8, Option<Vec<u8>>)> = Vec::new();
    let mut planned: HashSet<Vec<u8>> = HashSet::new();
    let mut push = |h: Vec<u8>, base: Option<Vec<u8>>, plan: &mut Vec<_>| {
        if let Some(&k) = sendable.get(&h) {
            if planned.insert(h.clone()) {
                plan.push((h, k, if r.allow_deltas { base } else { None }));
            }
        }
    };
    for mh in &order {
        if sendable.get(mh) != Some(&KIND_MANIFEST) {
            continue;
        }
        let Ok(m) = st.manifest(mh) else { continue };
        for e in &m.entries {
            let eh = e.blob_hash.clone().map(|h| h.value).unwrap_or_default();
            let base = path_base.get(&e.path).cloned();
            if e.chunked {
                let base_chunks = base.as_deref().map(chunks_of).unwrap_or_default();
                for (i, c) in chunks_of(&eh).into_iter().enumerate() {
                    push(c, base_chunks.get(i).cloned(), &mut plan);
                }
            }
            push(eh, base, &mut plan);
        }
        push(mh.clone(), prev_manifest.clone(), &mut plan);
        for e in &m.entries {
            path_base.insert(
                e.path.clone(),
                e.blob_hash.clone().map(|h| h.value).unwrap_or_default(),
            );
        }
        prev_manifest = Some(mh.clone());
    }
    for h in &order {
        push(h.clone(), None, &mut plan);
    }

    let mut w = PackWriter::new(Vec::new(), WriterOptions::default())
        .map_err(|e| Status::internal(e.to_string()))?;
    let mut sent: HashSet<Vec<u8>> = HashSet::new();
    let mut deltas = 0;
    for (h, k, base) in plan {
        let data = st.objects.get(&h).cloned().unwrap_or_default();
        let base = base
            .filter(|b| *b != h && (sent.contains(b) || have.contains(b)))
            .and_then(|b| st.objects.get(&b).cloned().map(|d| (b, d)));
        let bh = base.as_ref().map(|(b, _)| hash32(b));
        let used = w
            .add(
                hash32(&h),
                k,
                &data,
                bh.as_ref().zip(base.as_ref().map(|(_, d)| d.as_slice())),
            )
            .map_err(|e| Status::internal(e.to_string()))?;
        deltas += usize::from(used);
        sent.insert(h);
    }
    let (pack, _) = w.finish().map_err(|e| Status::internal(e.to_string()))?;
    st.last_download_deltas = deltas;
    let items: Vec<_> = pack
        .chunks(MAX_CHUNK)
        .enumerate()
        .map(|(i, c)| {
            Ok(wire::PackChunk {
                data: c.to_vec(),
                offset: (i * MAX_CHUNK) as u64,
                ..Default::default()
            })
        })
        .collect();
    Ok(Response::new(tokio_stream::iter(items)))
}

fn hash32(h: &[u8]) -> [u8; 32] {
    h.try_into().unwrap_or([0; 32])
}
