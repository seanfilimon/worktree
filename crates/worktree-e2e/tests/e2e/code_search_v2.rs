//! Code search v2 (ENGINEERING_PLAN §15.9): `wt search code --symbol`
//! finds definitions (ranked exact > prefix > substring, `--kind`), branch
//! searches (`--branch`, `--all-branches` without duplicates), `wt admin
//! code-index` reports the persistent index, a server restart keeps it (no
//! rebuild: the status endpoint reports 0 builds and the same `built_at`)
//! and `wt admin code-index rebuild` rebuilds it.

use serde_json::Value;
use worktree_e2e::json::pretty;
use worktree_e2e::{require_e2e, TestEnv, User};

use crate::common::{publish_tree, TREE};

const CONFIG_RS: &str = "pub struct Config {\n    path: String,\n}\n\npub fn parse_config(s: &str) -> Config {\n    Config { path: s.into() }\n}\n\nimpl Config {\n    pub fn validate(&self) -> bool {\n        true\n    }\n}\n";
const UTIL_GO: &str = "package util\n\nfunc ParseConfig(s string) error { return nil }\n";
const APP_TS: &str =
    "export function parseConfigFile(p: string) {}\nexport const CONFIG_PATH = \"/etc/app\";\n";

#[track_caller]
fn wt_json(user: &User, args: &[&str]) -> Value {
    user.wt().args(args).json()
}

