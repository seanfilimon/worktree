//! The push, clone and search workloads, driven in-process through the
//! worktree-sdk async API.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::JoinSet;
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote::{self, RestClient};
use worktree_sdk::{InitOptions, Repository, SnapshotOptions};

use crate::stats::Samples;

/// Everything a workload needs to know about the target and the shape of
/// the trees.
#[derive(Debug, Clone)]
pub struct Plan {
    pub server: String,
    pub tenant: String,
    pub token: String,
    /// Scratch directory for worktrees and clones.
    pub work: PathBuf,
    /// Unique prefix of this run's tree names.
    pub run_id: String,
    pub concurrency: u32,
    pub iterations: u32,
    pub files: u32,
    pub file_size: u64,
    pub changed: u32,
    pub query: String,
}

/// Samples of one operation kind and the wall time they were taken in
/// (the throughput denominator).
pub struct Op {
    pub name: String,
    pub samples: Samples,
    pub wall: Duration,
}

impl Op {
    pub(crate) fn new(name: &str, samples: Samples, wall: Duration) -> Op {
        Op {
            name: name.to_string(),
            samples,
            wall,
        }
    }
}

/// Remote name the bench's worktrees push to.
pub(crate) const REMOTE: &str = "origin";

/// Content of file `i` for `round` of `worker`: `size` bytes of text lines;
/// every tenth file mentions `needle` on its first line.
pub fn file_content(i: u32, round: u32, worker: &str, size: u64, needle: &str) -> Vec<u8> {
    let size = size as usize;
    let mut out = String::with_capacity(size + 128);
    if i.is_multiple_of(10) {
        out.push_str(&format!("// {needle} marker of file {i}\n"));
    }
    let mut n = 0u64;
    while out.len() < size {
        out.push_str(&format!(
            "line {n} of file {i} round {round} worker {worker}: the quick brown fox jumps over the lazy dog\n"
        ));
        n += 1;
    }
    let mut bytes = out.into_bytes();
    bytes.truncate(size);
    if let Some(last) = bytes.last_mut() {
        *last = b'\n';
    }
    bytes
}

/// Relative path of file `i` (100 files per directory).
pub fn file_path(i: u32) -> String {
    format!("src/d{:03}/file{:05}.txt", i / 100, i)
}

/// Which files a push round rewrites: all of them in round 0 (and when
/// `changed` is 0 or at least `files`), else `changed` files rotating
/// through the tree.
pub fn round_files(round: u32, files: u32, changed: u32) -> Vec<u32> {
    if round == 0 || changed == 0 || changed >= files {
        return (0..files).collect();
    }
    let start = ((round - 1) as u64 * changed as u64 % files as u64) as u32;
    (0..changed).map(|k| (start + k) % files).collect()
}

fn write_files(
    root: &Path,
    which: &[u32],
    round: u32,
    worker: &str,
    plan: &Plan,
) -> std::io::Result<u64> {
    let mut bytes = 0;
    for &i in which {
        let p = root.join(file_path(i));
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let data = file_content(i, round, worker, plan.file_size, &plan.query);
        bytes += data.len() as u64;
        std::fs::write(p, data)?;
    }
    Ok(bytes)
}

/// Create a worktree named `name` under the work dir with origin set.
pub(crate) fn init_repo(plan: &Plan, name: &str) -> Result<Repository, String> {
    let dir = plan.work.join(name);
    let mut repo = Repository::init(
        &dir,
        InitOptions {
            name: Some(name.to_string()),
            default_branch: None,
        },
    )
    .map_err(|e| format!("init {}: {e}", dir.display()))?;
    let mut config = repo.config().clone();
    config.user.name = "Bench".into();
    config.user.email = "bench@example.com".into();
    config.remote.insert(
        REMOTE.into(),
        RemoteSection {
            url: plan.server.clone(),
            tenant: plan.tenant.clone(),
        },
    );
    repo.set_config(config)
        .map_err(|e| format!("configure {name}: {e}"))?;
    Ok(repo)
}

