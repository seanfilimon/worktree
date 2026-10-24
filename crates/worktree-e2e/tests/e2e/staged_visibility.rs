//! Team visibility of staged (WIP) snapshots and their clearing on push.

use worktree_e2e::json::{self, array_field, find_objects, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, staged_by, TREE};

#[test]
fn staged_visibility() {
    require_e2e!();
    let env = TestEnv::with_server("staged_visibility");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("README.md", "# staged demo\n"),
            ("src/main.txt", "main v1\n"),
        ],
    );
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);

    // Nothing staged yet.
    let team = b.wt().args(["status", "--team"]).json();
    assert!(
        staged_by(&team, "alice").is_empty(),
        "no staged work yet:\n{}",
        json::pretty(&team)
    );

    // alice stages uncommitted work.
    a.write("notes.txt", "wip notes from alice\n");
    a.write("src/main.txt", "main v2, work in progress\n");
    a.wt().arg("stage").ok();
    let head_before = a.head_id();
    // Staging neither snapshots nor resets the working directory.
    assert_eq!(a.changes().len(), 2, "working changes survive `wt stage`");
    assert_eq!(a.head_id(), head_before, "stage must not move the branch");

    // bob sees it.
    let team = b.wt().args(["status", "--team"]).json();
    let entries = staged_by(&team, "alice");
    assert_eq!(
        entries.len(),
        1,
        "exactly one staged entry by alice:\n{}",
        json::pretty(&team)
    );
    let entry = entries[0];
    let files: Vec<String> = array_field(entry, "files_changed")
        .iter()
        .filter_map(|f| f.as_str().map(String::from))
        .collect();
    assert!(
        files.contains(&"notes.txt".to_string()),
        "files_changed: {files:?}"
    );
    assert!(
        files.contains(&"src/main.txt".to_string()),
        "files_changed: {files:?}"
    );
    let branch = entry
        .get("branch")
        .or_else(|| entry.get("branch_name"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(
        branch,
        "main",
        "staged entry branch:\n{}",
        json::pretty(entry)
    );
    if let Some(added) = entry.get("files_added").and_then(|v| v.as_u64()) {
        assert_eq!(added, 1, "files_added:\n{}", json::pretty(entry));
    }
    if let Some(modified) = entry.get("files_modified").and_then(|v| v.as_u64()) {
        assert_eq!(modified, 1, "files_modified:\n{}", json::pretty(entry));
    }

    // REST view agrees.
    let rest = alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}/staged?branch=main"))
        .ok_json();
    let rest_entries = find_objects(&rest, &["files_changed"]);
    assert_eq!(
        rest_entries.len(),
        1,
        "REST staged list:\n{}",
        json::pretty(&rest)
    );
    let summary = alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}"))
        .ok_json();
    assert_eq!(
        summary["staged_count"].as_u64(),
        Some(1),
        "tree summary:\n{}",
        json::pretty(&summary)
    );

    // Restaging replaces (never duplicates) the entry.
    a.write("notes.txt", "wip notes from alice, revised and longer\n");
    a.wt().arg("stage").ok();
    let team = b.wt().args(["status", "--team"]).json();
    assert_eq!(
        staged_by(&team, "alice").len(),
        1,
        "restage replaces:\n{}",
        json::pretty(&team)
    );

    // Pushing clears alice's staged entry for the branch.
    a.snapshot("notes + main v2");
    a.push();
    let team = b.wt().args(["status", "--team"]).json();
    assert!(
        staged_by(&team, "alice").is_empty(),
        "push clears the staged entry:\n{}",
        json::pretty(&team)
    );
    let rest = alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}/staged"))
        .ok_json();
    assert!(
        find_objects(&rest, &["files_changed"]).is_empty(),
        "REST staged after push:\n{}",
        json::pretty(&rest)
    );

    // And bob can pull the real snapshot.
    b.pull();
    assert_eq!(b.head_id(), a.head_id());
    assert_eq!(b.read("notes.txt"), a.read("notes.txt"));
    assert_eq!(str_field(&b.head(), "message"), "notes + main v2");
}
