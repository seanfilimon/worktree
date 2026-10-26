//! One-step sync and the offline queue (ENGINEERING_PLAN §15.5): two clones
//! `wt sync` with the merge strategy (merge snapshot pushed), the rebase
//! strategy (linear history), conflicts (exit 3, resolve, `wt sync
//! --continue`); then the server is stopped: `wt push --queue` and `wt sync`
//! queue the push, the server is restarted on the same port and data dir,
//! and `wt sync flush` and the daemon's auto-flush deliver the queued
//! pushes; finally the daemon's `sync.auto_sync_secs` integrates a
//! teammate's push by itself.

use std::time::Duration;

use serde_json::Value;
use worktree_e2e::json::{self, parents, pretty, str_field};
use worktree_e2e::{poll_until, require_e2e, Repo, TestEnv, User};

use crate::common::{add_member, publish_tree, server_tip, TREE};

/// `wt --json sync <args>` (must succeed).
fn sync_json(repo: &Repo, args: &[&str]) -> Value {
    repo.wt().arg("sync").args(args).json()
}

fn field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no string {key:?} in\n{}", pretty(v)))
}

/// Queued pushes of `repo`'s tree (`wt --json sync queue`).
fn queue(repo: &Repo) -> Vec<Value> {
    let v = repo.wt().args(["sync", "queue"]).json();
    v.as_array()
        .and_then(|trees| trees.first())
        .and_then(|t| t.get("entries"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| panic!("unexpected queue output\n{}", pretty(&v)))
}

/// Server tip of `main`, asked as `user` with a fresh session.
fn tip_on_server(user: &User) -> Option<String> {
    server_tip(&user.api(), "alice", TREE, "main")
}

#[test]
fn one_step_sync() {
    require_e2e!();
    let mut env = TestEnv::with_server("one_step_sync");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("README.md", "# one-step sync\n"),
            ("notes.txt", "one\ntwo\nthree\n"),
        ],
    );
    let bob = env.user("bob");
    bob.register();
    add_member(&alice.api(), "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);

    // --- merge (default strategy): both sides changed different files
    b.commit(&[("bob.txt", "bob 1\n")], "bob 1");
    let r = sync_json(&b, &[]);
    assert_eq!(field(&r, "status"), "synced", "{}", pretty(&r));
    assert!(r["push"].is_object(), "{}", pretty(&r));
    a.commit(&[("alice.txt", "alice 1\n")], "alice 1");
    let r = sync_json(&a, &[]);
    assert_eq!(field(&r, "strategy"), "merge");
    assert_eq!(field(&r, "integration"), "merged", "{}", pretty(&r));
    assert_eq!(field(&r, "status"), "synced");
    assert_eq!(a.read("bob.txt"), "bob 1\n");
    let head = a.head();
    assert_eq!(parents(&head).len(), 2, "merge snapshot\n{}", pretty(&head));
    assert_eq!(tip_on_server(&alice), Some(str_field(&head, "id")));
    a.assert_clean();
    let out = a.wt().arg("sync").ok();
    assert!(out.mentions("Everything up to date"), "{out}");

    // bob fast-forwards; nothing to push.
    let r = sync_json(&b, &[]);
    assert_eq!(field(&r, "integration"), "fast_forward", "{}", pretty(&r));
    assert!(r["push"].is_null());
    assert_eq!(b.read("alice.txt"), "alice 1\n");

    // --- rebase strategy (config key known to `wt config`)
    b.wt()
        .args(["config", "set", "sync.strategy", "rebase"])
        .ok();
    let got = b.wt().args(["config", "get", "sync.strategy"]).ok();
    assert!(got.stdout.contains("rebase"), "{got}");
    assert!(b
        .wt()
        .args(["config", "set", "sync.strategy", "squash"])
        .fails()
        .mentions("merge or rebase"));
    let alice2 = a.commit(&[("alice.txt", "alice 2\n")], "alice 2");
    sync_json(&a, &[]);
    b.commit(&[("bob.txt", "bob 2\n")], "bob 2");
    let r = sync_json(&b, &[]);
    assert_eq!(field(&r, "strategy"), "rebase");
    assert_eq!(field(&r, "integration"), "rebased", "{}", pretty(&r));
    assert_eq!(r["replayed"], 1);
    let head = b.head();
    assert_eq!(
        parents(&head),
        vec![alice2.clone()],
        "linear on top of alice 2"
    );
    assert_eq!(json::str_field(&head, "message"), "bob 2");
    assert_eq!(tip_on_server(&bob), Some(str_field(&head, "id")));

    // --- conflicts: exit 3, resolve, --continue pushes
    sync_json(&a, &[]);
    a.commit(&[("notes.txt", "one\nTWO (alice)\nthree\n")], "alice notes");
    sync_json(&a, &[]);
    let before = b.commit(&[("notes.txt", "one\nTWO (bob)\nthree\n")], "bob notes");
    let out = b.wt().args(["sync", "--merge"]).code(3);
    assert!(
        out.mentions("CONFLICT") && out.mentions("notes.txt"),
        "{out}"
    );
    assert!(out.mentions("wt sync --continue"), "{out}");
    assert!(b.read("notes.txt").contains("<<<<<<<"));
    // A plain sync is refused until the conflict is settled.
    assert!(b.wt().arg("sync").fails().mentions("--continue"));
    b.write("notes.txt", "one\nTWO (both)\nthree\n");
    let r = sync_json(&b, &["--continue"]);
    assert_eq!(field(&r, "status"), "synced", "{}", pretty(&r));
    assert_eq!(field(&r, "old_tip"), before);
    let merged = b.head_id();
    assert_eq!(tip_on_server(&bob), Some(merged.clone()));
    let r = sync_json(&a, &[]);
    assert_eq!(field(&r, "integration"), "fast_forward");
    assert_eq!(a.read("notes.txt"), "one\nTWO (both)\nthree\n");

    // --- server stopped: push --queue and sync queue the push
    env.server_mut().stop();
    let offline1 = a.commit(&[("offline.txt", "written offline\n")], "offline 1");
    let out = a.wt().arg("push").fails();
    assert!(
        out.mentions("--queue"),
        "plain push hints at --queue\n{out}"
    );
    let out = a.wt().args(["push", "--queue"]).ok();
    assert!(out.mentions("queued"), "{out}");
    let q = queue(&a);
    assert_eq!(q.len(), 1, "{q:?}");
    assert_eq!(field(&q[0], "tip"), offline1);
    let out = a.wt().args(["sync", "--status"]).ok();
    assert!(out.mentions("main -> origin/main"), "{out}");
    let out = a.wt().args(["sync", "flush"]).code(1);
    assert!(out.mentions("still queued"), "{out}");
    assert_eq!(
        queue(&a)[0]["attempts"].as_u64(),
        Some(1),
        "attempt recorded"
    );
    // `wt sync` while offline queues too (same entry, newer tip).
    let offline2 = a.commit(&[("offline.txt", "more offline work\n")], "offline 2");
    let r = sync_json(&a, &[]);
    assert_eq!(field(&r, "status"), "queued", "{}", pretty(&r));
    let q = queue(&a);
    assert_eq!(q.len(), 1);
    assert_eq!(field(&q[0], "tip"), offline2);

    // --- restart on the same port and data dir; flush delivers
    env.server_mut().restart(&[]);
    let out = a.wt().args(["sync", "flush"]).ok();
    assert!(out.mentions("Delivered"), "{out}");
    assert_eq!(tip_on_server(&alice), Some(offline2.clone()));
    assert!(queue(&a).is_empty());
    assert!(a
        .wt()
        .args(["sync", "queue"])
        .ok()
        .mentions("No queued pushes"));

    // --- daemon auto-flush
    for (k, v) in [
        ("auto_snapshot.enabled", "false"),
        ("sync.auto_stage", "false"),
        ("sync.auto_fetch", "false"),
    ] {
        a.wt().args(["config", "set", k, v]).ok();
    }
    let _daemon = alice.daemon_start();
    a.wt().args(["daemon", "watch"]).arg(&a.dir).ok();
    env.server_mut().stop();
    let offline3 = a.commit(&[("daemon.txt", "delivered by the daemon\n")], "offline 3");
    let r = sync_json(&a, &[]);
    assert_eq!(field(&r, "status"), "queued", "{}", pretty(&r));
    // The daemon notices the queue and tries while the server is down.
    poll_until(
        "the daemon to attempt the queued push",
        Duration::from_secs(60),
        Duration::from_millis(250),
        || {
            let q = queue(&a);
            match q.first().and_then(|e| e["attempts"].as_u64()) {
                Some(n) if n >= 1 => Ok(()),
                _ => Err(format!("{q:?}")),
            }
        },
    );
    env.server_mut().restart(&[]);
    poll_until(
        "the daemon to deliver the queued push",
        Duration::from_secs(90),
        Duration::from_millis(500),
        || {
            let q = queue(&a);
            let tip = tip_on_server(&alice);
            if q.is_empty() && tip.as_deref() == Some(offline3.as_str()) {
                Ok(())
            } else {
                Err(format!("queue {q:?}, server tip {tip:?}"))
            }
        },
    );

    // --- daemon auto-sync of a clean tree
    a.wt()
        .args(["config", "set", "sync.auto_sync_secs", "2"])
        .ok();
    let r = sync_json(&b, &[]);
    assert_eq!(field(&r, "integration"), "fast_forward", "{}", pretty(&r));
    let bob_tip = b.commit(&[("bob.txt", "bob 3\n")], "bob 3");
    sync_json(&b, &[]);
    poll_until(
        "alice's daemon to auto-sync bob's push",
        Duration::from_secs(90),
        Duration::from_millis(500),
        || {
            let head = a.head_id();
            if head == bob_tip {
                Ok(())
            } else {
                Err(format!("alice head {head}, want {bob_tip}"))
            }
        },
    );
    assert_eq!(a.read("bob.txt"), "bob 3\n");
    a.assert_clean();
}
