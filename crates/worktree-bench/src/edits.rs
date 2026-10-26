//! pack-delta (docs/ENGINEERING_PLAN.md §16.1): the `edits` workload —
//! push a large tree, then push small edits to a few of its files, and
//! report what went over the wire (packs with deltas, or whole objects
//! with `WT_NO_PACK=1`).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::JoinSet;
use worktree_sdk::remote::{self, TransferStats};
use worktree_sdk::Repository;

use crate::stats::Samples;
use crate::workloads::{file_path, init_repo, round_files, snapshot, Op, Plan, REMOTE};

/// Files edited per round when `--changed` is 0.
const DEFAULT_CHANGED: u32 = 10;

const WORDS: &[&str] = &[
    "let", "fn", "match", "return", "self", "value", "error", "result", "config", "server",
    "client", "branch", "snapshot", "manifest", "object", "hash", "tree", "tenant", "push",
    "fetch", "delta", "pack", "chunk", "offset", "stream", "buffer", "index", "length", "write",
    "read", "if", "else", "for", "while", "in", "Some", "None", "Ok", "Err", "=", "+", "{", "}",
    "(", ")", ";", "::", "->", "&mut", "u64", "usize", "String", "Vec", "HashMap",
];

/// `size` bytes of source-like text unique to file `i` of `worker`: random
/// words, so it compresses like code (about 3-4x) rather than like the
/// repetitive lines of the push workload.
fn text_content(i: u32, worker: &str, size: u64) -> Vec<u8> {
    let mut s = (u64::from(i) + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ worker
            .bytes()
            .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
    s |= 1;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let mut out = String::with_capacity(size as usize + 128);
    while (out.len() as u64) < size {
        let indent = (next() % 4) as usize * 4;
        out.extend(std::iter::repeat_n(' ', indent));
        for w in 0..(4 + next() % 9) {
            if w > 0 {
                out.push(' ');
            }
            out.push_str(WORDS[(next() % WORDS.len() as u64) as usize]);
            if next() % 5 == 0 {
                out.push_str(&(next() % 10_000).to_string());
            }
        }
        out.push('\n');
    }
    let mut bytes = out.into_bytes();
    bytes.truncate(size as usize);
    if let Some(last) = bytes.last_mut() {
        *last = b'\n';
    }
    bytes
}

/// Insert one line in the middle of the file (a small edit).
fn edit_file(root: &Path, i: u32, round: u32) -> std::io::Result<u64> {
    let p = root.join(file_path(i));
    let mut data = std::fs::read(&p)?;
    let mid = data.len() / 2;
    let at = data[mid..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(data.len(), |n| mid + n + 1);
    let line = format!("edited in round {round}\n");
    data.splice(at..at, line.bytes());
    std::fs::write(&p, &data)?;
    Ok(data.len() as u64)
}

#[derive(Default)]
struct Totals {
    initial: TransferStats,
    edits: TransferStats,
    /// Content bytes of the edited files (what whole objects carry).
    edited_bytes: u64,
    pushes: u32,
}

/// N workers, each with its own tree of `--files` × `--file-size`: round 0
/// writes and pushes everything ("edits: initial push"), later rounds
/// insert one line into `--changed` files (default 10) and push ("edits:
/// push").
pub async fn edits(plan: &Plan) -> Vec<Op> {
    let started = Instant::now();
    let mut set = JoinSet::new();
    for w in 0..plan.concurrency {
        let plan = plan.clone();
        set.spawn(async move { worker(plan, w).await });
    }
    let (mut initial, mut pushes, mut snaps) =
        (Samples::default(), Samples::default(), Samples::default());
    let mut totals = Totals::default();
    while let Some(res) = set.join_next().await {
        match res {
            Ok((i, p, s, t)) => {
                initial.merge(i);
                pushes.merge(p);
                snaps.merge(s);
                totals.initial.absorb(&t.initial);
                totals.edits.absorb(&t.edits);
                totals.edited_bytes += t.edited_bytes;
                totals.pushes += t.pushes;
            }
            Err(e) => pushes.err(format!("worker panicked: {e}")),
        }
    }
    let wall = started.elapsed();
    let describe = |t: &TransferStats, whole: u64| {
        if t.pack {
            format!(
                "pack: {} objects ({} deltas), {} object bytes, {} pack bytes sent",
                t.objects, t.deltas, t.object_bytes, t.wire_bytes
            )
        } else {
            format!(
                "whole objects: {} objects, about {whole} content bytes sent (before gRPC compression)",
                t.objects
            )
        }
    };
    eprintln!(
        "edits: initial push {}",
        describe(
            &totals.initial,
            plan.files as u64 * plan.file_size * plan.concurrency as u64
        )
    );
    eprintln!(
        "edits: {} incremental push(es) {}",
        totals.pushes,
        describe(&totals.edits, totals.edited_bytes)
    );
    vec![
        Op::new("edits: initial push", initial, wall),
        Op::new("edits: push", pushes, wall),
        Op::new("edits: snapshot (local)", snaps, wall),
    ]
}

async fn worker(plan: Plan, w: u32) -> (Samples, Samples, Samples, Totals) {
    let (mut initial, mut pushes, mut snaps) =
        (Samples::default(), Samples::default(), Samples::default());
    let mut totals = Totals::default();
    let name = format!("{}-edits-{w}", plan.run_id);
    let root = plan.work.join(&name);
    let repo = {
        let (p, n) = (plan.clone(), name.clone());
        match tokio::task::spawn_blocking(move || init_repo(&p, &n)).await {
            Ok(Ok(r)) => Arc::new(r),
            Ok(Err(e)) => {
                initial.err(e);
                return (initial, pushes, snaps, totals);
            }
            Err(e) => {
                initial.err(e.to_string());
                return (initial, pushes, snaps, totals);
            }
        }
    };
    let changed = if plan.changed == 0 {
        DEFAULT_CHANGED.min(plan.files)
    } else {
        plan.changed
    };
    for round in 0..plan.iterations.max(2) {
        let (p, r, n, rt) = (plan.clone(), repo.clone(), name.clone(), root.clone());
        let local = tokio::task::spawn_blocking(move || -> Result<(u64, Duration), String> {
            let mut bytes = 0;
            if round == 0 {
                for i in 0..p.files {
                    let path = rt.join(file_path(i));
                    if let Some(dir) = path.parent() {
                        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    }
                    let data = text_content(i, &n, p.file_size);
                    bytes += data.len() as u64;
                    std::fs::write(&path, data).map_err(|e| e.to_string())?;
                }
            } else {
                for i in round_files(round, p.files, changed) {
                    bytes += edit_file(&rt, i, round).map_err(|e| e.to_string())?;
                }
            }
            let t = Instant::now();
            snapshot(&r, &n, round)?;
            Ok((bytes, t.elapsed()))
        })
        .await;
        let bytes = match local {
            Ok(Ok((bytes, d))) => {
                snaps.ok(d, 0);
                bytes
            }
            Ok(Err(e)) => {
                snaps.err(e);
                continue;
            }
            Err(e) => {
                snaps.err(e.to_string());
                continue;
            }
        };
        let t = Instant::now();
        let res = push(&repo, &name).await;
        let samples = if round == 0 {
            &mut initial
        } else {
            &mut pushes
        };
        match res {
            Ok(stats) => {
                let sent = if stats.pack { stats.wire_bytes } else { bytes };
                samples.ok(t.elapsed(), sent);
                if round == 0 {
                    totals.initial.absorb(&stats);
                } else {
                    totals.edits.absorb(&stats);
                    totals.edited_bytes += bytes;
                    totals.pushes += 1;
                }
            }
            Err(e) => samples.err(e),
        }
    }
    (initial, pushes, snaps, totals)
}

async fn push(repo: &Repository, name: &str) -> Result<TransferStats, String> {
    remote::push_with_stats(repo, name, REMOTE, None)
        .await
        .map(|(_, stats)| stats)
        .map_err(|e| e.to_string())
}
