//! Deeper review (ENGINEERING_PLAN §15.7): an `OWNERS` file on the default
//! branch makes its owners reviewers of a new merge request and, with
//! `wt protect set --require-owner-approval`, merging waits for an owner's
//! approval (`wt mr owners`); a draft (`wt mr create --draft`) cannot merge
//! until `wt mr ready`; `wt mr suggest` + `wt mr apply-suggestion` write a
//! snapshot on the source branch (an outdated suggestion is refused); a
//! tenant storage quota blocks a server-side merge snapshot.

use std::time::Duration;

use serde_json::Value;
use worktree_e2e::json::{self, find_objects, pretty, str_field};
use worktree_e2e::{poll_until, require_e2e, Repo, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

const NOTES: &str = "line one\nline two\nline three\nline four\nline five\n";

/// Branch `branch` from main, write `files`, snapshot, push, back to main.
#[track_caller]
fn push_branch(repo: &Repo, branch: &str, files: &[(&str, &str)], msg: &str) -> String {
    repo.wt().args(["switch", "-c", branch]).ok();
    let tip = repo.commit(files, msg);
    repo.wt().args(["push", "--branch", branch]).ok();
    repo.wt().args(["switch", "main"]).ok();
    tip
}

/// `wt --json mr create` from `source` into main (extra flags appended).
#[track_caller]
fn open_mr(repo: &Repo, source: &str, title: &str, extra: &[&str]) -> Value {
    let mut args = vec![
        "mr", "create", "--source", source, "--target", "main", "--title", title,
    ];
    args.extend_from_slice(extra);
    repo.wt().args(args).json()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn number(mr: &Value) -> String {
    mr["number"]
        .as_u64()
        .unwrap_or_else(|| panic!("MR number in\n{}", pretty(mr)))
        .to_string()
}

#[test]
fn review_v07() {
    require_e2e!();
    let env = TestEnv::with_server("review_v07");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("OWNERS", "# path owners\n**  @bob\n"),
            ("README.md", "# deeper review\n"),
            ("docs/guide.md", "guide\n"),
            ("notes.txt", NOTES),
        ],
    );
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "maintainer");
    let b = bob.clone_tree("alice", TREE, TREE);

    // --- owners come from the OWNERS file of the default branch
    let owners = a.wt().args(["owners", "show"]).json();
    assert_eq!(str_field(&owners, "source"), "file", "{}", pretty(&owners));
    assert_eq!(
        strings(&owners["rules"][0]["owners"]),
        vec!["user:bob"],
        "{}",
        pretty(&owners)
    );
    a.wt()
        .args(["protect", "set", "main", "--require-owner-approval"])
        .ok();
    let list = a.wt().args(["protect", "list"]).json();
    assert!(
        find_objects(&list, &["pattern"])
            .iter()
            .any(|p| p["pattern"] == "main" && p["require_owner_approval"] == Value::Bool(true)),
        "{}",
        pretty(&list)
    );

    // --- 1. opening an MR requests the owners; merging waits for them
    push_branch(
        &a,
        "feat-docs",
        &[("docs/guide.md", "guide, improved\n")],
        "docs",
    );
    let mr = open_mr(&a, "feat-docs", "Improve the guide", &[]);
    let n1 = number(&mr);
    assert_eq!(
        strings(&mr["reviewers"]),
        vec!["user:bob"],
        "the owner is requested:\n{}",
        pretty(&mr)
    );
    let own = a.wt().args(["mr", "owners", &n1]).json();
    assert_eq!(own["required"], Value::Bool(true), "{}", pretty(&own));
    assert_eq!(own["satisfied"], Value::Bool(false), "{}", pretty(&own));
    assert_eq!(strings(&own["missing"]), vec!["docs/guide.md"]);
    let text = a.wt().args(["mr", "owners", &n1]).ok();
    assert!(text.mentions("docs/guide.md"), "{text}");
    let main_before = server_tip(&alice_api, "alice", TREE, "main");
    let out = a.wt().args(["mr", "merge", &n1]).fails();
    assert!(
        out.mentions("owner"),
        "the refusal names owner approval\n{out}"
    );
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), main_before);
    b.wt().args(["mr", "review", &n1, "--approve"]).ok();
    let own = a.wt().args(["mr", "owners", &n1]).json();
    assert_eq!(own["satisfied"], Value::Bool(true), "{}", pretty(&own));
    let merged = a.wt().args(["mr", "merge", &n1]).json();
    assert_eq!(str_field(&merged, "state"), "merged", "{}", pretty(&merged));
    a.pull();

    // --- 2. a draft cannot merge until it is ready
    push_branch(&a, "feat-draft", &[("draft.txt", "draft\n")], "draft work");
    let draft = open_mr(&a, "feat-draft", "Draft work", &["--draft"]);
    let n2 = number(&draft);
    assert_eq!(draft["draft"], Value::Bool(true), "{}", pretty(&draft));
    assert!(
        strings(&draft["reviewers"]).is_empty(),
        "owners are requested when the draft becomes ready:\n{}",
        pretty(&draft)
    );
    let shown = a.wt().args(["mr", "show", &n2]).ok();
    assert!(shown.mentions("draft"), "{shown}");
    b.wt().args(["mr", "review", &n2, "--approve"]).ok();
    let out = a.wt().args(["mr", "merge", &n2]).fails();
    assert!(out.mentions("draft"), "draft refusal\n{out}");
    let checks = a.wt().args(["mr", "checks", &n2]).json();
    assert!(
        find_objects(&checks, &["kind", "message"])
            .iter()
            .any(|blk| blk["kind"] == "draft"),
        "{}",
        pretty(&checks)
    );
    let ready = a.wt().args(["mr", "ready", &n2]).json();
    assert!(
        ready.get("draft").is_none_or(|d| d == &Value::Bool(false)),
        "{}",
        pretty(&ready)
    );
    assert_eq!(
        strings(&ready["reviewers"]),
        vec!["user:bob"],
        "{}",
        pretty(&ready)
    );
    let merged = a.wt().args(["mr", "merge", &n2]).json();
    assert_eq!(str_field(&merged, "state"), "merged", "{}", pretty(&merged));
    a.pull();

    // --- 3. suggested changes become snapshots on the source branch
    let sugg_tip = push_branch(
        &a,
        "feat-sugg",
        &[("notes.txt", &NOTES.replace("line one", "line ONE"))],
        "notes",
    );
    let n3 = number(&open_mr(&a, "feat-sugg", "Notes", &[]));
    let c = b
        .wt()
        .args([
            "mr",
            "suggest",
            &n3,
            "--path",
            "notes.txt",
            "--lines",
            "2-3",
            "-m",
            "line TWO (suggested)",
        ])
        .json();
    let cid = str_field(&c, "id");
    assert_eq!(
        c["suggestion"]["snapshot_id"].as_str(),
        Some(sugg_tip.as_str()),
        "{}",
        pretty(&c)
    );
    let outdated = b
        .wt()
        .args([
            "mr",
            "suggest",
            &n3,
            "--path",
            "notes.txt",
            "--lines",
            "5",
            "-m",
            "line 5 (suggested)",
        ])
        .json();
    let outdated_id = str_field(&outdated, "id");
    let listed = a.wt().args(["mr", "comments", &n3]).ok();
    assert!(listed.mentions("line TWO (suggested)"), "{listed}");

    let applied = a.wt().args(["mr", "apply-suggestion", &n3, &cid]).json();
    let snap_id = str_field(&applied["snapshot"], "id");
    assert_eq!(
        json::parents(&applied["snapshot"]),
        vec![sugg_tip.clone()],
        "{}",
        pretty(&applied)
    );
    assert_eq!(
        applied["comment"]["suggestion"]["applied_snapshot"].as_str(),
        Some(snap_id.as_str()),
        "{}",
        pretty(&applied)
    );
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "feat-sugg"),
        Some(snap_id.clone())
    );
    a.wt().args(["switch", "feat-sugg"]).ok();
    a.pull();
    assert_eq!(
        a.read("notes.txt"),
        "line ONE\nline TWO (suggested)\nline four\nline five\n"
    );
    a.wt().args(["mr", "apply-suggestion", &n3, &cid]).fails();
    // The source changes line 5 (now line 4 after the applied suggestion's
    // deletion of a line): the second suggestion is outdated.
    a.commit(
        &[(
            "notes.txt",
            "line ONE\nline TWO (suggested)\nline four\nline FIVE (alice)\n",
        )],
        "edit line five",
    );
    a.wt().args(["push", "--branch", "feat-sugg"]).ok();
    let out = a
        .wt()
        .args(["mr", "apply-suggestion", &n3, &outdated_id])
        .fails();
    assert!(out.mentions("outdated"), "outdated refusal\n{out}");
    a.wt().args(["switch", "main"]).ok();

    // --- 4. a tenant storage quota blocks a server-side merge snapshot
    push_branch(
        &a,
        "feat-quota",
        &[(
            "notes.txt",
            &NOTES.replace("line five", "line FIVE (quota)"),
        )],
        "quota feature",
    );
    a.commit(
        &[("notes.txt", &NOTES.replace("line two", "line TWO (main)"))],
        "main moves on",
    );
    a.push();
    let n4 = number(&open_mr(&a, "feat-quota", "Quota", &[]));
    b.wt().args(["mr", "review", &n4, "--approve"]).ok();
    let admin = env.admin_user();
    admin.login();
    // Wait until the pushes are accounted, then allow almost nothing more.
    let used = poll_until(
        "storage usage to be recorded",
        Duration::from_secs(20),
        Duration::from_millis(200),
        || {
            let u = admin.wt().args(["admin", "usage", "alice"]).json();
            match u["storage_bytes"].as_u64() {
                Some(n) if n > 0 => Ok(n),
                _ => Err(pretty(&u)),
            }
        },
    );
    admin
        .wt()
        .args([
            "admin",
            "limits",
            "alice",
            "--max-storage",
            &(used + 8).to_string(),
        ])
        .ok();
    let main_before = server_tip(&alice_api, "alice", TREE, "main");
    let out = a
        .wt()
        .args(["mr", "merge", &n4, "--method", "merge"])
        .fails();
    assert!(out.mentions("quota"), "quota refusal\n{out}");
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), main_before);
    alice_api
        .post(
            &format!("/api/v1/trees/alice/{TREE}/merge-requests/{n4}/merge"),
            serde_json::json!({"method": "squash"}),
        )
        .assert_error(409, "quota_exceeded");
    admin
        .wt()
        .args(["admin", "limits", "alice", "--clear"])
        .ok();
    let merged = a
        .wt()
        .args(["mr", "merge", &n4, "--method", "merge"])
        .json();
    assert_eq!(
        str_field(&merged, "merge_method"),
        "merge",
        "{}",
        pretty(&merged)
    );
}
