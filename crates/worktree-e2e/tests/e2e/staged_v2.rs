//! staged-v2 (ENGINEERING_PLAN §16.8): labeled staged slots, `wt stage
//! list|show`, `wt checkout --staged USER/LABEL`, the push-time advisory,
//! the status lifecycle/history and hiding staged metadata from anonymous
//! readers of public trees.

use serde_json::{json, Value};
use worktree_e2e::json::{self, str_field};
use worktree_e2e::{require_e2e, Api, TestEnv};

use crate::common::{add_member, publish_tree, staged_by, TREE};

fn labels_of(entries: &[&Value]) -> Vec<String> {
    let mut out: Vec<String> = entries
        .iter()
        .map(|e| {
            e.get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect();
    out.sort();
    out
}

#[test]
fn staged_v2() {
    require_e2e!();
    let env = TestEnv::with_server("staged_v2");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[("README.md", "# staged v2\n"), ("src/app.txt", "app v1\n")],
    );
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    let staged_api = format!("/api/v1/trees/alice/{TREE}/staged");

    // --- two slots: the default one and "exp"
    a.write("src/app.txt", "app v2\n");
    a.wt().arg("stage").ok();
    a.write("notes.txt", "experiment notes\n");
    let out = a.wt().args(["stage", "--label", "exp"]).json();
    assert_eq!(str_field(&out, "label"), "exp", "{}", json::pretty(&out));
    // Invalid labels are refused client-side.
    a.wt().args(["stage", "--label", "Bad Label"]).fails();

    let list = b.wt().args(["stage", "list"]).json();
    let entries = staged_by(&list, "alice");
    assert_eq!(
        labels_of(&entries),
        vec!["".to_string(), "exp".to_string()],
        "two slots:\n{}",
        json::pretty(&list)
    );

    // --- wt stage show
    let shown = b
        .wt()
        .args(["stage", "show", "exp", "--user", "alice", "--files"])
        .json();
    let staged = &shown["staged"];
    assert_eq!(
        str_field(staged, "label"),
        "exp",
        "{}",
        json::pretty(&shown)
    );
    let changed: Vec<&str> = staged["files_changed"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert!(
        changed.contains(&"notes.txt") && changed.contains(&"src/app.txt"),
        "changed files: {changed:?}"
    );
    let all_files = shown["files"]["files"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        all_files.iter().any(|f| f["path"] == "README.md"),
        "working state files:\n{}",
        json::pretty(&shown)
    );

    // --- wt checkout --staged alice/exp
    let co = b.wt().args(["checkout", "--staged", "alice/exp"]).json();
    assert_eq!(
        str_field(&co, "branch"),
        "staged/alice/exp",
        "{}",
        json::pretty(&co)
    );
    assert_eq!(b.current_branch(), "staged/alice/exp");
    assert_eq!(b.read("notes.txt"), "experiment notes\n");
    assert_eq!(b.read("src/app.txt"), "app v2\n");
    assert_eq!(str_field(&b.head(), "author"), "alice");
    // The branch exists now: a second checkout is refused.
    b.wt().args(["checkout", "--staged", "alice/exp"]).fails();
    b.wt().args(["switch", "main"]).ok();
    assert!(!b.exists("notes.txt"), "back on main");

    // --- anonymous readers of a public tree see no staged metadata
    alice_api
        .patch(
            &format!("/api/v1/trees/alice/{TREE}"),
            json!({ "visibility": "public" }),
        )
        .assert_ok();
    let anon = Api::new(&env.url());
    let anon_list = anon.get(&staged_api).ok_json();
    assert_eq!(anon_list, json!([]), "anonymous staged list");
    let summary = anon.get(&format!("/api/v1/trees/alice/{TREE}")).ok_json();
    assert_eq!(
        summary["staged_count"].as_u64(),
        Some(0),
        "{}",
        json::pretty(&summary)
    );
    anon.get(&format!("{staged_api}/overlaps?paths=src/app.txt"))
        .assert_status(401);
    let summary = alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}"))
        .ok_json();
    assert_eq!(
        summary["staged_count"].as_u64(),
        Some(2),
        "{}",
        json::pretty(&summary)
    );

    // --- alice pushes app v2: the default slot ends as "pushed", "exp"
    // (whose working state also had notes.txt) stays live.
    a.remove("notes.txt");
    a.snapshot("app v2");
    a.push();
    let list = b.wt().args(["stage", "list"]).json();
    let entries = staged_by(&list, "alice");
    assert_eq!(
        labels_of(&entries),
        vec!["exp".to_string()],
        "push ends the default slot only:\n{}",
        json::pretty(&list)
    );

    // --- push-time advisory: bob pushes notes.txt, which alice has staged
    b.pull();
    b.write("notes.txt", "bob's notes\n");
    b.snapshot("bob notes");
    let pushed = b.wt().arg("push").ok();
    assert!(
        pushed.mentions("alice has staged changes to notes.txt"),
        "push advisory:\n{pushed}"
    );
    assert!(pushed.mentions("label exp"), "push advisory:\n{pushed}");
    // The advisory never blocks: the push went through.
    assert_eq!(
        crate::common::server_tip(&alice_api, "alice", TREE, "main").as_deref(),
        Some(b.head_id().as_str())
    );

    // --- alice withdraws "exp": status "cleared"
    let removed = a.wt().args(["unstage", "--label", "exp"]).json();
    assert_eq!(
        removed["removed"],
        json!(true),
        "{}",
        json::pretty(&removed)
    );
    let nothing = a.wt().args(["unstage", "--label", "exp"]).json();
    assert_eq!(nothing["removed"], json!(false));
    assert!(staged_by(&b.wt().args(["stage", "list"]).json(), "alice").is_empty());

    // --- history: pushed + cleared, newest first
    let hist = alice_api.get(&format!("{staged_api}/history")).ok_json();
    let entries = hist["history"].as_array().cloned().unwrap_or_default();
    let statuses: Vec<(String, String)> = entries
        .iter()
        .map(|h| {
            (
                str_field(h, "status"),
                h["label"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("cleared".to_string(), "exp".to_string()),
            ("pushed".to_string(), String::new()),
        ],
        "history:\n{}",
        json::pretty(&hist)
    );
    let listed = b.wt().args(["stage", "list", "--history"]).json();
    assert_eq!(
        listed["history"].as_array().map(Vec::len),
        Some(2),
        "{}",
        json::pretty(&listed)
    );
    let anon_hist = anon.get(&format!("{staged_api}/history")).ok_json();
    assert_eq!(anon_hist["history"], json!([]), "anonymous history");
}
