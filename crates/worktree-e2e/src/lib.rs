//! End-to-end test harness for W0rkTree.
//!
//! The integration tests in `tests/e2e/` drive the real binaries:
//!
//! * `wt` (crate `worktree-cli`) — located via `WT_BIN`, else next to the
//!   test executable / in `target/{debug,release}`.
//! * `worktree-bg` (crate `worktree-bg`) — `WT_BG_BIN`, same search.
//! * the Go server (`services/server-go`) — `WT_SERVER_BIN`, else built once
//!   per test run with `go build` into the cargo profile directory.
//!
//! Every test is gated on `WT_E2E=1` (see [`require_e2e!`]) so that
//! `cargo test --workspace` stays green on machines (or checkouts) where the
//! components are not built or not yet implemented.
//!
//! Each scenario creates its own [`TestEnv`] (temp dir, optional server on a
//! free port, simulated users with their own `WT_HOME`), so scenarios run in
//! parallel safely. A failing scenario keeps its temp dir (command logs in
//! `<tmp>/logs`, server logs in `<tmp>/server`); `WT_E2E_KEEP=1` keeps all
//! of them, `WT_E2E_KEEP=0` none.

pub mod api;
pub mod bins;
pub mod cmd;
pub mod env;
pub mod fsutil;
pub mod git;
pub mod json;
pub mod server;
pub mod totp;
pub mod wait;

#[cfg(test)]
mod selftest;

pub use api::{Api, Resp, SseEvent, SseStream};
pub use cmd::{Cmd, CmdOutput, Running};
pub use env::{DaemonGuard, Repo, TestEnv, User};
pub use git::Git;
pub use server::Server;
pub use totp::Totp;
pub use wait::poll_until;

/// Whether the end-to-end suite is enabled (`WT_E2E=1`).
pub fn e2e_enabled() -> bool {
    std::env::var("WT_E2E")
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

/// Return early from a test unless `WT_E2E=1`.
#[macro_export]
macro_rules! require_e2e {
    () => {
        if !$crate::e2e_enabled() {
            eprintln!("skipping e2e (set WT_E2E=1)");
            return;
        }
    };
}

/// A random lowercase hex string of `len` characters (std-only, no `rand`).
pub fn random_hex(len: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut out = String::with_capacity(len + 16);
    while out.len() < len {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
        );
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out.truncate(len);
    out
}

/// Best-effort forced kill of a process id (used for leaked daemons).
pub fn kill_pid(pid: u32) {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("taskkill");
        c.args(["/F", "/T", "/PID", &pid.to_string()]);
        c
    } else {
        let mut c = std::process::Command::new("kill");
        c.args(["-9", &pid.to_string()]);
        c
    };
    let _ = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
