//! Branch protection (no direct push, required approvals) and the merge
//! request flow over REST, observed through `wt push` / `wt pull`.

use serde_json::{json, Value};
use worktree_e2e::json::{self, array_field, find_objects, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

#[test]
fn branch_protection_and_mr() {
    require_e2e!();
    let env = TestEnv::with_server("branch_protection_and_mr");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# protected\n")]);
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    let bob_api = bob.api();
    let tree_path = format!("/api/v1/trees/alice/{TREE}");

    // --- protect main (owner only)
    let protection = json!({"pattern": "main", "no_direct_push": true, "required_approvals": 1});
    bob_api
        .put(&format!("{tree_path}/protections"), protection.clone())
        .assert_error(403, "forbidden");
    let p = alice_api
        .put(&format!("{tree_path}/protections"), protection)
        .ok_json();
    assert_eq!(str_field(&p, "pattern"), "main");
    let list = alice_api.get(&format!("{tree_path}/protections")).ok_json();
    let prots = find_objects(&list, &["pattern", "no_direct_push"]);
    assert_eq!(prots.len(), 1, "protections:\n{}", json::pretty(&list));
    assert_eq!(prots[0]["no_direct_push"], Value::Bool(true));
    assert_eq!(prots[0]["required_approvals"].as_u64(), Some(1));

    // --- direct push to main is rejected (even for the owner)
    let main_before = server_tip(&alice_api, "alice", TREE, "main").expect("main exists");
    a.commit(&[("direct.txt", "direct push attempt\n")], "direct to main");
    let out = a.wt().arg("push").fails();
    assert!(
        out.mentions("protect") || out.mentions("merge request") || out.mentions("review"),
        "rejection should explain the branch protection\n{out}"
    );
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(main_before.clone()),
        "main unchanged"
    );

    // --- push a feature branch instead
    a.wt().args(["switch", "-c", "feature"]).ok();
    let feat_tip = a.commit(&[("feature.txt", "reviewed feature\n")], "feature: add");
    a.wt().args(["push", "--branch", "feature"]).ok();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "feature"),
        Some(feat_tip.clone())
    );

    // --- open MR
    let resp = alice_api.post(
        &format!("{tree_path}/merge-requests"),
        json!({"title": "Add feature", "description": "Please review", "source_branch": "feature", "target_branch": "main"}),
    );
    resp.assert_status(201);
    let mr = resp.json();
    let number = mr["number"]
        .as_u64()
        .unwrap_or_else(|| panic!("MR number:\n{}", json::pretty(&mr)));
    assert_eq!(number, 1, "first MR of the tree is #1");
    assert_eq!(str_field(&mr, "state"), "open");
    assert_eq!(str_field(&mr, "source_branch"), "feature");
    assert_eq!(str_field(&mr, "target_branch"), "main");
    assert_eq!(str_field(&mr, "author"), "alice");
    let mr_path = format!("{tree_path}/merge-requests/{number}");
    let open = alice_api
        .get(&format!("{tree_path}/merge-requests?state=open"))
        .ok_json();
    assert_eq!(
        find_objects(&open, &["number", "state"]).len(),
        1,
        "open MRs:\n{}",
        json::pretty(&open)
    );
    assert_eq!(
        bob_api.get(&mr_path).ok_json()["title"],
        json!("Add feature")
    );

    // --- not mergeable without approvals; author may not approve
    let early = alice_api.post_empty(&format!("{mr_path}/merge"));
    assert!(
        !early.is_success(),
        "merge without the required approval must fail\n{early}"
    );
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(main_before.clone())
    );
    alice_api
        .post_empty(&format!("{mr_path}/approve"))
        .assert_error(403, "forbidden");

    // --- second user approves, owner merges (fast-forward)
    let approved = bob_api.post_empty(&format!("{mr_path}/approve")).ok_json();
    let approvals = array_field(&approved, "approvals");
    assert_eq!(
        approvals.len(),
        1,
        "approvals:\n{}",
        json::pretty(&approved)
    );
    assert_eq!(approvals[0]["username"], json!("bob"));
    let merged = alice_api.post_empty(&format!("{mr_path}/merge")).ok_json();
    assert_eq!(str_field(&merged, "state"), "merged");
    if let Some(ms) = json::opt_str(&merged, "merged_snapshot") {
        assert_eq!(
            ms, feat_tip,
            "fast-forward merge: merged snapshot is the source tip"
        );
    }
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(feat_tip.clone()),
        "main fast-forwarded"
    );
    let merged_list = alice_api
        .get(&format!("{tree_path}/merge-requests?state=merged"))
        .ok_json();
    assert_eq!(find_objects(&merged_list, &["number", "state"]).len(), 1);

    // --- pulls show the new main
    a.wt().args(["switch", "main"]).ok();
    a.pull();
    assert_eq!(
        a.head_id(),
        feat_tip,
        "alice's main fast-forwards to the merged tip"
    );
    assert!(a.exists("feature.txt") && a.exists("direct.txt"));
    a.assert_clean();
    b.pull();
    assert_eq!(
        b.head_id(),
        feat_tip,
        "bob's main fast-forwards to the merged tip"
    );
    assert_eq!(b.read("feature.txt"), "reviewed feature\n");
    b.assert_clean();
}