pub(crate) fn snapshot(repo: &Repository, tree: &str, round: u32) -> Result<(), String> {
    repo.snapshot(
        tree,
        SnapshotOptions {
            message: format!("bench round {round}"),
            author: Some("bench".into()),
            skip_secret_scan: true,
            skip_hooks: true,
            ..Default::default()
        },
    )
    .map(|_| ())
    .map_err(|e| format!("snapshot: {e}"))
}

/// Seed tree `name` with `plan.files` files and push it (not measured).
pub async fn seed(plan: &Plan, name: &str) -> Result<(), String> {
    let (p, n) = (plan.clone(), name.to_string());
    let repo = tokio::task::spawn_blocking(move || -> Result<Repository, String> {
        let repo = init_repo(&p, &n)?;
        let all: Vec<u32> = (0..p.files).collect();
        write_files(&p.work.join(&n), &all, 0, "seed", &p).map_err(|e| e.to_string())?;
        snapshot(&repo, &n, 0)?;
        Ok(repo)
    })
    .await
    .map_err(|e| e.to_string())??;
    remote::push(&repo, name, REMOTE, None)
        .await
        .map(|_| ())
        .map_err(|e| format!("seed push: {e}"))
}

/// N workers, each with its own tree: round 0 writes every file, later
/// rounds rewrite `changed` files; each round is snapshotted locally and
/// pushed. Reports "push" (bytes = file content written that round) and
/// "snapshot (local)".
pub async fn push(plan: &Plan) -> Vec<Op> {
    let started = Instant::now();
    let mut set = JoinSet::new();
    for w in 0..plan.concurrency {
        let plan = plan.clone();
        set.spawn(async move { push_worker(plan, w).await });
    }
    let (mut pushes, mut snaps) = (Samples::default(), Samples::default());
    while let Some(res) = set.join_next().await {
        match res {
            Ok((p, s)) => {
                pushes.merge(p);
                snaps.merge(s);
            }
            Err(e) => pushes.err(format!("worker panicked: {e}")),
        }
    }
    let wall = started.elapsed();
    vec![
        Op::new("push", pushes, wall),
        Op::new("snapshot (local)", snaps, wall),
    ]
}

