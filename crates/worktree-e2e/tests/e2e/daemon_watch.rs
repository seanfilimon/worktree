//! Daemon sync loop (ENGINEERING_PLAN §13.11): with `sync.auto_fetch` the
//! daemon watching alice's worktree follows `WatchBranches` and fetches
//! bob's push into `origin/main` by itself (local branch and files
//! untouched); `wt unstage` / `wt stage --discard` remove a staged snapshot.

use std::time::Duration;

use worktree_e2e::json::{self, str_field};
use worktree_e2e::{poll_until, require_e2e, Repo, TestEnv};

use crate::common::{add_member, publish_tree, staged_by, TREE};

/// Tip of `origin/main` in `repo` (None while unknown).
fn origin_main(repo: &Repo) -> Option<String> {
    let out = repo
        .wt()
        .args(["--json", "log", "origin/main", "-n", "1"])
        .run();
    if !out.success() {
        return None;
    }
    json::snapshots(&out.json())
        .first()
        .map(|s| str_field(s, "id"))
}

#[test]
fn daemon_watch() {
    require_e2e!();
    let env = TestEnv::with_server("daemon_watch");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# watched remote\n")]);
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);

    // Fetch on events only: the periodic fallback is an hour away, so a
    // timely update proves the WatchBranches path.
    for (k, v) in [
        ("sync.auto_fetch", "true"),
        ("sync.interval_secs", "3600"),
        ("sync.auto_stage", "false"),
        ("auto_snapshot.enabled", "false"),
    ] {
        a.wt().args(["config", "set", k, v]).ok();
    }
    let alice_head = a.head_id();
    assert_eq!(origin_main(&a), Some(alice_head.clone()));

    let _daemon = alice.daemon_start();
    a.wt().args(["daemon", "watch"]).arg(&a.dir).ok();
    // Wait until the sync loop has opened the WatchBranches stream.
    poll_until(
        "the daemon's sync loop to watch the remote",
        Duration::from_secs(30),
        Duration::from_millis(200),
        || {
            let s = alice.wt().args(["daemon", "status"]).json();
            if s.to_string().contains("\"sync_state\":\"watching\"") {
                Ok(())
            } else {
                Err(json::pretty(&s))
            }
        },
    );

    // --- bob pushes twice; alice's origin/main follows each time
    for n in 1..=2 {
        let tip = b.commit(
            &[("bob.txt", &format!("bob change {n}\n"))],
            &format!("bob: change {n}"),
        );
        b.push();
        poll_until(
            &format!("alice's origin/main to reach bob's push #{n}"),
            Duration::from_secs(30),
            Duration::from_millis(250),
            || match origin_main(&a) {
                Some(t) if t == tip => Ok(()),
                other => Err(format!("origin/main = {other:?}, want {tip}")),
            },
        );
    }
    assert_eq!(a.head_id(), alice_head, "local main is not moved by fetch");
    assert!(!a.exists("bob.txt"), "fetch does not touch files");
    a.assert_clean();
    let status = alice.wt().args(["daemon", "status"]).json();
    assert!(
        status.to_string().contains("last_fetch_at"),
        "daemon status reports the sync loop:\n{}",
        json::pretty(&status)
    );
    a.pull();
    assert_eq!(a.read("bob.txt"), "bob change 2\n");

    // --- unstage
    a.write("wip.txt", "work in progress\n");
    a.wt().arg("stage").ok();
    let team = b.wt().args(["status", "--team"]).json();
    assert_eq!(
        staged_by(&team, "alice").len(),
        1,
        "{}",
        json::pretty(&team)
    );
    a.wt().arg("unstage").ok();
    let team = b.wt().args(["status", "--team"]).json();
    assert!(
        staged_by(&team, "alice").is_empty(),
        "unstage removes the staged snapshot:\n{}",
        json::pretty(&team)
    );
    assert!(a.exists("wip.txt"), "unstage keeps the working files");
    // `wt stage --discard` is the same operation.
    a.wt().arg("stage").ok();
    assert_eq!(
        staged_by(&b.wt().args(["status", "--team"]).json(), "alice").len(),
        1
    );
    a.wt().args(["stage", "--discard"]).ok();
    assert!(staged_by(&b.wt().args(["status", "--team"]).json(), "alice").is_empty());
    // Nothing staged: unstage still succeeds (idempotent) or explains.
    let out = a.wt().arg("unstage").run();
    assert!(
        out.success() || out.mentions("staged"),
        "unstage with nothing staged\n{out}"
    );
}
