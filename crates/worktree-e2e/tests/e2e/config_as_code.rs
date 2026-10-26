//! Config-as-code (ENGINEERING_PLAN §16.3): a tree's
//! `.wt-tree/{config,branches,protections}.toml` and `.wt-tree/access/`
//! configure the server when pushed to the default branch.
//!
//! `wt trees config validate` and `wt status` report located errors
//! locally, `wt push` refuses an invalid configuration before contacting
//! the server, the server rejects what only it can check (an owner that
//! does not exist; access changes by a developer without iam:admin), a
//! valid push applies branch descriptions/owners and protections (an API
//! rule with the same pattern is left alone) and the tree's deny policy,
//! `GET {tree}/config` reports the applied state and the last rejection,
//! and `wt branch describe|owner` edit branches that the file does not
//! manage.

use std::time::Duration;

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{poll_until, require_e2e, Api, TestEnv};

use crate::common::add_member;

const BRANCHES_BAD: &str = "[[branch]]\nname = \"main\"\nowner = \"group:x\"\n";

const BRANCHES: &str = r#"# Branch metadata (config-as-code)
[[branch]]
name = "main"
description = "Production web app"
owner = "user:bob"

[[branch]]
name = "release/1.0"
owner = "user:alice"
"#;

const PROTECTIONS: &str = r#"[[protection]]
pattern = "release/*"
no_delete = true
required_approvals = 1

[[protection]]
pattern = "hotfix/*"
no_delete = true
"#;

const POLICIES: &str = r#"[[policy]]
name = "no-bob-main"
[[policy.statement]]
effect = "deny"
principals = ["user:bob"]
actions = ["sync:push"]
resources = ["wt:branch/alice/web/main"]
"#;

fn config(api: &Api) -> Value {
    api.get("/api/v1/trees/alice/web/config").ok_json()
}

/// Wait until the configuration of `snapshot` is applied.
fn wait_applied(api: &Api, snapshot: &str) -> Value {
    poll_until(
        "the configuration to be applied",
        Duration::from_secs(20),
        Duration::from_millis(100),
        || {
            let v = config(api);
            if v["applied"]["snapshot_id"] == snapshot {
                Ok(v)
            } else {
                Err(pretty(&v))
            }
        },
    )
}

fn branch(api: &Api, name: &str) -> Value {
    let v = api.get("/api/v1/trees/alice/web/branches").ok_json();
    find_objects(&v, &["name", "tip"])
        .into_iter()
        .find(|b| b["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no branch {name}:\n{}", pretty(&v)))
}

