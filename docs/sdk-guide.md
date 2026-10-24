# SDK guide (`worktree-sdk`)

`worktree-sdk` is the W0rkTree engine as a Rust library. The `wt` CLI, the `worktree-bg` daemon and
the `worktree-git` bridge are thin layers over it; your own tools can be too.

- [Setup](#setup)
- [Module map](#module-map)
- [Data model](#data-model)
- [Repository API](#repository-api): [lifecycle](#lifecycle), [trees](#trees), [status and snapshots](#status-and-snapshots), [history and revisions](#history-and-revisions), [branches](#branches), [merge, revert, restore](#merge-revert-restore), [diff](#diff), [tags](#tags), [reflog and ignore](#reflog-and-ignore), [configuration](#configuration), [plumbing](#plumbing)
- [Remote module](#remote-module)
- [IPC module](#ipc-module)
- [Pure algorithms](#pure-algorithms)
- [Errors](#errors)
- [Concurrency and testing](#concurrency-and-testing)

The public API is defined by the source in
[`crates/worktree-sdk/src`](../crates/worktree-sdk/src/lib.rs); signatures there are authoritative.

---

## Setup

The SDK is not published to crates.io; depend on it by path or Git:

```toml
[dependencies]
worktree-sdk = { path = "../worktree/crates/worktree-sdk" }
# or: worktree-sdk = { git = "https://github.com/seanfilimon/worktree" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }   # only for remote/ipc
```

Local operations are synchronous. The `remote` and `ipc` modules are `async` and run on Tokio.

## Module map

| Module | Contents |
|---|---|
| `repo` | `Repository` — everything that reads or writes `.wt/` |
| `store` | `ObjectStore`, `ObjectHash` (BLAKE3-256), `hash_bytes` |
| `model` | Data types (re-exported at the crate root) |
| `config` | `WorktreeConfig`, `wt_home()` |
| `diff` | Line diff, unified diff, diff stats, binary detection (no I/O) |
| `merge` | Three-way text merge, manifest merge, merge base (no I/O) |
| `scan` | Secret scanning, SPDX detection, FastCDC chunk boundaries (no I/O) |
| `remote` | Server client: login/register, ping, push, fetch, pull, clone, stage, list staged |
| `ipc` | Client for the `worktree-bg` daemon |
| `error` | `SdkError`, `Result<T>` |

Crate-root re-exports: `Repository`, `ObjectHash`, `ObjectStore`, `SdkError`, `Result`, and every
type in `model`.

## Data model

| Type | Fields (abridged) | Notes |
|---|---|---|
| `Snapshot` | `id`, `tree_id`, `branch`, `parents`, `manifest`, `author`, `message`, `created_at`, `auto_generated` | `short_id()`, `is_merge()`, `to_record()` / `from_record()` for the wire |
| `Manifest` | `entries: Vec<ManifestEntry>` | Sorted, unique; `from_entries`, `get`, `to_bytes`, `from_bytes`, `hash` |
| `ManifestEntry` | `path`, `hash`, `size`, `executable` | `path` is `/`-separated, relative to the tree root |
| `TreeInfo` | `name`, `id`, `path`, `current_branch`, `default_branch`, `remote_name` | `path` is `""` for the root tree |
| `BranchInfo` | `name`, `tip`, `is_current`, `created_at`, `archived`, `upstream_tip` | `upstream_tip` = `origin/<name>` |
| `TagInfo` | `name`, `target`, `message`, `tagger`, `created_at` | |
| `StatusReport` | `tree`, `branch`, `head`, `changes: Vec<FileChange>`, `merge` | `is_clean()` |
| `FileChange` | `path`, `kind: ChangeKind` | `Added` / `Modified` / `Deleted` |
| `MergeOutcome` | `UpToDate`, `FastForward { to }`, `Merged { snapshot }`, `Conflicts { conflicts }` | |
| `ConflictInfo` | `path`, `kind: ConflictKind` | `Content`, `ModifyDelete`, `AddAdd`, `Binary` |
| `MergeState` | `source_branch`, `source_tip`, `target_tip`, `base`, `conflicts`, `message` | Persisted during a conflicted merge |
| `FileDiff` | `path`, `kind`, `old_hash`, `new_hash`, `binary`, `patch`, `added`, `removed` | |
| `ReflogEntry` | `at`, `tree`, `branch`, `action`, `old`, `new`, `message` | |
| Options | `InitOptions`, `SnapshotOptions`, `SwitchOptions`, `MergeOptions`, `DiffOptions` | All implement `Default` |

All model types implement `serde::Serialize`/`Deserialize` (this is what `wt --json` prints).
`SnapshotId` is a `String` (UUID v4).

---

## Repository API

Most methods take a **tree name** as their first argument. Use `repo.root_tree()?.name` for the
root tree, or `repo.tree_for_path(&std::env::current_dir()?)?` to pick the tree the user is in
(that is what `wt` does).

Except for the first and last complete programs, the snippets below are function bodies: they assume
`repo: &Repository` (`&mut` for configuration), `tree: &str`, a return type of
`worktree_sdk::Result<()>`, and `use worktree_sdk::*;` plus `use std::path::Path;`. The plumbing
example additionally uses the `uuid` (feature `v4`) and `chrono` crates.

### Lifecycle

```rust
use std::path::Path;
use worktree_sdk::{InitOptions, Repository, Result};

fn main() -> Result<()> {
    // Create ./demo/.wt with root tree "demo" and default branch "main".
    let repo = Repository::init(
        Path::new("demo"),
        InitOptions { name: None, default_branch: Some("main".into()) },
    )?;
    println!("worktree at {}", repo.root().display());

    // Later: open from any directory inside the worktree (walks up to find .wt/).
    let repo = Repository::open(Path::new("demo/src"))?;
    println!("author: {}", repo.author());
    Ok(())
}
```

`init` fails with `AlreadyInitialized`; `open` fails with `NotAWorktree`.

### Trees

```rust
let web = repo.add_tree("web", Path::new("apps/web"))?;   // writes apps/web/.wt-tree/config.toml
for t in repo.trees()? {
    println!("{:<10} {:<12} {}", t.name, t.current_branch, t.path.display());
}
let here = repo.tree_for_path(Path::new("apps/web/src/main.ts"))?;
assert_eq!(here.name, web.name);
repo.set_tree_remote_name("web", "platform-web")?;        // name on the server
repo.remove_tree("web")?;                                  // files are kept
```

### Status and snapshots

```rust
let tree = repo.root_tree()?.name;
let status = repo.status(&tree)?;
for change in &status.changes {
    let mark = match change.kind {
        ChangeKind::Added => 'A',
        ChangeKind::Modified => 'M',
        ChangeKind::Deleted => 'D',
    };
    println!("{mark} {}", change.path);
}

match repo.snapshot(&tree, SnapshotOptions { message: "Add parser".into(), ..Default::default() }) {
    Ok(snap) => println!("snapshot {}", snap.short_id()),
    Err(SdkError::NoChanges) => println!("nothing to snapshot"),
    Err(SdkError::SecretsDetected(details)) => eprintln!("refused: {details}"),
    Err(e) => return Err(e),
}
```

`SnapshotOptions` fields: `message`, `author` (override), `auto_generated`, `allow_empty`,
`skip_secret_scan`. During a conflicted merge, `snapshot` succeeds only once all markers are gone
and then completes the merge with two parents.

### History and revisions

```rust
for s in repo.log(tree, None, Some(10))? {             // newest first, from HEAD
    println!("{} {} {}", s.short_id(), s.created_at.format("%F"), s.message);
}
let snap = repo.resolve(tree, "main~2")?;               // HEAD, HEAD~N, branch, origin/x, tag, id prefix
let manifest = repo.manifest_of(&snap)?;
for entry in &manifest.entries {
    println!("{:>8} {}", entry.size, entry.path);
}
let readme = repo.read_file_at(&snap, "README.md")?;
let same = repo.get_snapshot(&snap.id[..8])?;            // unique prefix, >= 4 chars
assert_eq!(same.id, snap.id);
```

### Branches

```rust
repo.create_branch(tree, "feature/login", Some("HEAD~1"))?;
repo.switch_branch(tree, "feature/login", SwitchOptions::default())?;   // DirtyWorkingTree if dirty
repo.switch_branch(tree, "spike", SwitchOptions { create: true, force: false })?;
for b in repo.branches(tree, false)? {
    println!("{}{} {:?}", if b.is_current { "* " } else { "  " }, b.name, b.tip);
}
repo.switch_branch(tree, "main", SwitchOptions::default())?;
repo.archive_branch(tree, "spike")?;     // soft delete; not the current or default branch
repo.restore_branch(tree, "spike")?;
```

### Merge, revert, restore

```rust
match repo.merge(tree, "feature/login", MergeOptions::default())? {
    MergeOutcome::UpToDate => println!("already up to date"),
    MergeOutcome::FastForward { to } => println!("fast-forwarded to {to}"),
    MergeOutcome::Merged { snapshot } => println!("merge snapshot {}", snapshot.short_id()),
    MergeOutcome::Conflicts { conflicts } => {
        for c in &conflicts {
            println!("conflict ({:?}): {}", c.kind, c.path);
        }
        // Let the user edit files, then:
        // repo.continue_merge(tree, Some("Merge feature/login"))?;   // MergeConflict if markers remain
        // or give up:
        repo.abort_merge(tree)?;
    }
}

let undo = repo.revert(tree, "HEAD~1")?;                 // new snapshot; history is not rewritten
let restored = repo.restore(tree, &["src/main.rs".into()], Some("HEAD~2"))?;
```

`merge` requires a clean working tree and returns `MergeInProgress` while another merge is pending.
`merge_state(tree)` returns the persisted `MergeState`, if any.

### Diff

```rust
let opts = DiffOptions { context: 3, name_only: false, paths: vec!["src/".into()] };
for d in repo.diff_working(tree, None, &opts)? {          // working dir vs HEAD
    println!("{:?} {} +{} -{}", d.kind, d.path, d.added, d.removed);
    if let Some(patch) = &d.patch {
        print!("{patch}");
    }
}
let changed = repo.diff_revs(tree, "main", "feature/login", &DiffOptions { name_only: true, ..Default::default() })?;
```

### Tags

```rust
repo.create_tag(tree, "v1.0.0", None, Some("First release"))?;   // None = HEAD
for t in repo.tags(tree)? {
    println!("{} -> {}", t.name, &t.target[..8]);
}
repo.delete_tag(tree, "v1.0.0")?;
```

### Reflog and ignore

```rust
for e in repo.reflog(tree, Some(20))? {                   // newest first
    println!("{} {:<14} {:?} -> {:?} {}", e.at, e.action, e.old, e.new, e.message);
}
repo.add_ignore_pattern("*.log")?;                        // appends to .wt/ignore
assert!(repo.is_ignored("build/debug.log")?);
println!("{:?}", repo.ignore_patterns()?);                // built-ins + .wt/ignore + .wtignore
```

### Configuration

```rust
let mut cfg = repo.config().clone();
cfg.set("user.name", "Ada Lovelace")?;                    // InvalidConfig for unknown keys / bad values
cfg.set("remote.origin.url", "http://localhost:7420")?;
cfg.set("remote.origin.tenant", "acme")?;
repo.set_config(cfg)?;                                    // persists .wt/config.toml
for (key, value) in repo.config().entries() {
    println!("{key} = {value}");
}
println!("{:?}", worktree_sdk::config::wt_home());        // $WT_HOME or ~/.wt
```

Keys are listed in [configuration](./configuration.md#keys).

### Plumbing

Lower-level operations used by the remote client and the Git bridge. They never touch the working
directory unless documented.

| Method | Purpose |
|---|---|
| `store()` | The `ObjectStore` (`put`, `put_with_hash`, `get`, `has`, `path_of`, `list`, `stats`) |
| `write_manifest` / `read_manifest` | Store / load a manifest object |
| `manifest_from_working(tree)` | Hash the working directory into a manifest (storing blobs) |
| `write_snapshot(&Snapshot)` | Persist a snapshot record (idempotent; moves no ref) |
| `has_snapshot(id)` | Existence check |
| `branch_tip` / `set_branch_tip` | Read / move a local branch ref |
| `remote_tip` / `set_remote_tip` | Read / move `refs/remotes/<remote>/<branch>` |
| `put_tag(tree, &TagInfo)` | Write a tag record directly |
| `ancestry(tip)` | All snapshots reachable from `tip`, parents before children |
| `is_ancestor(a, b)`, `merge_base(a, b)` | DAG queries |
| `checkout(tree, snapshot, force)` | Rewrite the working directory to a snapshot and refresh the index |
| `append_reflog(&ReflogEntry)` | Add a reflog line |
| `set_tree_id(tree, id)` | Adopt a server tree id (used by clone) |

Building a snapshot by hand (what an importer does):

```rust
let blob = repo.store().put(b"hello\n")?;
let manifest = Manifest::from_entries(vec![ManifestEntry {
    path: "hello.txt".into(), hash: blob, size: 6, executable: false,
}]);
let snap = Snapshot {
    id: uuid::Uuid::new_v4().to_string(),
    tree_id: repo.tree(tree)?.id,
    branch: "main".into(),
    parents: repo.branch_tip(tree, "main")?.into_iter().collect(),
    manifest: repo.write_manifest(&manifest)?,
    author: repo.author(),
    message: "imported".into(),
    created_at: chrono::Utc::now(),
    auto_generated: false,
};
repo.write_snapshot(&snap)?;
repo.set_branch_tip(tree, "main", Some(&snap.id))?;
```

---

## Remote module

`worktree_sdk::remote` talks to the server: gRPC `SyncService` over h2c plus the REST auth endpoints,
both at the remote's base URL. Credentials are stored per normalized server URL in
`$WT_HOME/credentials.toml`. Protocol details: [sync protocol](./protocol-spec.md).

| Function | Purpose |
|---|---|
| `register(server, username, email, password)` | Create an account; saves the credential; returns `LoginInfo` |
| `login(server, username, password)` | Log in; saves the credential |
| `load_credential` / `save_credential` / `remove_credential` | Manage `credentials.toml` |
| `ping(server)` | `PingInfo { server_version, username, tenant }` |
| `push(repo, tree, remote, branch)` | `PushReport` (snapshots/objects sent, `up_to_date`) |
| `fetch(repo, tree, remote)` | `FetchReport` (updated branches, counts) |
| `pull(repo, tree, remote, branch)` | `PullReport { fetch, branch, outcome: MergeOutcome }` |
| `clone(server, tenant, tree_name, dest)` | New `Repository` linked to the server tree |
| `stage(repo, tree, remote)` | Upload the working state as a staged snapshot → `StagedInfo` |
| `list_staged(repo, tree, remote, branch)` | Team-wide `Vec<StagedInfo>` |

```rust
use std::path::Path;
use worktree_sdk::{remote, MergeOutcome, Repository, Result, SdkError};

#[tokio::main]
async fn main() -> Result<()> {
    let server = "http://localhost:7420";
    let password = std::env::var("WT_PASSWORD").unwrap_or_default();
    let info = remote::login(server, "ada", &password).await?;
    println!("logged in as {} (tenant {})", info.username, info.tenant);

    let repo = Repository::open(Path::new("."))?;
    let tree = repo.root_tree()?.name;

    match remote::push(&repo, &tree, "origin", None).await {
        Ok(r) if r.up_to_date => println!("up to date"),
        Ok(r) => println!("pushed {} snapshots, {} objects", r.snapshots_sent, r.objects_sent),
        Err(SdkError::PushRejected(why)) => {
            eprintln!("rejected: {why}");
            let pulled = remote::pull(&repo, &tree, "origin", None).await?;
            if let MergeOutcome::Conflicts { conflicts } = pulled.outcome {
                eprintln!("{} conflicts to resolve", conflicts.len());
            }
        }
        Err(e) => return Err(e),
    }

    for s in remote::list_staged(&repo, &tree, "origin", None).await? {
        println!("{} on {}: {} files", s.author, s.branch, s.files_changed.len());
    }

    let copy = remote::clone(server, "ada", "demo", Path::new("demo-copy")).await?;
    println!("cloned into {}", copy.root().display());
    Ok(())
}
```

Typical errors: `RemoteNotConfigured`, `NotLoggedIn` (no credential or `UNAUTHENTICATED`),
`PermissionDenied`, `TreeNotFound`, `PushRejected`, `Remote` (connection and other server errors).

## IPC module

`worktree_sdk::ipc` is the client side of the [daemon protocol](./daemon.md#ipc-protocol).

```rust
use worktree_sdk::ipc::{self, IpcRequest, IpcResponse};
use worktree_sdk::SdkError;

#[tokio::main]
async fn main() -> worktree_sdk::Result<()> {
    match ipc::send(&IpcRequest::Status).await {
        Ok(IpcResponse::Status(st)) => {
            println!("daemon pid {} v{}", st.pid, st.version);
            for w in st.watched {
                println!("{} paused={} snapshots={}", w.root.display(), w.paused, w.snapshots_created);
            }
        }
        Ok(IpcResponse::Error { message }) => eprintln!("daemon error: {message}"),
        Ok(other) => println!("{other:?}"),
        Err(SdkError::DaemonNotRunning) => println!("daemon is not running"),
        Err(e) => return Err(e),
    }
    let cwd = std::env::current_dir()?;
    ipc::send(&IpcRequest::Watch { path: cwd.clone() }).await?;
    ipc::send(&IpcRequest::SnapshotNow { path: cwd }).await?;
    Ok(())
}
```

`ipc::read_daemon_info()` returns the parsed `$WT_HOME/bg.json` (`pid`, `port`, `version`) without
checking liveness.

## Pure algorithms

These modules do no I/O and are useful on their own:

```rust
use worktree_sdk::{diff, merge, scan};

let patch = diff::unified_diff("a.txt", "a.txt", "one\ntwo\n", "one\n2\n", 3);
let stat = diff::diff_stat("one\ntwo\n", "one\n2\n");          // DiffStat { added: 1, removed: 1 }
assert!(diff::is_binary(b"\x00\x01"));

let base = "a\nb\nc\n";
let ours = "A\nb\nc\n";   // changed line 1
let theirs = "a\nb\nC\n"; // changed line 3
match merge::merge_text(base, ours, theirs, &merge::MergeLabels::default()) {
    merge::TextMergeResult::Clean(text) => assert_eq!(text, "A\nb\nC\n"),
    merge::TextMergeResult::Conflicted { merged, conflicts } => { let _ = (merged, conflicts); }
}

for f in scan::scan_secrets("config.env", b"AWS_KEY=AKIAABCDEFGHIJKLMNOP\n") {
    println!("{}:{} {} {}", f.path, f.line, f.rule, f.excerpt);
}
let spdx = scan::detect_license(b"// SPDX-License-Identifier: MIT\n");  // Some("MIT")
let chunks = scan::chunk_boundaries(&vec![0u8; 8 << 20]);                // FastCDC (offset, len)
```

`merge::merge_manifests(base, ours, theirs)` returns a `PathResolution` per path, and
`merge::merge_base(a, b, &parents)` computes the best common ancestor of a snapshot DAG.

## Errors

Every fallible function returns `worktree_sdk::Result<T>` = `Result<T, SdkError>`. Match on
variants rather than messages. Frequently handled variants:

| Variant | When |
|---|---|
| `NotAWorktree`, `AlreadyInitialized` | `open` / `init` |
| `TreeNotFound`, `BranchNotFound`, `SnapshotNotFound`, `TagNotFound` | Unknown names / ids |
| `BranchExists`, `TreeExists`, `TagExists` | Name taken |
| `AmbiguousId`, `InvalidRevision` | `resolve` / `get_snapshot` |
| `NoChanges` | `snapshot` without changes and without `allow_empty` |
| `DirtyWorkingTree(n)` | `switch_branch` / `checkout` / `merge` with uncommitted changes |
| `MergeConflict`, `MergeInProgress`, `NoMergeInProgress` | Merge workflow |
| `SecretsDetected` | Pre-snapshot scan |
| `ObjectNotFound`, `CorruptObject` | Object store |
| `InvalidConfig`, `InvalidArgument` | Bad input |
| `RemoteNotConfigured`, `NotLoggedIn`, `PermissionDenied`, `PushRejected`, `Remote` | Remote operations |
| `DaemonNotRunning` | IPC |

The CLI maps `MergeConflict` and conflicted outcomes to exit code 3 and everything else to 1.

## Concurrency and testing

- Mutating operations on a tree take an exclusive file lock on `.wt/trees/<tree>/LOCK`, so several
  processes (for example `wt` and the daemon) can safely use the same worktree. A `Repository` value
  is a handle; reopen it to observe config changes made by other processes.
- Object writes are atomic and idempotent; concurrent writers of the same object are safe.
- **Tests must set `WT_HOME` to a temporary directory** so they never read or write the user's real
  credentials or daemon files. Use `tempfile::TempDir` for worktrees as well.