async fn push_worker(plan: Plan, w: u32) -> (Samples, Samples) {
    let (mut pushes, mut snaps) = (Samples::default(), Samples::default());
    let name = format!("{}-push-{w}", plan.run_id);
    let repo = {
        let (p, n) = (plan.clone(), name.clone());
        match tokio::task::spawn_blocking(move || init_repo(&p, &n)).await {
            Ok(Ok(r)) => Arc::new(r),
            Ok(Err(e)) => {
                pushes.err(e);
                return (pushes, snaps);
            }
            Err(e) => {
                pushes.err(e.to_string());
                return (pushes, snaps);
            }
        }
    };
    let root = plan.work.join(&name);
    for round in 0..plan.iterations {
        let which = round_files(round, plan.files, plan.changed);
        let (p, r, n, rt) = (plan.clone(), repo.clone(), name.clone(), root.clone());
        let local = tokio::task::spawn_blocking(move || -> Result<(u64, Duration), String> {
            let bytes = write_files(&rt, &which, round, &n, &p).map_err(|e| e.to_string())?;
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
        match remote::push(&repo, &name, REMOTE, None).await {
            Ok(_) => pushes.ok(t.elapsed(), bytes),
            Err(e) => pushes.err(e.to_string()),
        }
    }
    (pushes, snaps)
}

/// N workers each cloning the seed tree `iterations` times into fresh
/// directories (removed after each clone, outside the timing).
pub async fn clone(plan: &Plan, seed: &str) -> Vec<Op> {
    let started = Instant::now();
    let mut set = JoinSet::new();
    for w in 0..plan.concurrency {
        let (plan, seed) = (plan.clone(), seed.to_string());
        set.spawn(async move {
            let mut s = Samples::default();
            let bytes = plan.files as u64 * plan.file_size;
            for i in 0..plan.iterations {
                let dest = plan.work.join(format!("clone-{w}-{i}"));
                let t = Instant::now();
                match remote::clone(&plan.server, &plan.tenant, &seed, &dest).await {
                    Ok(_) => s.ok(t.elapsed(), bytes),
                    Err(e) => s.err(e.to_string()),
                }
                let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(dest)).await;
            }
            s
        });
    }
    let mut all = Samples::default();
    while let Some(res) = set.join_next().await {
        match res {
            Ok(s) => all.merge(s),
            Err(e) => all.err(format!("worker panicked: {e}")),
        }
    }
    vec![Op::new("clone", all, started.elapsed())]
}

/// One code search over the tenant's trees; the number of hits.
async fn search_once(client: &RestClient, plan: &Plan) -> Result<usize, String> {
    let v: serde_json::Value = client
        .get_query(
            "/api/v1/search/code",
            &[("q", plan.query.clone()), ("tenant", plan.tenant.clone())],
        )
        .await
        .map_err(|e| e.to_string())?;
    let hits = v
        .get("hits")
        .and_then(|h| h.as_array())
        .map(|h| h.len())
        .unwrap_or(0);
    if hits == 0 {
        return Err(format!("no hits for {:?}", plan.query));
    }
    Ok(hits)
}

/// A first (cold: builds the code index) search, then N workers issuing
/// `iterations` searches each over the tenant's trees.
pub async fn search(plan: &Plan) -> Vec<Op> {
    let mut cold = Samples::default();
    let t = Instant::now();
    let client = match RestClient::new(&plan.server, Some(plan.token.clone())) {
        Ok(c) => c,
        Err(e) => {
            cold.err(e.to_string());
            return vec![Op::new("search (cold)", cold, t.elapsed())];
        }
    };
    match search_once(&client, plan).await {
        Ok(_) => cold.ok(t.elapsed(), 0),
        Err(e) => cold.err(e),
    }
    let cold_wall = t.elapsed();
    let warm_start = Instant::now();
    let mut set = JoinSet::new();
    for _ in 0..plan.concurrency {
        let (plan, client) = (plan.clone(), client.clone());
        set.spawn(async move {
            let mut s = Samples::default();
            for _ in 0..plan.iterations {
                let t = Instant::now();
                match search_once(&client, &plan).await {
                    Ok(_) => s.ok(t.elapsed(), 0),
                    Err(e) => s.err(e),
                }
            }
            s
        });
    }
    let mut warm = Samples::default();
    while let Some(res) = set.join_next().await {
        match res {
            Ok(s) => warm.merge(s),
            Err(e) => warm.err(format!("worker panicked: {e}")),
        }
    }
    vec![
        Op::new("search (cold)", cold, cold_wall),
        Op::new("search", warm, warm_start.elapsed()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_has_exact_size_and_needle() {
        for size in [1u64, 17, 4096, 10_000] {
            let c = file_content(3, 0, "w", size, "needle");
            assert_eq!(c.len() as u64, size);
            assert_eq!(*c.last().unwrap(), b'\n');
        }
        let c = file_content(20, 1, "w", 4096, "benchneedle");
        assert!(String::from_utf8(c).unwrap().starts_with("// benchneedle"));
        // Rounds and workers change the content (new blobs per push).
        assert_ne!(
            file_content(1, 0, "a", 512, "n"),
            file_content(1, 1, "a", 512, "n")
        );
        assert_ne!(
            file_content(1, 0, "a", 512, "n"),
            file_content(1, 0, "b", 512, "n")
        );
    }

    #[test]
    fn rounds_rotate_changed_files() {
        assert_eq!(round_files(0, 5, 2), vec![0, 1, 2, 3, 4]);
        assert_eq!(round_files(1, 5, 2), vec![0, 1]);
        assert_eq!(round_files(2, 5, 2), vec![2, 3]);
        assert_eq!(round_files(3, 5, 2), vec![4, 0]);
        assert_eq!(round_files(4, 5, 0), vec![0, 1, 2, 3, 4]);
        assert_eq!(round_files(4, 5, 9), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn paths_fan_out() {
        assert_eq!(file_path(7), "src/d000/file00007.txt");
        assert_eq!(file_path(1234), "src/d012/file01234.txt");
    }
}
