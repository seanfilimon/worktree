//! Shared helpers for e2e tests: build workspace binaries once, run them
//! with isolated environments.

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

/// Run `wt` with the given args in `cwd`, isolated from the host
/// environment (storage and IPC endpoints point into the temp dir).
pub fn wt(cwd: &Path, args: &[&str]) -> Output {
    Command::new(wt_bin())
        .args(args)
        .current_dir(cwd)
        .env("WT_STORAGE_DIR", cwd.join(".wt-e2e-storage"))
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
