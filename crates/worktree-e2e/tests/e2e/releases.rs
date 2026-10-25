//! Releases (ENGINEERING_PLAN §13.5): `wt release create` tags a snapshot
//! and records notes, `upload` stores assets, `download` returns them byte
//! for byte (also to a viewer, who may not create releases), `delete`
//! removes the release but keeps its tag.

use serde_json::Value;
use worktree_e2e::fsutil;
use worktree_e2e::json::{find_objects, names, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, TREE};

const NOTES: &str = "First stable release.\n\n- parse()\n- format()\n";

/// Release records (`{tag, snapshot_id, assets, ...}`).
fn releases(v: &Value) -> Vec<Value> {
    find_objects(v, &["tag", "snapshot_id"])
        .into_iter()
        .cloned()
        .collect()
}

#[test]
fn releases_flow() {
    require_e2e!();
    let env = TestEnv::with_server("releases");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# releases\n")]);
    let v1 = a.commit(&[("src/lib.txt", "v1\n")], "lib v1");
    a.push();
    let alice_api = alice.api();
    let tree_ref = format!("alice/{TREE}");

    assert!(
        releases(&a.wt().args(["release", "list"]).json()).is_empty(),
        "no releases yet"
    );

    // --- create v1.0.0 at the current main tip
    let created = a
        .wt()
        .args([
            "release",
            "create",
            "v1.0.0",
            "--name",
            "Version 1.0",
            "--notes",
            NOTES,
            "--snapshot",
            "main",
        ])
        .json();
    let rel = releases(&created)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("release record:\n{}", pretty(&created)));
    assert_eq!(str_field(&rel, "tag"), "v1.0.0");
    assert_eq!(str_field(&rel, "snapshot_id"), v1, "tagged at main's tip");
    assert_eq!(str_field(&rel, "name"), "Version 1.0");
    assert_eq!(str_field(&rel, "notes"), NOTES);
    assert!(
        !rel["draft"].as_bool().unwrap_or(false),
        "published, not a draft:\n{}",
        pretty(&rel)
    );
    let tags = alice_api
        .get(&format!("/api/v1/trees/{tree_ref}/tags"))
        .ok_json();
    assert!(
        names(&tags, &[]).contains(&"v1.0.0".to_string()),
        "the release created its tag:\n{}",
        pretty(&tags)
    );
    // Same tag twice: refused.
    a.wt()
        .args(["release", "create", "v1.0.0", "--notes", "again"])
        .fails();

    // --- upload a binary asset and a text asset
    let binary: Vec<u8> = (0..300_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let bin_path = env.path("files").join("app-x86_64.bin");
    fsutil::write(&bin_path, &binary);
    let sums_path = env.path("files").join("SHA256SUMS.txt");
    fsutil::write(&sums_path, "deadbeef  app-x86_64.bin\n");
    let up = a
        .wt()
        .args(["release", "upload", "v1.0.0"])
        .arg(&bin_path)
        .arg(&sums_path)
        .json();
    let uploaded = find_objects(&up, &["name", "size"]);
    assert_eq!(uploaded.len(), 2, "uploaded:\n{}", pretty(&up));
    assert!(uploaded
        .iter()
        .any(|x| x["name"] == "app-x86_64.bin" && x["size"].as_u64() == Some(binary.len() as u64)));

    let shown = a.wt().args(["release", "show", "v1.0.0"]).json();
    let assets = find_objects(&shown, &["name", "size", "content_type"]);
    let mut asset_names: Vec<String> = assets.iter().map(|x| str_field(x, "name")).collect();
    asset_names.sort();
    assert_eq!(
        asset_names,
        vec!["SHA256SUMS.txt".to_string(), "app-x86_64.bin".to_string()],
        "release show lists the assets:\n{}",
        pretty(&shown)
    );
    let text = a.wt().args(["release", "show", "v1.0.0"]).ok();
    assert!(
        text.stdout.contains("Version 1.0") && text.stdout.contains("parse()"),
        "human release show has the name and notes\n{text}"
    );

    // --- download one asset (alice) and all assets (a viewer)
    let out_dir = env.path("download-alice");
    a.wt()
        .args(["release", "download", "v1.0.0", "app-x86_64.bin", "-o"])
        .arg(&out_dir)
        .ok();
    assert!(
        fsutil::read(&out_dir.join("app-x86_64.bin")) == binary,
        "downloaded asset is byte-identical"
    );

    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "viewer");
    let bob_dir = env.path("download-bob");
    bob.wt()
        .args(["release", "download", "v1.0.0", "--in", &tree_ref, "-o"])
        .arg(&bob_dir)
        .ok();
    assert!(fsutil::read(&bob_dir.join("app-x86_64.bin")) == binary);
    assert_eq!(
        fsutil::read_string(&bob_dir.join("SHA256SUMS.txt")),
        "deadbeef  app-x86_64.bin\n"
    );
    // Viewers may read releases but not create or delete them.
    assert_eq!(
        releases(&bob.wt().args(["release", "list", &tree_ref]).json()).len(),
        1
    );
    bob.wt()
        .args([
            "release",
            "create",
            "v9.9.9",
            &tree_ref,
            "--snapshot",
            "main",
        ])
        .fails();
    bob.wt()
        .args(["release", "delete", "v1.0.0", &tree_ref, "--yes"])
        .fails();

    // --- delete: the release goes, the tag stays
    a.wt().args(["release", "delete", "v1.0.0"]).confirmed();
    assert!(
        releases(&a.wt().args(["release", "list"]).json()).is_empty(),
        "release deleted"
    );
    a.wt().args(["release", "show", "v1.0.0"]).fails();
    alice_api
        .get(&format!(
            "/api/v1/trees/{tree_ref}/releases/v1.0.0/assets/app-x86_64.bin"
        ))
        .assert_status(404);
    let tags = alice_api
        .get(&format!("/api/v1/trees/{tree_ref}/tags"))
        .ok_json();
    assert!(
        names(&tags, &[]).contains(&"v1.0.0".to_string()),
        "the tag is kept:\n{}",
        pretty(&tags)
    );
}
