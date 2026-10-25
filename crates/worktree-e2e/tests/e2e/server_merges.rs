//! Server-side merges (ENGINEERING_PLAN §14.1): a diverged merge request
//! merged with `wt mr merge --method merge` (a merge snapshot with both tips
//! as parents and the content merged line-wise), another with `--method
//! squash` (one snapshot on the target), a conflicting one refused with
//! `merge_conflict` (exit 3) and explained by `wt mr mergeability`, and
//! `wt protect set --merge-methods fast-forward` refusing a merge snapshot.

use std::time::Duration;

use serde_json::Value;
use worktree_e2e::json::{self, find_objects, pretty, str_field};
use worktree_e2e::{poll_until, require_e2e, Repo, TestEnv};

use crate::common::{publish_tree, server_tip, TREE};

const NOTES: &str = "line one\nline two\nline three\nline four\nline five\n";

/// Branch `branch` from the current main, write `files`, snapshot, push it
/// and return to main.
#[track_caller]
fn push_branch(repo: &Repo, branch: &str, files: &[(&str, &str)], msg: &str) -> String {
    repo.wt().args(["switch", "-c", branch]).ok();
    let tip = repo.commit(files, msg);
    repo.wt().args(["push", "--branch", branch]).ok();
    repo.wt().args(["switch", "main"]).ok();
    tip
}

/// `wt mr create` from `source` into main; returns the MR number.
#[track_caller]
fn open_mr(repo: &Repo, source: &str, title: &str) -> u64 {
    let v = repo
        .wt()
        .args([
            "mr", "create", "--source", source, "--target", "main", "--title", title,
        ])
        .json();
    v["number"]
        .as_u64()
        .unwrap_or_else(|| panic!("MR number in\n{}", pretty(&v)))
}

