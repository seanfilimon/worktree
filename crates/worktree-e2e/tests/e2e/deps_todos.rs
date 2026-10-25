//! Snapshot dependencies and TODOs (ENGINEERING_PLAN §13.5): a snapshot of
//! tree `proj` declaring `--depend lib --todo ... --blocking` creates a TODO
//! in `lib` when pushed; a `lib` snapshot whose message carries
//! `Completes: #1` marks it done; `wt deps status` follows both sides.

use std::time::Duration;

use serde_json::Value;
use worktree_e2e::json::{self, find_objects, pretty, str_field};
use worktree_e2e::{poll_until, require_e2e, Repo, TestEnv};

use crate::common::publish_tree;

const TITLE: &str = "Expose parse() in lib";

/// TODO records (`{number, title, status, ...}`) in `wt --json todo list`.
fn todos(v: &Value) -> Vec<Value> {
    find_objects(v, &["number", "title", "status"])
        .into_iter()
        .cloned()
        .collect()
}

#[track_caller]
fn todo_list(repo: &Repo, status: &str) -> Vec<Value> {
    todos(&repo.wt().args(["todo", "list", "--status", status]).json())
}

/// The outgoing dependency entries of a `wt --json deps status` report.
fn outgoing(v: &Value) -> Vec<Value> {
    find_objects(v, &["snapshot_id", "dependency"])
        .into_iter()
        .cloned()
        .collect()
}

#[test]
fn deps_todos() {
    require_e2e!();
    let env = TestEnv::with_server("deps_todos");
    let alice = env.user("alice");
    let app = publish_tree(&env, &alice, &[("README.md", "# app\n")]);
    let lib = alice.init("lib");
    lib.commit(&[("lib.txt", "lib v1\n")], "lib: initial");
    lib.add_remote("origin", &env.url(), "alice");
    lib.push();

    assert!(
        todo_list(&lib, "all").is_empty(),
        "no TODOs in lib before the dependent push"
    );

    // --- app declares a blocking dependency on lib
    app.write("app.txt", "uses lib::parse()\n");
    app.wt()
        .args([
            "snapshot",
            "-m",
            "app: call lib::parse",
            "--depend",
            "lib",
            "--todo",
            TITLE,
            "--details",
            "app needs a parse() entry point",
            "--priority",
            "high",
            "--blocking",
        ])
        .ok();
    let dep_snapshot = app.head();
    let deps = json::array_field(&dep_snapshot, "dependencies");
    assert_eq!(deps.len(), 1, "{}", pretty(&dep_snapshot));
    assert_eq!(str_field(&deps[0], "tree"), "lib");
    assert_eq!(str_field(&deps[0], "title"), TITLE);
    assert_eq!(deps[0]["blocking"], Value::Bool(true));
    let dep_id = str_field(&dep_snapshot, "id");
    app.push();

    // --- the TODO appears in lib (created asynchronously on branch.pushed)
    let todo = poll_until(
        "the dependency TODO in lib",
        Duration::from_secs(20),
        Duration::from_millis(200),
        || {
            let list = todo_list(&lib, "active");
            match list.as_slice() {
                [t] => Ok(t.clone()),
                other => Err(format!("{other:?}")),
            }
        },
    );
    assert_eq!(todo["number"].as_u64(), Some(1), "{}", pretty(&todo));
    assert_eq!(str_field(&todo, "title"), TITLE);
    assert_eq!(str_field(&todo, "status"), "open");
    assert_eq!(str_field(&todo, "priority"), "high");
    assert_eq!(todo["blocking"], Value::Bool(true));
    assert_eq!(str_field(&todo, "source_snapshot_id"), dep_id);
    assert_eq!(str_field(&todo, "source_branch"), "main");
    let shown = lib.wt().args(["todo", "show", "1"]).ok();
    assert!(
        shown.stdout.contains(TITLE) && shown.mentions("open"),
        "`wt todo show 1`\n{shown}"
    );
    // A second push of the same snapshot does not duplicate it.
    app.push();
    assert_eq!(todo_list(&lib, "all").len(), 1, "deduplicated");

    // --- deps status on both sides
    let report = app.wt().args(["deps", "status"]).json();
    let out = outgoing(&report);
    assert_eq!(out.len(), 1, "outgoing:\n{}", pretty(&report));
    assert_eq!(str_field(&out[0], "snapshot_id"), dep_id);
    assert_eq!(str_field(&out[0]["dependency"], "tree"), "lib");
    assert_eq!(
        out[0]["todo"]["status"],
        "open",
        "dependency TODO status:\n{}",
        pretty(&report)
    );
    assert_eq!(out[0]["todo"]["number"].as_u64(), Some(1));
    let incoming = lib.wt().args(["deps", "status"]).json();
    let inc = todos(&incoming);
    assert_eq!(inc.len(), 1, "lib's incoming:\n{}", pretty(&incoming));
    assert_eq!(str_field(&inc[0], "title"), TITLE);
    let text = app.wt().args(["deps", "status"]).ok();
    assert!(
        text.stdout.contains("lib") && text.stdout.contains(TITLE),
        "human deps status\n{text}"
    );

    // --- lib completes it from a snapshot message
    lib.commit(
        &[("lib.txt", "lib v2 with parse()\n")],
        "lib: add parse()\n\nCompletes: #1\n",
    );
    let resolving = lib.head_id();
    lib.push();
    let done = poll_until(
        "TODO #1 to be marked done",
        Duration::from_secs(20),
        Duration::from_millis(200),
        || {
            let v = lib.wt().args(["todo", "show", "1"]).json();
            let t = todos(&v).into_iter().next().ok_or_else(|| pretty(&v))?;
            if t["status"] == "done" {
                Ok(t)
            } else {
                Err(pretty(&t))
            }
        },
    );
    assert_eq!(
        str_field(&done, "resolved_by"),
        resolving,
        "resolved by the completing snapshot"
    );
    assert!(todo_list(&lib, "active").is_empty(), "nothing active");
    assert_eq!(todo_list(&lib, "done").len(), 1);

    let report = app.wt().args(["deps", "status"]).json();
    let out = outgoing(&report);
    assert_eq!(
        out.first().map(|o| o["todo"]["status"].clone()),
        Some(Value::from("done")),
        "deps status follows the TODO:\n{}",
        pretty(&report)
    );
}
