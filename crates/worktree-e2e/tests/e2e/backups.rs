//! Storage backends and backups (ENGINEERING_PLAN §15.2): `wt admin backup
//! create/list/download/delete` (server admins only), then the server is
//! stopped, its data directory wiped and `worktree-server restore` brings
//! it back (refusing a non-empty data directory without `--force`); the
//! restarted server serves the same branches, history, tags and releases,
//! a fresh clone has the same files, saved logins still work and pushes
//! continue.

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{bins, fsutil, require_e2e, Api, Cmd, TestEnv};

use crate::common::{publish_tree, TREE};

/// A compressible ~40 KiB text (stored WTZ1-compressed on the server).
fn big_text() -> String {
    (0..800)
        .map(|i| format!("row {i:04}: backups keep every object safe\n"))
        .collect()
}

/// What a restore must preserve, as seen through the REST API.
fn server_view(api: &Api) -> Value {
    let tree = format!("/api/v1/trees/alice/{TREE}");
    let mut branches = api.get(&format!("{tree}/branches")).ok_json();
    // Branch listings may carry volatile fields; keep names and tips.
    let mut tips: Vec<(String, String)> = find_objects(&branches, &["name", "tip"])
        .into_iter()
        .map(|b| (str_field(b, "name"), str_field(b, "tip")))
        .collect();
    tips.sort();
    branches = serde_json::json!(tips);
    let snapshots = api
        .get(&format!("{tree}/snapshots?branch=main&limit=50"))
        .ok_json();
    let mut ids: Vec<(String, String)> = find_objects(&snapshots, &["id", "message"])
        .into_iter()
        .map(|s| (str_field(s, "id"), str_field(s, "message")))
        .collect();
    ids.sort();
    let tags = api.get(&format!("{tree}/tags")).ok_json();
    let mut tag_targets: Vec<(String, String)> = find_objects(&tags, &["name", "target"])
        .into_iter()
        .map(|t| (str_field(t, "name"), str_field(t, "target")))
        .collect();
    tag_targets.sort();
    let releases = api.get(&format!("{tree}/releases")).ok_json();
    let mut rel: Vec<String> = find_objects(&releases, &["tag", "notes"])
        .into_iter()
        .map(|r| format!("{}:{}", str_field(r, "tag"), str_field(r, "notes")))
        .collect();
    rel.sort();
    serde_json::json!({ "branches": branches, "snapshots": ids, "tags": tag_targets, "releases": rel })
}

fn server_cmd(env: &TestEnv) -> Cmd {
    Cmd::new(bins::server_bin(), &env.logs).actor("worktree-server")
}