fn codes(diags: &Value) -> Vec<String> {
    diags
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| d["code"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn config_as_code() {
    require_e2e!();
    let env = TestEnv::with_server("config_as_code");
    let alice = env.user("alice");
    alice.register();
    let bob = env.user("bob");
    bob.register();
    let repo = alice.init("proj");
    repo.commit(&[("README.md", "# proj\n")], "initial");
    repo.add_remote("origin", &env.url(), "alice");
    repo.push();
    repo.wt().args(["tree", "add", "web", "apps/web"]).ok();
    let web = repo.nested("web");
    repo.write("apps/web/index.html", "<h1>web</h1>\n");
    web.snapshot("web");
    web.push();
    let api = alice.api();
    add_member(&api, "alice", "bob", "developer");
    let st = config(&api);
    assert!(
        st["applied"].is_null() && st["last_attempt"].is_null(),
        "{}",
        pretty(&st)
    );

    // --- local validation: wt trees config validate, wt status, wt push
    repo.write("apps/web/.wt-tree/branches.toml", BRANCHES_BAD);
    let out = web.wt().args(["trees", "config", "validate"]).fails();
    assert!(
        out.mentions(".wt-tree/branches.toml:3:1") && out.mentions("E3008"),
        "located error\n{out}"
    );
    let status = web.status_json();
    assert_eq!(
        codes(&status["config_problems"]),
        vec!["E3008"],
        "{}",
        pretty(&status)
    );
    web.snapshot("bad owner syntax");
    let tip_before = branch(&api, "main")["tip"].clone();
    let out = web.wt().arg("push").fails();
    assert!(
        out.mentions("E3008") && out.mentions("would reject"),
        "pre-push check\n{out}"
    );
    assert_eq!(branch(&api, "main")["tip"], tip_before);

    // --- the server rejects an owner that does not exist (located)
    repo.write(
        "apps/web/.wt-tree/branches.toml",
        "[[branch]]\nname = \"main\"\nowner = \"user:ghost\"\n",
    );
    web.wt().args(["trees", "config", "validate"]).ok();
    web.snapshot("unknown owner");
    let out = web.wt().arg("push").fails();
    assert!(
        out.mentions(".wt-tree/branches.toml:3:1: E3008 owner user:ghost does not exist"),
        "server rejection\n{out}"
    );
    assert_eq!(branch(&api, "main")["tip"], tip_before);
    let st = config(&api);
    assert_eq!(st["last_attempt"]["accepted"], false, "{}", pretty(&st));
    assert_eq!(codes(&st["last_attempt"]["errors"]), vec!["E3008"]);
    let shown = web.wt().args(["trees", "config", "show", "alice/web"]).ok();
    assert!(
        shown.mentions("rejected") && shown.mentions("user:ghost"),
        "{shown}"
    );

    // --- a valid configuration is applied after the push
    // An API rule for hotfix/* exists before the file declares it.
    api.put(
        "/api/v1/trees/alice/web/protections",
        json!({"pattern": "hotfix/*", "required_approvals": 3}),
    )
    .assert_ok();
    repo.write("apps/web/.wt-tree/branches.toml", BRANCHES);
    repo.write("apps/web/.wt-tree/protections.toml", PROTECTIONS);
    repo.write(
        "apps/web/.wt-tree/config.toml",
        "[hooks]\ntimeout_secs = 30\n\n[large_files]\nthreshold_bytes = 1\n",
    );
    let valid = web.wt().args(["trees", "config", "validate"]).json();
    assert_eq!(valid["valid"], true, "{}", pretty(&valid));
    assert_eq!(codes(&valid["diagnostics"]), vec!["W3103"]);
    web.snapshot("config as code");
    let head = web.head_id();
    web.push();
    let st = wait_applied(&api, &head);
    let applied = &st["applied"];
    assert_eq!(applied["applied_by"], "alice", "{}", pretty(&st));
    assert_eq!(applied["managed_patterns"], json!(["release/*"]));
    assert!(
        codes(&applied["warnings"]).contains(&"W3101".to_string())
            && codes(&applied["warnings"]).contains(&"W3102".to_string()),
        "API-managed pattern and missing release/1.0 are reported:\n{}",
        pretty(&st)
    );
    let main = branch(&api, "main");
    assert_eq!(main["description"], "Production web app");
    assert_eq!(main["owner"], "user:bob");
    let prots = api.get("/api/v1/trees/alice/web/protections").ok_json();
    let by_pattern = |p: &str| {
        find_objects(&prots, &["pattern"])
            .into_iter()
            .find(|r| r["pattern"] == p)
            .cloned()
            .unwrap_or_else(|| panic!("no protection {p}:\n{}", pretty(&prots)))
    };
    assert_eq!(by_pattern("release/*")["no_delete"], true);
    assert_eq!(
        by_pattern("hotfix/*")["required_approvals"],
        3,
        "the API rule is not clobbered"
    );
    let shown = web
        .wt()
        .args(["trees", "config", "show", "alice/web"])
        .json();
    assert_eq!(shown["applied"]["snapshot_id"], head.as_str());
    assert_eq!(shown["last_attempt"]["accepted"], true);

    // A branch created later gets its declared metadata.
    web.wt()
        .args(["trees", "branch-create", "alice/web", "release/1.0"])
        .ok();
    poll_until(
        "release/1.0 to get its owner",
        Duration::from_secs(20),
        Duration::from_millis(100),
        || {
            let b = branch(&api, "release/1.0");
            if b["owner"] == "user:alice" {
                Ok(())
            } else {
                Err(pretty(&b))
            }
        },
    );

    // --- wt branch describe / owner
    let out = web
        .wt()
        .args(["branch", "describe", "main", "manual"])
        .fails();
    assert!(
        out.mentions("branches.toml"),
        "main is config-managed\n{out}"
    );
    web.wt()
        .args(["trees", "branch-create", "alice/web", "feature/login"])
        .ok();
    web.wt()
        .args(["branch", "describe", "feature/login", "Login page"])
        .ok();
    let owned = web
        .wt()
        .args(["branch", "owner", "feature/login", "user:bob"])
        .json();
    assert_eq!(owned["owner"], "user:bob", "{}", pretty(&owned));
    assert_eq!(owned["description"], "Login page");
    web.wt()
        .args(["branch", "owner", "feature/login", "nobody"])
        .fails();
    let listed = web.wt().args(["trees", "branches", "alice/web"]).ok();
    assert!(
        listed.mentions("Login page") && listed.mentions("user:bob"),
        "{listed}"
    );

    // --- access/: needs iam:admin; applied for the tenant admin
    let b = bob.clone_tree("alice", "web", "web-bob");
    assert!(b.exists(".wt-tree/branches.toml"));
    b.write(".wt-tree/access/policies.toml", POLICIES);
    b.snapshot("bob tries to add a policy");
    let out = b.wt().arg("push").fails();
    assert!(
        out.mentions("E3010") && out.mentions("iam:admin"),
        "a developer may not change access/\n{out}"
    );
    b.remove(".wt-tree/access/policies.toml");
    b.snapshot("revert");
    b.write("bob.txt", "bob was here\n");
    b.snapshot("bob: content");
    b.push();

    repo.wt().arg("pull").ok();
    web.pull();
    repo.write("apps/web/.wt-tree/access/policies.toml", POLICIES);
    let checked = web
        .wt()
        .args([
            "trees",
            "config",
            "validate",
            "--online",
            "--remote-tree",
            "alice/web",
        ])
        .json();
    assert_eq!(checked["valid"], true, "{}", pretty(&checked));
    assert!(
        checked["access_changes"]
            .to_string()
            .contains("no-bob-main"),
        "dry-run plan:\n{}",
        pretty(&checked)
    );
    web.snapshot("deny bob on main");
    let head = web.head_id();
    web.push();
    let st = wait_applied(&api, &head);
    assert!(
        st["applied"]["access_hash"]
            .as_str()
            .is_some_and(|h| !h.is_empty()),
        "{}",
        pretty(&st)
    );
    let policies = alice.wt().args(["policy", "list", "alice"]).json();
    assert!(
        policies.to_string().contains("web/no-bob-main"),
        "{}",
        pretty(&policies)
    );
    b.pull();
    b.commit(&[("bob.txt", "denied now\n")], "bob: after the policy");
    let out = b.wt().arg("push").fails();
    assert!(out.mentions("denied"), "the tree policy applies\n{out}");
}