/// `wt --json mr mergeability N`.
#[track_caller]
fn mergeability(repo: &Repo, number: u64) -> Value {
    repo.wt()
        .args(["mr", "mergeability", &number.to_string()])
        .json()
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn server_merges() {
    require_e2e!();
    let env = TestEnv::with_server("server_merges");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[("README.md", "# merges\n"), ("notes.txt", NOTES)],
    );
    let api = alice.api();

    // --- 1. diverged MR merged with a merge snapshot
    let feat_tip = push_branch(
        &a,
        "feat-a",
        &[
            (
                "notes.txt",
                &NOTES.replace("line one", "line ONE (feature)"),
            ),
            ("a.txt", "from feature a\n"),
        ],
        "feature a",
    );
    let main_tip = a.commit(
        &[
            ("notes.txt", &NOTES.replace("line five", "line FIVE (main)")),
            ("main.txt", "from main\n"),
        ],
        "main moves on",
    );
    a.push();
    let n1 = open_mr(&a, "feat-a", "Feature A");
    let m = mergeability(&a, n1);
    assert_eq!(m["fast_forward"], Value::Bool(false), "{}", pretty(&m));
    let methods = strings(&m, "methods");
    assert!(
        methods.contains(&"merge".to_string()) && methods.contains(&"squash".to_string()),
        "a diverged clean MR can be merged or squashed:\n{}",
        pretty(&m)
    );
    assert!(
        !methods.contains(&"fast-forward".to_string()),
        "{}",
        pretty(&m)
    );
    assert!(
        m["conflicts"].as_array().is_none_or(|c| c.is_empty()),
        "{}",
        pretty(&m)
    );
    let text = a.wt().args(["mr", "mergeability", &n1.to_string()]).ok();
    assert!(text.mentions("diverged"), "{text}");

    let merged = a
        .wt()
        .args(["mr", "merge", &n1.to_string(), "--method", "merge"])
        .json();
    assert_eq!(str_field(&merged, "state"), "merged", "{}", pretty(&merged));
    assert_eq!(str_field(&merged, "merge_method"), "merge");
    let merge_id = str_field(&merged, "merged_snapshot");
    assert_eq!(
        server_tip(&api, "alice", TREE, "main"),
        Some(merge_id.clone())
    );
    a.pull();
    let head = a.head();
    assert_eq!(str_field(&head, "id"), merge_id);
    assert_eq!(
        json::parents(&head),
        vec![main_tip.clone(), feat_tip.clone()],
        "merge snapshot parents are [target tip, source tip]:\n{}",
        pretty(&head)
    );
    assert!(
        str_field(&head, "message").contains("feat-a"),
        "default merge message names the source:\n{}",
        pretty(&head)
    );
    assert_eq!(
        a.read("notes.txt"),
        NOTES
            .replace("line one", "line ONE (feature)")
            .replace("line five", "line FIVE (main)"),
        "both edits of notes.txt are merged"
    );
    assert_eq!(a.read("a.txt"), "from feature a\n");
    assert_eq!(a.read("main.txt"), "from main\n");
    a.assert_clean();
    a.wt().args(["fsck", "--full"]).ok();
    // The server attributes the merged lines to the snapshots that wrote
    // them, not to the merge snapshot.
    let blame = api
        .get(&format!("/api/v1/trees/alice/{TREE}/blame/main/notes.txt"))
        .ok_json();
    let by_line: Vec<String> = find_objects(&blame, &["line", "snapshot_id"])
        .into_iter()
        .map(|l| str_field(l, "snapshot_id"))
        .collect();
    assert_eq!(by_line.len(), 5, "{}", pretty(&blame));
    assert_eq!(by_line[0], feat_tip, "{}", pretty(&blame));
    assert_eq!(by_line[4], main_tip, "{}", pretty(&blame));
    // Code search indexes the merged default branch.
    poll_until(
        "code search to see the server-side merge",
        Duration::from_secs(20),
        Duration::from_millis(200),
        || {
            let v = alice
                .wt()
                .args(["search", "code", "line ONE (feature)"])
                .json();
            let found = find_objects(&v, &["path", "line", "text"])
                .iter()
                .any(|h| h["path"] == "notes.txt" && h["snapshot_id"] == merge_id.as_str());
            if found {
                Ok(())
            } else {
                Err(pretty(&v))
            }
        },
    );

    // --- 2. squash: one snapshot on the target with the combined changes
    let notes_now = a.read("notes.txt");
    push_branch(
        &a,
        "feat-b",
        &[("notes.txt", &notes_now.replace("line two", "line TWO (b)"))],
        "feature b: notes",
    );
    a.wt().args(["switch", "feat-b"]).ok();
    let b_tip = a.commit(&[("b.txt", "from feature b\n")], "feature b: file");
    a.wt().args(["push", "--branch", "feat-b"]).ok();
    a.wt().args(["switch", "main"]).ok();
    let main_tip2 = a.commit(&[("main2.txt", "main again\n")], "main again");
    a.push();
    let n2 = open_mr(&a, "feat-b", "Squashed feature B");
    let squashed = a
        .wt()
        .args(["mr", "merge", &n2.to_string(), "--method", "squash"])
        .json();
    assert_eq!(str_field(&squashed, "state"), "merged");
    assert_eq!(str_field(&squashed, "merge_method"), "squash");
    let squash_id = str_field(&squashed, "merged_snapshot");
    assert_ne!(squash_id, b_tip);
    a.pull();
    let head = a.head();
    assert_eq!(str_field(&head, "id"), squash_id);
    assert_eq!(
        json::parents(&head),
        vec![main_tip2.clone()],
        "a squash snapshot has only the target tip as parent:\n{}",
        pretty(&head)
    );
    assert_eq!(str_field(&head, "message"), "Squashed feature B");
    assert_eq!(a.read("b.txt"), "from feature b\n");
    assert_eq!(a.read("main2.txt"), "main again\n");
    assert!(a.read("notes.txt").contains("line TWO (b)"));
    assert!(a.read("notes.txt").contains("line ONE (feature)"));
    a.assert_clean();

    // --- 3. conflicting MR: refused with merge_conflict, exit code 3
    let notes_now = a.read("notes.txt");
    push_branch(
        &a,
        "feat-c",
        &[(
            "notes.txt",
            &notes_now.replace("line three", "line three (C)"),
        )],
        "feature c",
    );
    a.commit(
        &[(
            "notes.txt",
            &notes_now.replace("line three", "line three (main)"),
        )],
        "main edits line three",
    );
    a.push();
    let before = server_tip(&api, "alice", TREE, "main");
    let n3 = open_mr(&a, "feat-c", "Conflicting C");
    let m = mergeability(&a, n3);
    let conflicts = find_objects(&m, &["path", "kind"]);
    assert!(
        conflicts.iter().any(|c| c["path"] == "notes.txt"),
        "mergeability lists the conflicting path:\n{}",
        pretty(&m)
    );
    assert!(strings(&m, "methods").is_empty(), "{}", pretty(&m));
    let out = a
        .wt()
        .args(["mr", "merge", &n3.to_string(), "--method", "merge"])
        .code(3);
    assert!(out.mentions("merge_conflict"), "{out}");
    assert!(
        out.mentions("mergeability"),
        "hint names the command\n{out}"
    );
    assert_eq!(server_tip(&api, "alice", TREE, "main"), before);
    let mr = a.wt().args(["mr", "show", &n3.to_string()]).json();
    assert_eq!(str_field(&mr, "state"), "open", "{}", pretty(&mr));

    // --- 4. protection allows only fast-forward: a merge snapshot is refused
    a.wt()
        .args(["protect", "set", "main", "--merge-methods", "fast-forward"])
        .ok();
    let rules = a.wt().args(["protect", "list"]).json();
    assert!(
        rules.to_string().contains("fast-forward"),
        "protection lists its merge methods:\n{}",
        pretty(&rules)
    );
    a.pull();
    push_branch(&a, "feat-d", &[("d.txt", "from feature d\n")], "feature d");
    a.commit(&[("main3.txt", "main three\n")], "main three");
    a.push();
    let n4 = open_mr(&a, "feat-d", "Feature D");
    let m = mergeability(&a, n4);
    assert_eq!(
        strings(&m, "allowed"),
        vec!["fast-forward".to_string()],
        "{}",
        pretty(&m)
    );
    assert!(strings(&m, "methods").is_empty(), "{}", pretty(&m));
    let before = server_tip(&api, "alice", TREE, "main");
    let out = a
        .wt()
        .args(["mr", "merge", &n4.to_string(), "--method", "merge"])
        .fails();
    assert!(out.mentions("merge_method_not_allowed"), "{out}");
    assert_eq!(server_tip(&api, "alice", TREE, "main"), before);
    // Rebasing the pushed feature locally is refused without --force.
    a.wt().args(["switch", "feat-d"]).ok();
    let out = a.wt().args(["rebase", "main"]).fails();
    assert!(out.mentions("--force") || out.mentions("pushed"), "{out}");
    a.wt().args(["rebase", "main", "--force"]).ok();
    let rebased = a.head();
    assert_eq!(
        json::parents(&rebased),
        vec![before.clone().unwrap()],
        "feat-d now sits on main:\n{}",
        pretty(&rebased)
    );
    // The server keeps the old tip: pushing the rewritten branch is not a
    // fast-forward.
    a.wt().args(["push", "--branch", "feat-d"]).fails();
    a.wt().args(["switch", "main"]).ok();

    // A source branched from the current main fast-forwards: allowed.
    let e_tip = push_branch(&a, "feat-e", &[("e.txt", "from feature e\n")], "feature e");
    let n5 = open_mr(&a, "feat-e", "Feature E");
    let m = mergeability(&a, n5);
    assert_eq!(m["fast_forward"], Value::Bool(true), "{}", pretty(&m));
    assert_eq!(
        strings(&m, "methods"),
        vec!["fast-forward".to_string()],
        "{}",
        pretty(&m)
    );
    let ff = a.wt().args(["mr", "merge", &n5.to_string()]).json();
    assert_eq!(str_field(&ff, "merge_method"), "fast-forward");
    assert_eq!(str_field(&ff, "merged_snapshot"), e_tip);
    assert_eq!(server_tip(&api, "alice", TREE, "main"), Some(e_tip));
}