/// (name, kind, match) of every symbol, in result order.
fn symbols(v: &Value) -> Vec<(String, String, String)> {
    v["symbols"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| {
                    (
                        s["name"].as_str().unwrap_or_default().to_string(),
                        s["kind"].as_str().unwrap_or_default().to_string(),
                        s["match"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn sym(name: &str, kind: &str, m: &str) -> (String, String, String) {
    (name.into(), kind.into(), m.into())
}

/// (branch, path, line) of every code hit.
fn hits(v: &Value) -> Vec<(String, String, u64)> {
    v["hits"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|h| {
                    (
                        h["branch"].as_str().unwrap_or_default().to_string(),
                        h["path"].as_str().unwrap_or_default().to_string(),
                        h["line"].as_u64().unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn hit(branch: &str, path: &str, line: u64) -> (String, String, u64) {
    (branch.into(), path.into(), line)
}

/// The index status entry of alice/proj.
#[track_caller]
fn proj_status(status: &Value) -> Value {
    status["trees"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|t| t["tenant"] == "alice" && t["tree"] == TREE)
        })
        .cloned()
        .unwrap_or_else(|| panic!("alice/{TREE} is not indexed:\n{}", pretty(status)))
}

#[test]
fn code_search_v2() {
    require_e2e!();
    let mut env = TestEnv::with_server("code_search_v2");
    let alice = env.user("alice");
    let repo = publish_tree(
        &env,
        &alice,
        &[
            ("src/config.rs", CONFIG_RS),
            ("lib/util.go", UTIL_GO),
            ("web/app.ts", APP_TS),
        ],
    );
    let proj = format!("alice/{TREE}");

    // --- symbol search: ranking, kinds, containers
    let v = wt_json(
        &alice,
        &["search", "code", "config", "--symbol", "--tree", &proj],
    );
    assert_eq!(
        symbols(&v),
        vec![
            sym("Config", "struct", "exact"),
            sym("CONFIG_PATH", "const", "prefix"),
            sym("ParseConfig", "function", "substring"),
            sym("parse_config", "function", "substring"),
            sym("parseConfigFile", "function", "substring"),
        ],
        "{}",
        pretty(&v)
    );
    let first = &v["symbols"][0];
    assert_eq!(first["path"], "src/config.rs", "{}", pretty(first));
    assert_eq!(first["line"], 1, "{}", pretty(first));
    assert_eq!(first["language"], "rust", "{}", pretty(first));
    let v = wt_json(&alice, &["search", "code", "valid", "--symbol"]);
    assert_eq!(
        symbols(&v),
        vec![sym("validate", "method", "prefix")],
        "{}",
        pretty(&v)
    );
    assert_eq!(v["symbols"][0]["container"], "Config", "{}", pretty(&v));
    let v = wt_json(
        &alice,
        &[
            "search",
            "code",
            "config",
            "--symbol",
            "--kind",
            "const,struct",
        ],
    );
    assert_eq!(
        symbols(&v),
        vec![
            sym("Config", "struct", "exact"),
            sym("CONFIG_PATH", "const", "prefix")
        ],
        "{}",
        pretty(&v)
    );
    let out = alice
        .wt()
        .args(["search", "code", "parse_config", "--symbol"])
        .ok();
    assert!(
        out.stdout
            .contains("alice/proj:src/config.rs:5: function parse_config"),
        "{out}"
    );
    alice
        .wt()
        .args(["search", "code", "x", "--symbol", "--kind", "widget"])
        .fails();

    // --- branches
    repo.wt().args(["switch", "-c", "dev"]).ok();
    repo.commit(
        &[("src/dev_only.rs", "pub fn dev_only_needle() {}\n")],
        "dev work",
    );
    repo.wt().args(["push", "--branch", "dev"]).ok();
    repo.wt().args(["switch", "main"]).ok();

    let v = wt_json(&alice, &["search", "code", "dev_only_needle"]);
    assert!(
        hits(&v).is_empty(),
        "the default branch only:\n{}",
        pretty(&v)
    );
    let v = wt_json(
        &alice,
        &["search", "code", "dev_only_needle", "--branch", "dev"],
    );
    assert_eq!(
        hits(&v),
        vec![hit("dev", "src/dev_only.rs", 1)],
        "{}",
        pretty(&v)
    );
    let out = alice
        .wt()
        .args(["search", "code", "dev_only_needle", "--branch", "dev"])
        .ok();
    assert!(
        out.stdout
            .contains("alice/proj@dev:src/dev_only.rs:1:pub fn dev_only_needle() {}"),
        "{out}"
    );
    // All branches: the unchanged src/config.rs of dev is reported once.
    let v = wt_json(
        &alice,
        &["search", "code", "parse_config", "--all-branches"],
    );
    assert_eq!(
        hits(&v),
        vec![hit("main", "src/config.rs", 5)],
        "{}",
        pretty(&v)
    );
    let v = wt_json(&alice, &["search", "code", "needle", "--all-branches"]);
    assert_eq!(
        hits(&v),
        vec![hit("dev", "src/dev_only.rs", 1)],
        "{}",
        pretty(&v)
    );
    let v = wt_json(
        &alice,
        &["search", "code", "dev_only", "--symbol", "--all-branches"],
    );
    assert_eq!(
        symbols(&v),
        vec![sym("dev_only_needle", "function", "prefix")],
        "{}",
        pretty(&v)
    );
    assert_eq!(v["symbols"][0]["branch"], "dev", "{}", pretty(&v));
    alice
        .wt()
        .args(["search", "code", "abc", "--branch", "dev", "--all-branches"])
        .fails();

    // --- index status (server admins only)
    let admin = env.admin_user();
    admin.login();
    alice.wt().args(["admin", "code-index"]).fails();
    let before = wt_json(&admin, &["admin", "code-index"]);
    assert_eq!(before["persistent"], true, "{}", pretty(&before));
    assert!(
        before["builds"].as_u64().unwrap_or(0) >= 2,
        "{}",
        pretty(&before)
    );
    let tree_before = proj_status(&before);
    assert_eq!(tree_before["branch_count"], 2, "{}", pretty(&tree_before));
    assert_eq!(tree_before["files"], 3, "{}", pretty(&tree_before));
    assert!(
        tree_before["symbols"].as_u64().unwrap_or(0) >= 6,
        "{}",
        pretty(&tree_before)
    );
    let built_at = tree_before["built_at"].clone();
    assert!(
        built_at.as_str().is_some_and(|s| !s.is_empty()),
        "{}",
        pretty(&tree_before)
    );

    // --- restart: the index is loaded from disk, not rebuilt
    env.restart_server();
    let after = wt_json(&admin, &["admin", "code-index"]);
    assert_eq!(
        after["builds"],
        0,
        "nothing rebuilt at startup:\n{}",
        pretty(&after)
    );
    let tree_after = proj_status(&after);
    assert_eq!(tree_after["built_at"], built_at, "{}", pretty(&tree_after));
    assert_eq!(tree_after["branch_count"], 2, "{}", pretty(&tree_after));

    let v = wt_json(
        &alice,
        &["search", "code", "config", "--symbol", "--tree", &proj],
    );
    assert_eq!(symbols(&v).len(), 5, "{}", pretty(&v));
    let v = wt_json(
        &alice,
        &["search", "code", "dev_only_needle", "--branch", "dev"],
    );
    assert_eq!(
        hits(&v),
        vec![hit("dev", "src/dev_only.rs", 1)],
        "{}",
        pretty(&v)
    );
    let v = wt_json(&alice, &["search", "code", "parse_config"]);
    assert_eq!(
        hits(&v),
        vec![hit("main", "src/config.rs", 5)],
        "{}",
        pretty(&v)
    );

    let after = wt_json(&admin, &["admin", "code-index"]);
    assert_eq!(
        after["builds"],
        0,
        "searches after the restart must not rebuild:\n{}",
        pretty(&after)
    );
    assert!(
        after["disk_loads"].as_u64().unwrap_or(0) >= 1,
        "{}",
        pretty(&after)
    );
    assert_eq!(proj_status(&after)["built_at"], built_at);
    let out = admin.wt().args(["admin", "code-index"]).ok();
    assert!(out.stdout.contains("0 built"), "{out}");
    assert!(out.stdout.contains(&proj), "{out}");

    // --- a push after the restart is picked up incrementally
    repo.commit(&[("src/late.rs", "pub fn late_symbol() {}\n")], "late");
    repo.push();
    let v = wt_json(&alice, &["search", "code", "late_symbol", "--symbol"]);
    assert_eq!(
        symbols(&v),
        vec![sym("late_symbol", "function", "exact")],
        "{}",
        pretty(&v)
    );

    // --- rebuild
    let rebuilt = wt_json(&admin, &["admin", "code-index", "rebuild", &proj]);
    assert_eq!(rebuilt["rebuilt"], 1, "{}", pretty(&rebuilt));
    assert_eq!(
        rebuilt["trees"][0]["branch_count"],
        2,
        "{}",
        pretty(&rebuilt)
    );
    let status = wt_json(&admin, &["admin", "code-index"]);
    assert!(
        status["builds"].as_u64().unwrap_or(0) >= 3,
        "{}",
        pretty(&status)
    );
    assert_ne!(
        proj_status(&status)["built_at"],
        built_at,
        "{}",
        pretty(&status)
    );
    let v = wt_json(&alice, &["search", "code", "late_symbol", "--symbol"]);
    assert_eq!(symbols(&v).len(), 1, "{}", pretty(&v));
}
