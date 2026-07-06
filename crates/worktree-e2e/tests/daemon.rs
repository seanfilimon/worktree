//! WT-PHASE-3 acceptance: daemon lifecycle, IPC routing, auto-snapshots.

mod common;

use common::{wt, wt_ok};
use std::path::Path;
use std::time::{Duration, Instant};

/// Poll `f` every 250 ms until it returns true or `timeout` elapses.
fn wait_for(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if f() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn daemon_log(root: &Path) -> String {
    std::fs::read_to_string(root.join(".wt").join("cache").join("bgprocess.log"))
        .unwrap_or_else(|_| "<no daemon log>".into())
}

#[test]
fn daemon_lifecycle_and_auto_snapshot() {
    // Build both binaries up front so `wt server start` finds its sibling.
    common::wt_bin();
    common::bg_bin();

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    wt_ok(root, &["init", "."]);
    // Fast auto-snapshot for the test.
    wt_ok(
        root,
        &[
            "config",
            "set",
            "auto_snapshot.inactivity_timeout_secs",
            "1",
        ],
    );

    // Start the daemon through the real user path.
    let out = wt_ok(root, &["server", "start"]);
    assert!(out.contains("daemon"), "{out}");

    // Status reports running.
    let running = wait_for(Duration::from_secs(10), || {
        wt(root, &["server", "status"])
            .stdout
            .windows(b"running".len())
            .any(|w| w == b"running")
    });
    assert!(
        running,
        "daemon never reported running.\nlog:\n{}",
        daemon_log(root)
    );

    // CLI operations route through the daemon transparently.
    std::fs::write(root.join("manual.txt"), "manual change\n").unwrap();
    let out = wt_ok(root, &["snapshot", "-m", "via daemon"]);
    assert!(out.contains("Snapshot created"), "{out}");

    // A working-directory change triggers an auto-snapshot after the
    // inactivity window.
    std::fs::write(root.join("watched.txt"), "auto change\n").unwrap();
    let auto_appeared = wait_for(Duration::from_secs(30), || {
        wt(root, &["log", "-n", "10"])
            .stdout
            .windows(b"auto-snapshot".len())
            .any(|w| w == b"auto-snapshot")
    });
    assert!(
        auto_appeared,
        "auto-snapshot never appeared.\nlog:\n{}\nwt log:\n{}",
        daemon_log(root),
        wt_ok(root, &["log", "-n", "10"]),
    );

    // Stop the daemon; status flips to stopped.
    wt_ok(root, &["server", "stop"]);
    let stopped = wait_for(Duration::from_secs(10), || {
        wt(root, &["server", "status"])
            .stdout
            .windows(b"stopped".len())
            .any(|w| w == b"stopped")
    });
    assert!(stopped, "daemon never stopped.\nlog:\n{}", daemon_log(root));

    // Degraded mode still works without the daemon.
    let out = wt_ok(root, &["status"]);
    assert!(out.contains("Worktree Status"), "{out}");
    let out = wt_ok(root, &["log", "-n", "10"]);
    assert!(out.contains("via daemon"), "{out}");
}

#[test]
fn stop_without_daemon_is_friendly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    wt_ok(root, &["init", "."]);

    let out = wt_ok(root, &["server", "stop"]);
    assert!(out.contains("No daemon"), "{out}");

    let out = wt_ok(root, &["server", "status"]);
    assert!(out.contains("stopped"), "{out}");
}
