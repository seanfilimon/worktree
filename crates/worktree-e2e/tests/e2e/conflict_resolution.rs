//! Conflict resolution on the server (ENGINEERING_PLAN §16.9): a conflicting
//! merge request is refused by `wt mr merge` with a hint naming `wt mr
//! conflicts`; `wt mr conflicts` reports base / ours / theirs and the diff3
//! markers, `--write --dir` writes them out, `wt mr resolve` refuses leftover
//! markers and then writes a merge-resolution snapshot on the source branch
//! (parents: source tip, target tip) from the edited file plus `--take
//! theirs` for a binary file; stale tips answer `resolve_outdated`; the
//! request then fast-forwards with `--delete-branch`. Finally a diverged
//! request merges with `wt mr merge --rebase`, replaying its snapshots onto
//! the target.

use serde_json::{json, Value};
use worktree_e2e::json::{self, find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, Repo, TestEnv};

use crate::common::{publish_tree, server_tip, TREE};

const NOTES: &str = "line one\nline two\nline three\nline four\nline five\n";
const RESOLVED: &str = "line one\nline two\nline three (feat + main)\nline four\nline five\n";

/// Branch `branch` from the current branch, write `files`, snapshot, push
/// it and return to main.
#[track_caller]
fn push_branch(repo: &Repo, branch: &str, files: &[(&str, &str)], msg: &str) -> String {
    repo.wt().args(["switch", "-c", branch]).ok();
    let tip = repo.commit(files, msg);
    repo.wt().args(["push", "--branch", branch]).ok();
    repo.wt().args(["switch", "main"]).ok();
    tip
}

#[track_caller]
fn open_mr(repo: &Repo, source: &str, title: &str) -> String {
    let v = repo
        .wt()
        .args([
            "mr", "create", "--source", source, "--target", "main", "--title", title,
        ])
        .json();
    v["number"]
        .as_u64()
        .unwrap_or_else(|| panic!("MR number in\n{}", pretty(&v)))
        .to_string()
}

