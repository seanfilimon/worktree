//! Shared helpers for e2e tests: build workspace binaries once, run them
//! with isolated environments.
//!
//! Each integration-test binary compiles this module independently, so any
//! helper unused by one binary would trip `-D warnings` — hence the
//! file-level allow.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// Build (once) and return the path to the `wt` CLI binary.
pub fn wt_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        escargot::CargoBuild::new()
            .package("worktree-cli")
            .bin("wt")
            .run()
            .expect("failed to build wt binary")
            .path()
            .to_path_buf()
    })
}

/// Build (once) and return the path to the `worktree-bg` daemon binary.
pub fn bg_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        escargot::CargoBuild::new()
            .package("worktree-bg")
            .bin("worktree-bg")
            .run()
            .expect("failed to build worktree-bg binary")
            .path()
            .to_path_buf()
    })
}

/// Process-wide storage base for all e2e runs.
///
/// Lives **outside** every test worktree (stores must never be inside the
/// working directory, or the engine would scan them) and outside the real
/// platform data dir. Stores are isolated per worktree hash underneath it.
pub fn storage_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().expect("storage tempdir");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir); // keep alive for the whole test process
        path
    })
}

/// Run `wt` with the given args in `cwd`, isolated from the host
/// environment (storage and IPC endpoints point into temp dirs).
pub fn wt(cwd: &Path, args: &[&str]) -> Output {
    Command::new(wt_bin())
        .args(args)
        .current_dir(cwd)
        .env("WT_STORAGE_DIR", storage_dir())
        .env("WT_AUTHOR", "e2e-tester")
        .output()
        .expect("failed to run wt")
}

/// Run `wt` and require success; returns stdout.
pub fn wt_ok(cwd: &Path, args: &[&str]) -> String {
    let out = wt(cwd, args);
    assert!(
        out.status.success(),
        "wt {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}
