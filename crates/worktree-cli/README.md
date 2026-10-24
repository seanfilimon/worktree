# worktree-cli (`wt`)

The W0rkTree command-line client. It parses arguments, calls
[`worktree-sdk`](../worktree-sdk/README.md) (and [`worktree-git`](../worktree-git/Implementation.md)
for `wt git`), and renders the result as text or JSON. It contains no version-control logic of its own.

```bash
cargo build -p worktree-cli --release      # target/release/wt (wt.exe on Windows)
wt --help
wt init demo && cd demo && wt snapshot -m "first" --allow-empty
```

## Commands

| Area | Commands |
|---|---|
| Local history | `init`, `status`, `snapshot`, `log`, `show`, `diff`, `reflog` |
| Branches | `branch`, `switch`, `merge`, `revert`, `restore`, `tag` |
| Structure | `tree`, `ignore`, `config` |
| Server | `remote`, `register`, `login`, `logout`, `whoami`, `push`, `fetch`, `pull`, `clone`, `stage` |
| Interop | `archive`, `git import`, `git export` |
| Daemon | `daemon` with `start`, `stop`, `status`, `watch`, `unwatch`, `pause`, `resume` |

Global flags: `--json`, `-C <path>`, `--tree <name>`, `--no-color`.
Exit codes: `0` success, `1` error, `2` usage error, `3` merge conflicts.

Full reference: [docs/cli-reference.md](../../docs/cli-reference.md).
Implementation notes: [Implementation.md](./Implementation.md).

## Development

```bash
cargo test -p worktree-cli
cargo clippy -p worktree-cli --all-targets -- -D warnings
```

Tests run `wt` against temporary worktrees with `WT_HOME` pointing to a temporary directory.
End-to-end scenarios against a real server are in `crates/worktree-e2e` (`WT_E2E=1`).