#[test]
fn backups() {
    require_e2e!();
    let mut env = TestEnv::with_server("backups");
    let alice = env.user("alice");
    let big = big_text();
    let a = publish_tree(
        &env,
        &alice,
        &[("README.md", "# backed up\n"), ("big.txt", big.as_str())],
    );
    a.commit(&[("src/lib.rs", "pub fn safe() {}\n")], "add lib");
    a.push();
    a.wt()
        .args([
            "release",
            "create",
            "v1.0.0",
            "--notes",
            "first release",
            "--snapshot",
            "main",
        ])
        .ok();
    let alice_api = alice.api();
    let before = server_view(&alice_api);
    assert!(
        before["snapshots"].as_array().is_some_and(|s| s.len() >= 2),
        "history before the backup:\n{}",
        pretty(&before)
    );

    let admin = env.admin_user();
    admin.login();

    // --- only server admins manage backups
    let denied = alice.wt().args(["admin", "backup", "list"]).fails();
    assert!(
        denied.mentions("permission") || denied.mentions("denied") || denied.mentions("forbidden"),
        "non-admin list refused with a permission error:\n{}",
        denied.combined()
    );

    // --- create, list
    let created = admin
        .wt()
        .args(["admin", "backup", "create", "--note", "e2e backup"])
        .json();
    let id = str_field(&created, "id");
    assert!(id.starts_with("bk-"), "backup id: {}", pretty(&created));
    assert_eq!(str_field(&created, "note"), "e2e backup");
    assert_eq!(str_field(&created, "created_by"), "admin");
    let size = created["size_bytes"].as_u64().unwrap_or(0);
    assert!(
        created["objects"].as_u64().unwrap_or(0) >= 4 && size > 0,
        "{}",
        pretty(&created)
    );
    let on_server = std::path::PathBuf::from(str_field(&created, "path"));
    assert!(
        on_server.starts_with(&env.server().data_dir),
        "archive lives under <data>/backups: {}",
        on_server.display()
    );

    let list = admin.wt().args(["admin", "backup", "list"]).json();
    assert_eq!(
        str_field(&list["store"], "kind"),
        "disk",
        "{}",
        pretty(&list)
    );
    assert!(list["supported"].as_bool().unwrap_or(false));
    let ids: Vec<String> = find_objects(&list, &["id", "size_bytes"])
        .into_iter()
        .map(|b| str_field(b, "id"))
        .collect();
    assert_eq!(ids, vec![id.clone()], "{}", pretty(&list));
    let human = admin.wt().args(["admin", "backup", "list"]).ok();
    assert!(
        human.mentions(&id) && human.mentions("e2e backup"),
        "{}",
        human.combined()
    );

    // --- download
    let file = env.path("downloads/full.wtbak");
    let dl = admin
        .wt()
        .args(["admin", "backup", "download", &id, "-o"])
        .arg(&file)
        .json();
    assert_eq!(dl["size_bytes"].as_u64(), Some(size), "{}", pretty(&dl));
    let bytes = fsutil::read(&file);
    assert_eq!(bytes.len() as u64, size);
    assert_eq!(&bytes[..4], &[0x28, 0xb5, 0x2f, 0xfd], "a zstd stream");

    // --- delete (a second backup), unknown ids are 404
    let extra = admin.wt().args(["admin", "backup", "create"]).json();
    let extra_id = str_field(&extra, "id");
    admin
        .wt()
        .args(["admin", "backup", "delete", &extra_id])
        .confirmed();
    let list = admin.wt().args(["admin", "backup", "list"]).json();
    assert!(
        !pretty(&list).contains(&extra_id),
        "deleted backup gone:\n{}",
        pretty(&list)
    );
    let missing = admin
        .wt()
        .args(["admin", "backup", "delete", &extra_id, "--yes"])
        .fails();
    assert!(
        missing.mentions("not found") || missing.mentions("not_found"),
        "{}",
        missing.combined()
    );

    // --- disaster: stop the server and wipe its data directory
    let data_dir = env.server().data_dir.clone();
    env.server_mut().stop();
    std::fs::remove_dir_all(&data_dir).expect("wipe the data directory");
    std::fs::create_dir_all(&data_dir).expect("recreate the data directory");

    // A non-empty data directory is refused without --force.
    fsutil::write(&data_dir.join("stray.txt"), "not empty");
    let refused = server_cmd(&env)
        .arg("restore")
        .arg("--data")
        .arg(&data_dir)
        .arg(&file)
        .code(1);
    assert!(refused.mentions("--force"), "{}", refused.combined());
    fsutil::remove(&data_dir.join("stray.txt"));

    let restored = server_cmd(&env)
        .arg("restore")
        .arg("--data")
        .arg(&data_dir)
        .arg(&file)
        .ok();
    assert!(
        restored.mentions("hashes verified") && restored.mentions(&id),
        "{}",
        restored.combined()
    );
    assert!(data_dir.join("meta.db").is_file());

    // --- the restored server serves the same trees and history
    env.server_mut().restart(&[]);
    let alice_api = alice.api();
    let after = server_view(&alice_api);
    assert_eq!(
        after,
        before,
        "restored server state differs:\nbefore {}\nafter {}",
        pretty(&before),
        pretty(&after)
    );
    // Saved CLI logins (tokens in the restored database) keep working.
    let fresh = alice.clone_tree("alice", TREE, "after-restore");
    assert_eq!(fresh.read("big.txt"), big);
    assert_eq!(fresh.read("src/lib.rs"), "pub fn safe() {}\n");
    assert_eq!(fresh.files(), a.files(), "a fresh clone has the same files");
    // New work continues on the restored server.
    a.commit(&[("NOTES.md", "after the restore\n")], "post-restore");
    a.push();

    // The restored database predates the backup's own index entry and the
    // wiped backups directory is gone, so the list is empty again.
    let list = admin.wt().args(["admin", "backup", "list"]).json();
    assert!(
        !pretty(&list).contains(&id),
        "no stale backup entries after a restore:\n{}",
        pretty(&list)
    );

    // --- restoring again over live data needs --force (server stopped)
    env.server_mut().stop();
    server_cmd(&env)
        .arg("restore")
        .arg("--data")
        .arg(&data_dir)
        .arg(&file)
        .code(1);
    server_cmd(&env)
        .arg("restore")
        .arg("--force")
        .arg("--data")
        .arg(&data_dir)
        .arg(&file)
        .ok();
    env.server_mut().restart(&[]);
    assert_eq!(
        server_view(&alice.api()),
        before,
        "--force restore rolls back to the backup"
    );
}
