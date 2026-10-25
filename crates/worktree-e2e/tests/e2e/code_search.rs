//! Code search (ENGINEERING_PLAN §14.3): pushed files are found by `wt
//! search code` (literal, regex, case-insensitive, `--path` glob, one tree
//! via the global `--tree TENANT/TREE`, context lines), the index follows
//! new pushes to the default branch, and a private tree is invisible to an
//! outsider (CLI and anonymous REST) while a public one is searchable.

use std::time::Duration;

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{poll_until, require_e2e, Api, TestEnv, User};

use crate::common::{publish_tree, TREE};

const MAIN_RS: &str = "// entry point\nfn needle_function() -> u32 {\n    42\n}\n\nfn other() {}\n";
const GUIDE_MD: &str = "# Guide\n\nThe Needle is documented here.\n";
const UTIL_GO: &str = "package util\n\nfunc NeedleGo() int { return 7 }\n";

/// (tenant/tree, path, line) of every hit.
fn hits(v: &Value) -> Vec<(String, String, u64)> {
    let mut out: Vec<_> = find_objects(v, &["path", "line", "text"])
        .into_iter()
        .map(|h| {
            (
                format!(
                    "{}/{}",
                    h["tenant"].as_str().unwrap_or_default(),
                    h["tree"].as_str().unwrap_or_default()
                ),
                h["path"].as_str().unwrap_or_default().to_string(),
                h["line"].as_u64().unwrap_or_default(),
            )
        })
        .collect();
    out.sort();
    out
}

#[track_caller]
fn search(user: &User, args: &[&str]) -> Value {
    user.wt().args(["search", "code"]).args(args).json()
}

fn hit(tree: &str, path: &str, line: u64) -> (String, String, u64) {
    (tree.to_string(), path.to_string(), line)
}

