//! Shared `.wt-tree/` files (ENGINEERING_PLAN §15.10): a nested tree's own
//! `.wt-tree/{config.toml,ignore,hooks/**,access/**}` are snapshotted,
//! pushed and cloned with it, while the identity marker
//! (`.wt-tree/local.toml`) stays local. A legacy marker (identity lines in
//! `config.toml`) is migrated on the next scan. The clone (where the tree is
//! the root tree) honours the shared config, ignore file and hook (which
//! keeps its executable bit), and a change made there comes back to the
//! nested tree with `wt pull`.

use serde_json::Value;
use worktree_e2e::json::{self, find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

#[cfg(windows)]
const HOOK: (&str, &str) = (
    ".wt-tree/hooks/post-snapshot.cmd",
    "@echo off\r\necho ran> hook-ran.txt\r\n",
);
#[cfg(not(windows))]
const HOOK: (&str, &str) = (
    ".wt-tree/hooks/post-snapshot",
    "#!/bin/sh\necho ran > hook-ran.txt\n",
);

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) {}

fn changed_paths(changes: &[(String, String)]) -> Vec<String> {
    changes.iter().map(|(p, _)| p.clone()).collect()
}

#[test]
fn wt_tree_sharing() {
    require_e2e!();
    let env = TestEnv::with_server("wt_tree_sharing");
    let alice = env.user("alice");
    alice.register();
    let repo = alice.init("proj");
    repo.commit(&[("README.md", "# proj\n")], "initial");
    repo.add_remote("origin", &env.url(), "alice");
    repo.push();

    // --- a nested tree with a legacy (pre-v0.7) marker
    repo.wt().args(["tree", "add", "web", "apps/web"]).ok();
    assert!(repo.exists("apps/web/.wt-tree/local.toml"));
    let trees = repo.wt().args(["tree", "list"]).json();
    let web_id = find_objects(&trees, &["name", "id"])
        .into_iter()
        .find(|t| t["name"] == "web")
        .map(|t| str_field(t, "id"))
        .unwrap_or_else(|| panic!("no web tree:\n{}", pretty(&trees)));
    repo.remove("apps/web/.wt-tree/local.toml");
    repo.write(
        "apps/web/.wt-tree/config.toml",
        format!("name = \"web\"\nid = \"{web_id}\"\n\n[hooks]\ntimeout_secs = 42\n"),
    );
    repo.write("apps/web/.wt-tree/ignore", "*.gen\nhook-ran.txt\n");
    repo.write(
        "apps/web/.wt-tree/access/policies.toml",
        "# tree-scope deny policies (none yet)\n",
    );
    repo.write(&format!("apps/web/{}", HOOK.0), HOOK.1);
    make_executable(&repo.path(&format!("apps/web/{}", HOOK.0)));
    repo.write("apps/web/index.html", "<h1>web</h1>\n");
    repo.write("apps/web/build.gen", "generated\n");

    let web = repo.nested("web");
    let changes = changed_paths(&web.changes());
    let mut want = vec![
        ".wt-tree/access/policies.toml".to_string(),
        ".wt-tree/config.toml".to_string(),
        HOOK.0.to_string(),
        ".wt-tree/ignore".to_string(),
        "index.html".to_string(),
    ];
    want.sort();
    assert_eq!(
        changes, want,
        "the shared files are tracked, not the identity"
    );
    // The scan migrated the identity out of the shared config.
    let config = repo.read("apps/web/.wt-tree/config.toml");
    assert!(
        !config.contains("name =") && !config.contains(&web_id),
        "{config}"
    );
    assert!(repo.read("apps/web/.wt-tree/local.toml").contains(&web_id));
    repo.wt().args(["tree", "validate", "web"]).ok();
    // The root tree never sees the nested tree's files.
    repo.assert_clean();

    web.snapshot("web with shared config");
    assert!(
        repo.exists("apps/web/hook-ran.txt"),
        "the post-snapshot hook ran"
    );
    web.assert_clean();
    web.push();

    // --- clone: the tree becomes the root tree of a new worktree
    let clone = alice.clone_tree("alice", "web", "web-clone");
    for rel in [
        ".wt-tree/config.toml",
        ".wt-tree/ignore",
        ".wt-tree/access/policies.toml",
        HOOK.0,
        "index.html",
    ] {
        assert!(clone.exists(rel), "{rel} not cloned");
    }
    assert!(
        !clone.exists(".wt-tree/local.toml"),
        "the identity marker is local"
    );
    assert!(!clone.exists("build.gen"), "ignored files are not tracked");
    assert_eq!(
        clone.read(".wt-tree/config.toml"),
        "[hooks]\ntimeout_secs = 42\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(clone.path(HOOK.0))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the hook keeps its executable bit");
    }
    let got = clone
        .wt()
        .args(["config", "get", "--show-origin", "hooks.timeout_secs"])
        .json();
    assert_eq!(got["value"], "42", "{}", pretty(&got));
    assert_eq!(got["origin"]["level"], "tree", "{}", pretty(&got));
    clone.assert_clean();
    clone.write("local.gen", "the shared ignore file applies\n");
    clone.assert_clean();

    // --- change the shared ignore file in the clone and push it back
    clone.write(".wt-tree/ignore", "*.gen\nhook-ran.txt\n*.tmp\n");
    clone.write("page.html", "<p>page</p>\n");
    clone.snapshot("ignore *.tmp too");
    assert!(
        clone.exists("hook-ran.txt"),
        "the cloned hook runs in the clone"
    );
    clone.assert_clean();
    clone.push();

    web.pull();
    assert_eq!(
        repo.read("apps/web/.wt-tree/ignore"),
        "*.gen\nhook-ran.txt\n*.tmp\n"
    );
    assert!(repo.exists("apps/web/page.html"));
    // The first push linked the tree to its server id; the local identity
    // file follows the registry and survives the pull.
    let trees = repo.wt().args(["tree", "list"]).json();
    let linked_id = find_objects(&trees, &["name", "id"])
        .into_iter()
        .find(|t| t["name"] == "web")
        .map(|t| str_field(t, "id"))
        .unwrap();
    let identity = repo.read("apps/web/.wt-tree/local.toml");
    assert!(
        identity.contains(&linked_id) && identity.contains("name = \"web\""),
        "pull keeps the local identity:\n{identity}"
    );
    repo.write("apps/web/scratch.tmp", "x\n");
    web.assert_clean();
    repo.assert_clean();

    // The server serves the shared files of the tree (never local.toml).
    let api = alice.api();
    let head = clone.head_id();
    let blob = api.get(&format!(
        "/api/v1/trees/alice/web/blob/{head}/.wt-tree/ignore"
    ));
    blob.assert_ok();
    assert_eq!(blob.text(), "*.gen\nhook-ran.txt\n*.tmp\n");
    let missing = api.get(&format!(
        "/api/v1/trees/alice/web/blob/{head}/.wt-tree/local.toml"
    ));
    assert!(!missing.is_success(), "{}", missing.text());
    let snap: Value = api
        .get(&format!("/api/v1/trees/alice/web/snapshots/{head}"))
        .ok_json();
    assert!(
        !json::all_strings(&snap)
            .iter()
            .any(|s| s.contains("local.toml")),
        "{}",
        pretty(&snap)
    );
}
