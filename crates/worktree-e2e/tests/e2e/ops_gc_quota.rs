//! Operations (ENGINEERING_PLAN §13.3): tenant quotas via `wt admin
//! limits` (a second tree and an extra branch are refused with
//! `quota_exceeded`), `wt admin usage`, garbage collection (`wt admin gc
//! --dry-run` reports unreachable objects of a deleted branch without
//! touching them; a real run only marks within the grace period) and
//! `/healthz` with its probes.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{require_e2e, Api, TestEnv};

use crate::common::{publish_tree, server_tip, TREE};

/// The GC report in `wt --json admin gc` output.
fn gc_report(v: &Value) -> Value {
    find_objects(v, &["scanned", "reachable", "unreachable"])
        .first()
        .map(|r| (*r).clone())
        .unwrap_or_else(|| panic!("no GCReport in\n{}", pretty(v)))
}

fn num(v: &Value, key: &str) -> u64 {
    v[key]
        .as_u64()
        .unwrap_or_else(|| panic!("expected number `{key}` in\n{}", pretty(v)))
}

#[test]
fn ops_gc_quota() {
    require_e2e!();
    let env = TestEnv::with_server("ops_gc_quota");

    // --- /healthz (anonymous) reports its probes
    let health = Api::new(&env.url()).get("/healthz");
    health.assert_status(200);
    let h = health.json();
    assert_eq!(h["status"], "ok", "{}", pretty(&h));
    assert_eq!(h["checks"]["meta"], "ok", "{}", pretty(&h));
    assert_eq!(h["checks"]["objects"], "ok", "{}", pretty(&h));
    assert!(h["version"].as_str().is_some_and(|v| !v.is_empty()));
    assert!(h["uptime_seconds"].as_u64().is_some(), "{}", pretty(&h));

    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# ops\n")]);
    let alice_api = alice.api();
    let admin = env.admin_user();
    admin.login();

    // --- quotas: only the server admin sets them
    alice
        .wt()
        .args(["admin", "limits", "alice", "--max-trees", "1"])
        .fails();
    let limits = admin
        .wt()
        .args([
            "admin",
            "limits",
            "alice",
            "--max-trees",
            "1",
            "--max-branches",
            "2",
        ])
        .json();
    assert!(
        limits.to_string().contains("max_trees"),
        "limits:\n{}",
        pretty(&limits)
    );
    let usage = admin.wt().args(["admin", "usage", "alice"]).json();
    assert_eq!(usage["trees"].as_u64(), Some(1), "{}", pretty(&usage));
    assert_eq!(
        usage["effective"]["max_trees"].as_u64(),
        Some(1),
        "{}",
        pretty(&usage)
    );

    // A second tree: refused over REST, by `wt trees create` and by pushing
    // a new worktree.
    alice_api
        .post(
            "/api/v1/tenants/alice/trees",
            json!({"name": "second", "description": "over quota"}),
        )
        .assert_error(409, "quota_exceeded");
    let out = alice.wt().args(["trees", "create", "alice/third"]).fails();
    assert!(out.mentions("quota"), "trees create refusal\n{out}");
    let lib = alice.init("lib");
    lib.commit(&[("lib.txt", "lib\n")], "lib");
    lib.add_remote("origin", &env.url(), "alice");
    let out = lib.wt().arg("push").fails();
    assert!(out.mentions("quota"), "push of a new tree refused\n{out}");
    let trees = alice_api.get("/api/v1/tenants/alice/trees").ok_json();
    assert_eq!(
        find_objects(&trees, &["name"])
            .iter()
            .filter(|t| t["name"] == "lib" || t["name"] == "second" || t["name"] == "third")
            .count(),
        0,
        "no tree beyond the quota:\n{}",
        pretty(&trees)
    );

    // Branches per tree: main + one more.
    a.wt().args(["switch", "-c", "doomed"]).ok();
    a.commit(
        &[("doomed.txt", "only on a branch that will be deleted\n")],
        "doomed work",
    );
    a.wt().args(["push", "--branch", "doomed"]).ok();
    a.wt().args(["switch", "-c", "one-too-many"]).ok();
    let out = a.wt().args(["push", "--branch", "one-too-many"]).fails();
    assert!(out.mentions("quota"), "third branch refused\n{out}");
    assert_eq!(server_tip(&alice_api, "alice", TREE, "one-too-many"), None);
    a.wt().args(["switch", "main"]).ok();

    // Clearing the limits restores the server defaults (unlimited).
    admin
        .wt()
        .args(["admin", "limits", "alice", "--clear"])
        .ok();
    alice.wt().args(["trees", "create", "alice/third"]).ok();

    // --- garbage collection
    // Deleting `doomed` leaves its snapshot, manifest and blob unreachable.
    alice
        .wt()
        .args(["trees", "branch-delete", &format!("alice/{TREE}"), "doomed"])
        .confirmed();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "doomed"), None);
    alice.wt().args(["admin", "gc", "--dry-run"]).fails(); // admins only
    let dry = gc_report(&admin.wt().args(["admin", "gc", "--dry-run"]).json());
    assert_eq!(dry["dry_run"], Value::Bool(true), "{}", pretty(&dry));
    assert!(num(&dry, "scanned") > 0, "{}", pretty(&dry));
    assert!(num(&dry, "reachable") > 0, "{}", pretty(&dry));
    assert!(
        num(&dry, "unreachable") >= 1,
        "the deleted branch's objects are unreachable:\n{}",
        pretty(&dry)
    );
    assert_eq!(num(&dry, "deleted"), 0, "{}", pretty(&dry));
    let text = admin.wt().args(["admin", "gc", "--dry-run"]).ok();
    assert!(text.mentions("unreachable"), "human gc report\n{text}");

    // A real run inside the (default 30 day) grace period only marks.
    let real = gc_report(&admin.wt().args(["admin", "gc"]).json());
    assert_eq!(real["dry_run"], Value::Bool(false), "{}", pretty(&real));
    assert!(num(&real, "marked") >= 1, "{}", pretty(&real));
    assert_eq!(num(&real, "deleted"), 0, "{}", pretty(&real));
    let status = admin.wt().args(["admin", "gc-status"]).json();
    assert!(
        !find_objects(&status, &["scanned", "marked"]).is_empty(),
        "gc-status lists the runs:\n{}",
        pretty(&status)
    );

    // Everything reachable survives: a fresh clone of main works.
    let bob = env.user("bob");
    bob.register();
    alice_api
        .put(
            "/api/v1/tenants/alice/members/bob",
            json!({"role": "viewer"}),
        )
        .assert_ok();
    let b = bob.clone_tree("alice", TREE, TREE);
    assert_eq!(b.head_id(), a.head_id());
    assert!(!b.exists("doomed.txt"));
    let h = Api::new(&env.url()).get("/healthz").json();
    assert_eq!(h["status"], "ok", "{}", pretty(&h));
}