#[test]
fn code_search() {
    require_e2e!();
    let env = TestEnv::with_server("code_search");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("src/main.rs", MAIN_RS),
            ("docs/guide.md", GUIDE_MD),
            ("lib/util.go", UTIL_GO),
        ],
    );
    let proj = format!("alice/{TREE}");

    // A second, public tree with one matching file.
    let p = alice.init("pub");
    p.commit(
        &[("open.txt", "a public needle_function mention\n")],
        "public",
    );
    p.add_remote("origin", &env.url(), "alice");
    p.push();
    alice
        .wt()
        .args(["trees", "update", "alice/pub", "--visibility", "public"])
        .ok();

    // --- literal (case-sensitive by default)
    let v = search(&alice, &["needle_function"]);
    assert_eq!(
        hits(&v),
        vec![
            hit("alice/proj", "src/main.rs", 2),
            hit("alice/pub", "open.txt", 1)
        ],
        "{}",
        pretty(&v)
    );
    let h = find_objects(&v, &["path", "line", "text"])
        .into_iter()
        .find(|h| h["path"] == "src/main.rs")
        .unwrap();
    assert_eq!(h["text"], "fn needle_function() -> u32 {", "{}", pretty(h));
    assert_eq!(h["branch"], "main", "{}", pretty(h));
    assert_eq!(
        h["snapshot_id"].as_str(),
        Some(a.head_id().as_str()),
        "hits come from the default-branch tip"
    );
    let v = search(&alice, &["NEEDLE_FUNCTION"]);
    assert!(
        hits(&v).is_empty(),
        "case-sensitive by default:\n{}",
        pretty(&v)
    );

    // grep-like text output
    let out = alice
        .wt()
        .args(["search", "code", "needle_function", "--tree", &proj])
        .ok();
    assert!(
        out.stdout
            .contains("alice/proj:src/main.rs:2:fn needle_function() -> u32 {"),
        "{out}"
    );
    assert!(
        !out.stdout.contains("alice/pub"),
        "--tree limits the search\n{out}"
    );

    // --- case-insensitive, path filter, one tree
    let v = search(&alice, &["needle", "-i", "--tree", &proj]);
    assert_eq!(
        hits(&v),
        vec![
            hit("alice/proj", "docs/guide.md", 3),
            hit("alice/proj", "lib/util.go", 3),
            hit("alice/proj", "src/main.rs", 2),
        ],
        "{}",
        pretty(&v)
    );
    let v = search(&alice, &["needle", "-i", "--path", "*.md"]);
    assert_eq!(
        hits(&v),
        vec![hit("alice/proj", "docs/guide.md", 3)],
        "{}",
        pretty(&v)
    );
    let v = search(&alice, &["needle", "-i", "--path", "src"]);
    assert_eq!(
        hits(&v),
        vec![hit("alice/proj", "src/main.rs", 2)],
        "a directory covers its files:\n{}",
        pretty(&v)
    );

    // --- regex
    let v = search(&alice, &["-E", r"^func \w+\(\)", "--tree", &proj]);
    assert_eq!(
        hits(&v),
        vec![hit("alice/proj", "lib/util.go", 3)],
        "{}",
        pretty(&v)
    );
    let v = search(
        &alice,
        &["-E", r"fn (needle_function|other)\(", "--tree", &proj],
    );
    assert_eq!(
        hits(&v),
        vec![
            hit("alice/proj", "src/main.rs", 2),
            hit("alice/proj", "src/main.rs", 6)
        ],
        "{}",
        pretty(&v)
    );
    alice
        .wt()
        .args(["search", "code", "-E", "(unclosed"])
        .fails();
    alice.wt().args(["search", "code", "ab"]).fails(); // literal queries need 3+ characters

    // --- context lines
    let v = search(
        &alice,
        &["needle_function", "--tree", &proj, "--context", "1"],
    );
    let h = find_objects(&v, &["path", "line", "text"])[0];
    assert_eq!(
        h["before"],
        serde_json::json!(["// entry point"]),
        "{}",
        pretty(h)
    );
    assert_eq!(h["after"], serde_json::json!(["    42"]), "{}", pretty(h));

    // --- the index follows pushes to the default branch
    a.commit(&[("src/late.rs", "fn needle_late() {}\n")], "late file");
    a.push();
    poll_until(
        "the pushed file to be searchable",
        Duration::from_secs(20),
        Duration::from_millis(200),
        || {
            let v = search(&alice, &["needle_late"]);
            if hits(&v) == vec![hit("alice/proj", "src/late.rs", 1)] {
                Ok(())
            } else {
                Err(pretty(&v))
            }
        },
    );

    // --- an outsider sees the public tree only
    let bob = env.user("bob");
    bob.register();
    let v = search(&bob, &["needle", "-i"]);
    assert_eq!(
        hits(&v),
        vec![hit("alice/pub", "open.txt", 1)],
        "bob must not see the private tree:\n{}",
        pretty(&v)
    );
    // Naming the private tree is refused like any other read of it.
    let out = bob
        .wt()
        .args(["search", "code", "needle_function", "--tree", &proj])
        .fails();
    assert!(
        out.mentions("permission denied") || out.mentions("forbidden") || out.mentions("not found"),
        "{out}"
    );
    assert!(!out.stdout.contains("src/main.rs"), "{out}");
    // --- a member with a path-deny policy does not see the denied files
    let carol = env.user("carol");
    carol.register();
    alice
        .wt()
        .args(["tenant", "add-member", "alice", "carol", "--role", "viewer"])
        .ok();
    let v = search(&carol, &["needle", "-i", "--tree", &proj]);
    assert_eq!(hits(&v).len(), 4, "{}", pretty(&v));
    let policy = env.path("deny-docs.json");
    worktree_e2e::fsutil::write(
        &policy,
        serde_json::json!({
            "name": "no-docs",
            "enabled": true,
            "statements": [{
                "sid": "no-docs",
                "effect": "deny",
                "actions": ["tree:read"],
                "resources": [format!("wt:path/alice/{TREE}/docs/**")],
                "principals": ["user:carol"],
                "conditions": {}
            }]
        })
        .to_string(),
    );
    alice
        .wt()
        .args(["policy", "create", "alice", "--file"])
        .arg(&policy)
        .ok();
    let v = search(&carol, &["needle", "-i", "--tree", &proj]);
    assert_eq!(
        hits(&v),
        vec![
            hit("alice/proj", "lib/util.go", 3),
            hit("alice/proj", "src/late.rs", 1),
            hit("alice/proj", "src/main.rs", 2),
        ],
        "denied paths are never searched:\n{}",
        pretty(&v)
    );
    let v = search(&alice, &["needle", "-i", "--tree", &proj]);
    assert_eq!(hits(&v).len(), 4, "the owner still sees everything");
    // Blame applies the same path checks (§14.4).
    let carol_api = carol.api();
    carol_api
        .get(&format!("/api/v1/trees/{proj}/blame/main/docs/guide.md"))
        .assert_error(403, "path_denied");
    carol_api
        .get(&format!("/api/v1/trees/{proj}/blame/main/src/main.rs"))
        .assert_ok();

    let anon = Api::new(&env.url())
        .get("/api/v1/search/code?q=needle&case=false")
        .ok_json();
    assert_eq!(
        hits(&anon),
        vec![hit("alice/pub", "open.txt", 1)],
        "anonymous callers search public trees only:\n{}",
        pretty(&anon)
    );
}
