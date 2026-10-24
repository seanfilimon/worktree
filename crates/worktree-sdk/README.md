# worktree-sdk

The local W0rkTree engine as a Rust library. Everything that reads or writes a `.wt/` directory lives
here; the `wt` CLI ([`worktree-cli`](../worktree-cli/README.md)), the background daemon
([`worktree-bg`](../worktree-bg/README.md)) and the Git bridge
([`worktree-git`](../worktree-git/Implementation.md)) are thin layers over it.

```rust
use std::path::Path;
use worktree_sdk::{Repository, Result, SnapshotOptions};

fn main() -> Result<()> {
    let repo = Repository::open(Path::new("."))?;
    let tree = repo.tree_for_path(&std::env::current_dir()?)?.name;
    let status = repo.status(&tree)?;
    if !status.is_clean() {
        let snap = repo.snapshot(&tree, SnapshotOptions { message: "checkpoint".into(), ..Default::default() })?;
        println!("snapshot {}", snap.short_id());
    }
    Ok(())
}
```

## Modules

| Module | Contents |
|---|---|
| `repo` | `Repository`: init/open, trees, status, snapshots, log, revisions, branches, merge, revert, restore, diff, tags, reflog, ignore, plumbing |
| `store` | `ObjectStore` (`.wt/objects/<hh>/<62 hex>`), `ObjectHash` (BLAKE3-256) |
| `model` | `Snapshot`, `Manifest`, `TreeInfo`, `BranchInfo`, `TagInfo`, `StatusReport`, `MergeOutcome`, options… (re-exported at the crate root) |
| `config` | `WorktreeConfig` (`.wt/config.toml`), `wt_home()` |
| `diff`, `merge` | Pure line diff and three-way merge (diff3) algorithms |
| `scan` | Secret scanning, SPDX detection, FastCDC chunk boundaries |
| `remote` | Async client for the W0rkTree server: login, register, ping, push, fetch, pull, clone, stage, list staged |
| `ipc` | Async client for the `worktree-bg` daemon |
| `error` | `SdkError`, `Result<T>` |

## Documentation

- [SDK guide](../../docs/sdk-guide.md) — API walkthrough with examples.
- [Storage format](../../docs/storage-format.md) — the `.wt/` layout this crate reads and writes.
- [Sync protocol](../../docs/protocol-spec.md) — what `remote` sends over the wire.
- [Implementation notes](./Implementation.md) — internals and invariants.

## Development

```bash
cargo test -p worktree-sdk
cargo clippy -p worktree-sdk --all-targets -- -D warnings
```

Tests must set `WT_HOME` to a temporary directory. Tests that need a running server are marked
`#[ignore = "integration: ..."]`; see [CONTRIBUTING.md](../../CONTRIBUTING.md#testing).
