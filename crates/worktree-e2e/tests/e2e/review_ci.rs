//! Reviews and CI gates (ENGINEERING_PLAN §13.4): `wt protect set --require-ci
//! --check ci/build --approvals 1`, a failing `wt ci report` blocks
//! `wt mr checks` / `wt mr merge`; success + approval unblocks it; a new
//! push to the source makes approvals stale (and needs fresh CI); a change
//! request blocks until the reviewer approves; MR comments round-trip.

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, Repo, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

const CHECK: &str = "ci/build";

/// `wt --json mr checks 1`.
#[track_caller]
fn checks(repo: &Repo) -> Value {
    repo.wt().args(["mr", "checks", "1"]).json()
}

fn blocker_kinds(c: &Value) -> Vec<String> {
    c["blockers"]
        .as_array()
        .map(|bs| {
            bs.iter()
                .filter_map(|b| b["kind"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[track_caller]
fn assert_blocked(c: &Value, kind: &str) {
    assert_eq!(
        c["mergeable"],
        Value::Bool(false),
        "not mergeable:\n{}",
        pretty(c)
    );
    assert!(
        blocker_kinds(c).iter().any(|k| k == kind),
        "expected a {kind:?} blocker:\n{}",
        pretty(c)
    );
}

#[track_caller]
fn ci_report(repo: &Repo, snapshot: &str, state: &str) {
    repo.wt()
        .args([
            "ci",
            "report",
            "--snapshot",
            snapshot,
            "--context",
            CHECK,
            "--state",
            state,
            "--description",
            &format!("build {state}"),
            "--url",
            "https://ci.example.com/run/1",
        ])
        .ok();
}

#[test]
fn review_ci() {
    require_e2e!();
    let env = TestEnv::with_server("review_ci");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# reviewed\n")]);
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "maintainer");
    let b = bob.clone_tree("alice", TREE, TREE);

    // --- protection: one approval and a passing ci/build
    a.wt()
        .args([
            "protect",
            "set",
            "main",
            "--no-direct-push",
            "--approvals",
            "1",
            "--require-ci",
            "--check",
            CHECK,
        ])
        .ok();
    let list = a.wt().args(["protect", "list"]).json();
    let rule = find_objects(&list, &["pattern"])
        .into_iter()
        .find(|p| p["pattern"] == "main")
        .unwrap_or_else(|| panic!("protection for main:\n{}", pretty(&list)))
        .clone();
    assert_eq!(
        rule["required_approvals"].as_u64(),
        Some(1),
        "{}",
        pretty(&rule)
    );
    assert!(
        rule.to_string().contains(CHECK),
        "the required check is stored:\n{}",
        pretty(&rule)
    );

    // --- feature branch + MR
    a.wt().args(["switch", "-c", "feature"]).ok();
    let tip1 = a.commit(&[("feature.txt", "feature v1\n")], "feature v1");
    a.wt().args(["push", "--branch", "feature"]).ok();
    let mr = a
        .wt()
        .args([
            "mr",
            "create",
            "--source",
            "feature",
            "--target",
            "main",
            "--title",
            "Add feature",
            "--description",
            "Needs CI and a review",
        ])
        .json();
    assert_eq!(mr["number"].as_u64(), Some(1), "{}", pretty(&mr));
    let main_before = server_tip(&alice_api, "alice", TREE, "main").expect("main");

    // Nothing reported yet: CI missing, no approvals.
    let c = checks(&a);
    assert_blocked(&c, "ci");
    assert_blocked(&c, "approvals");
    assert_eq!(
        c["ci"]["missing"],
        serde_json::json!([CHECK]),
        "{}",
        pretty(&c)
    );

    // --- failing CI blocks, even with an approval
    ci_report(&a, &tip1, "failure");
    let status = a.wt().args(["ci", "status", "--branch", "feature"]).json();
    assert_eq!(
        str_field(&status, "state"),
        "failure",
        "{}",
        pretty(&status)
    );
    assert_eq!(str_field(&status, "snapshot"), tip1);
    b.wt()
        .args(["mr", "review", "1", "--approve", "-m", "lgtm"])
        .ok();
    let c = checks(&a);
    assert_blocked(&c, "ci");
    assert!(
        !blocker_kinds(&c).contains(&"approvals".to_string()),
        "the approval counts:\n{}",
        pretty(&c)
    );
    assert_eq!(c["approvals"]["current"].as_u64(), Some(1));
    assert_eq!(c["ci"]["state"], "failure");
    let human = a.wt().args(["mr", "checks", "1"]).run();
    assert!(
        human.mentions(CHECK),
        "human checks output names the failing check\n{human}"
    );
    let merge = a.wt().args(["mr", "merge", "1"]).fails();
    assert!(
        merge.mentions("ci") || merge.mentions("blocked"),
        "merge refusal explains the CI gate\n{merge}"
    );
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(main_before.clone())
    );

    // --- success unblocks
    ci_report(&a, &tip1, "success");
    let c = checks(&a);
    assert_eq!(c["mergeable"], Value::Bool(true), "{}", pretty(&c));

    // --- a new push to the source: stale approval, CI missing again
    let tip2 = a.commit(&[("feature.txt", "feature v2\n")], "feature v2");
    a.wt().args(["push", "--branch", "feature"]).ok();
    let c = checks(&a);
    assert_blocked(&c, "approvals");
    assert_blocked(&c, "ci");
    assert_eq!(
        c["approvals"]["current"].as_u64(),
        Some(0),
        "{}",
        pretty(&c)
    );
    assert_eq!(c["approvals"]["stale"].as_u64(), Some(1), "{}", pretty(&c));
    a.wt().args(["mr", "merge", "1"]).fails();

    ci_report(&a, &tip2, "success");
    // A change request blocks until the same reviewer approves.
    b.wt()
        .args([
            "mr",
            "review",
            "1",
            "--request-changes",
            "-m",
            "please rename the file",
        ])
        .ok();
    let c = checks(&a);
    assert_blocked(&c, "changes_requested");
    a.wt().args(["mr", "merge", "1"]).fails();

    // --- comments
    b.wt()
        .args([
            "mr",
            "comment",
            "1",
            "-m",
            "nit: trailing newline",
            "--path",
            "feature.txt",
            "--line",
            "1",
        ])
        .ok();
    a.wt()
        .args(["mr", "comment", "1", "-m", "fixed in the next push, thanks"])
        .ok();
    b.wt()
        .args(["mr", "review", "1", "--comment", "-m", "looks good now"])
        .ok();
    let comments = a.wt().args(["mr", "comments", "1"]).json();
    let list = find_objects(&comments, &["id", "body", "author"]);
    let bodies: Vec<String> = list.iter().map(|c| str_field(c, "body")).collect();
    assert!(
        bodies.contains(&"nit: trailing newline".to_string())
            && bodies.contains(&"fixed in the next push, thanks".to_string()),
        "comments:\n{}",
        pretty(&comments)
    );
    let nit = list
        .iter()
        .find(|c| c["body"] == "nit: trailing newline")
        .expect("nit comment");
    assert_eq!(str_field(nit, "author"), "bob");
    assert_eq!(str_field(nit, "path"), "feature.txt");
    assert_eq!(nit["line"].as_u64(), Some(1));
    // Commenting does not lift the change request.
    assert_blocked(&checks(&a), "changes_requested");

    // --- fresh approval of the current tip: merge succeeds
    b.wt()
        .args(["mr", "review", "1", "--approve", "-m", "approved v2"])
        .ok();
    let c = checks(&a);
    assert_eq!(c["mergeable"], Value::Bool(true), "{}", pretty(&c));
    assert_eq!(c["approvals"]["current"].as_u64(), Some(1));
    let merged = a.wt().args(["mr", "merge", "1"]).json();
    assert_eq!(str_field(&merged, "state"), "merged", "{}", pretty(&merged));
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(tip2.clone()),
        "main fast-forwarded to the reviewed tip"
    );
    b.pull();
    assert_eq!(b.head_id(), tip2);
    assert_eq!(b.read("feature.txt"), "feature v2\n");
}