#[test]
fn conflict_resolution() {
    require_e2e!();
    let env = TestEnv::with_server("conflict_resolution");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("README.md", "# conflicts\n"),
            ("notes.txt", NOTES),
            ("logo.bin", "BIN\u{0}base"),
        ],
    );
    let api = alice.api();

    // --- 1. a conflicting merge request
    let feat_tip = push_branch(
        &a,
        "feat",
        &[
            (
                "notes.txt",
                &NOTES.replace("line three", "line three (feat)"),
            ),
            ("logo.bin", "BIN\u{0}feat"),
            ("feat.txt", "from feat\n"),
        ],
        "feat edits",
    );
    let main_tip = a.commit(
        &[
            (
                "notes.txt",
                &NOTES.replace("line three", "line three (main)"),
            ),
            ("logo.bin", "BIN\u{0}main"),
            ("main.txt", "from main\n"),
        ],
        "main edits",
    );
    a.push();
    let n = open_mr(&a, "feat", "Conflicting feature");
    let out = a.wt().args(["mr", "merge", &n]).code(3);
    assert!(out.mentions("merge_conflict"), "{out}");
    assert!(
        out.mentions(&format!("wt mr conflicts {n}")),
        "the hint names the server-side resolution\n{out}"
    );

    // --- 2. the conflict report
    let rep = a.wt().args(["mr", "conflicts", &n]).json();
    assert_eq!(rep["total"], json!(2), "{}", pretty(&rep));
    assert_eq!(rep["resolvable"], json!(true), "{}", pretty(&rep));
    assert_eq!(str_field(&rep, "source_tip"), feat_tip);
    assert_eq!(str_field(&rep, "target_tip"), main_tip);
    let files = find_objects(&rep, &["path", "kind", "binary"]);
    let notes = files
        .iter()
        .find(|f| f["path"] == "notes.txt")
        .unwrap_or_else(|| panic!("notes.txt in\n{}", pretty(&rep)));
    assert_eq!(str_field(notes, "kind"), "content");
    assert_eq!(
        notes["ours"]["text"].as_str(),
        Some(NOTES.replace("line three", "line three (feat)").as_str()),
        "ours is the source branch"
    );
    let markers = str_field(notes, "markers");
    assert!(
        markers.contains("<<<<<<< feat\nline three (feat)\n||||||| base\nline three\n=======\nline three (main)\n>>>>>>> main\n"),
        "{markers}"
    );
    let logo = files
        .iter()
        .find(|f| f["path"] == "logo.bin")
        .unwrap_or_else(|| panic!("logo.bin in\n{}", pretty(&rep)));
    assert_eq!(logo["binary"], json!(true), "{}", pretty(logo));
    let text = a.wt().args(["mr", "conflicts", &n, "--markers"]).ok();
    assert!(text.mentions("notes.txt (content)"), "{text}");
    assert!(text.mentions("logo.bin (binary)"), "{text}");
    assert!(text.mentions("<<<<<<< feat"), "{text}");

    // --- 3. write the markers out, resolve them
    let dir = env.path("resolution");
    let dir_arg = dir.to_string_lossy().to_string();
    let written = a
        .wt()
        .args(["mr", "conflicts", &n, "--write", "--dir", &dir_arg])
        .json();
    assert_eq!(
        written["written"],
        json!(["notes.txt"]),
        "{}",
        pretty(&written)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).expect("written markers"),
        markers
    );
    // Leftover markers are refused (exit 3), a missing file too.
    let out = a
        .wt()
        .args([
            "mr", "resolve", &n, "--dir", &dir_arg, "--take", "theirs", "logo.bin",
        ])
        .code(3);
    assert!(out.mentions("conflict markers"), "{out}");
    let out = a
        .wt()
        .args(["mr", "resolve", &n, "--dir", &dir_arg])
        .fails();
    assert!(out.mentions("logo.bin"), "{out}");
    std::fs::write(dir.join("notes.txt"), RESOLVED).expect("write resolution");

    // Stale tips answer resolve_outdated.
    api.post(
        &format!("/api/v1/trees/alice/{TREE}/merge-requests/{n}/resolve"),
        json!({"files": [{"path": "notes.txt", "content": RESOLVED}, {"path": "logo.bin", "take": "theirs"}],
               "source_tip": main_tip}),
    )
    .assert_error(409, "resolve_outdated");

    let resolved = a
        .wt()
        .args([
            "mr", "resolve", &n, "--dir", &dir_arg, "--take", "theirs", "logo.bin",
        ])
        .json();
    let snap = &resolved["snapshot"];
    let resolution = str_field(snap, "id");
    assert_eq!(
        json::parents(snap),
        vec![feat_tip.clone(), main_tip.clone()],
        "the resolution merges the target into the source:\n{}",
        pretty(&resolved)
    );
    assert_eq!(str_field(snap, "branch_name"), "feat");
    assert_eq!(resolved["paths"], json!(["logo.bin", "notes.txt"]));
    assert_eq!(
        server_tip(&api, "alice", TREE, "feat"),
        Some(resolution.clone())
    );
    assert_eq!(
        server_tip(&api, "alice", TREE, "main"),
        Some(main_tip.clone())
    );
    let after = a.wt().args(["mr", "conflicts", &n]).json();
    assert_eq!(after["total"], json!(0), "{}", pretty(&after));
    let m = a.wt().args(["mr", "mergeability", &n]).json();
    assert_eq!(m["fast_forward"], Value::Bool(true), "{}", pretty(&m));

    // --- 4. merge, deleting the source branch
    let merged = a.wt().args(["mr", "merge", &n, "--delete-branch"]).json();
    assert_eq!(str_field(&merged, "state"), "merged", "{}", pretty(&merged));
    assert_eq!(str_field(&merged, "merged_snapshot"), resolution);
    assert_eq!(
        server_tip(&api, "alice", TREE, "feat"),
        None,
        "feat was deleted"
    );
    a.pull();
    assert_eq!(a.head_id(), resolution);
    assert_eq!(a.read("notes.txt"), RESOLVED);
    assert_eq!(a.read("logo.bin"), "BIN\u{0}main");
    assert_eq!(a.read("feat.txt"), "from feat\n");
    assert_eq!(a.read("main.txt"), "from main\n");
    a.assert_clean();
    a.wt().args(["fsck", "--full"]).ok();

    // --- 5. a rebase merge replays the source snapshots onto the target
    a.wt().args(["switch", "-c", "feat-r"]).ok();
    let r1 = a.commit(&[("r1.txt", "first\n")], "rebase me: first");
    let r2 = a.commit(&[("r2.txt", "second\n")], "rebase me: second");
    a.wt().args(["push", "--branch", "feat-r"]).ok();
    a.wt().args(["switch", "main"]).ok();
    let main2 = a.commit(&[("main2.txt", "main again\n")], "main again");
    a.push();
    let nr = open_mr(&a, "feat-r", "Rebased feature");
    let m = a.wt().args(["mr", "mergeability", &nr]).json();
    assert!(
        m["methods"]
            .as_array()
            .is_some_and(|ms| ms.contains(&json!("rebase"))),
        "{}",
        pretty(&m)
    );
    let rebased = a.wt().args(["mr", "merge", &nr, "--rebase"]).json();
    assert_eq!(
        str_field(&rebased, "merge_method"),
        "rebase",
        "{}",
        pretty(&rebased)
    );
    let tip = str_field(&rebased, "merged_snapshot");
    assert_ne!(tip, r2, "rebasing writes new snapshots");
    a.pull();
    let log = a.log(&["-n", "3"]);
    let messages: Vec<String> = log.iter().map(|s| str_field(s, "message")).collect();
    assert_eq!(
        messages,
        vec!["rebase me: second", "rebase me: first", "main again"],
        "linear history with the original messages:\n{}",
        pretty(&Value::Array(log.clone()))
    );
    assert_eq!(str_field(&log[0], "id"), tip);
    assert_ne!(str_field(&log[1], "id"), r1);
    assert_eq!(json::parents(&log[1]), vec![main2.clone()]);
    assert_eq!(a.read("r1.txt"), "first\n");
    assert_eq!(a.read("main2.txt"), "main again\n");
    a.assert_clean();
}
