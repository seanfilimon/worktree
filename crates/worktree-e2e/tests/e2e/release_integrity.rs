//! Signed tags and verified releases (ENGINEERING_PLAN §15.8): `wt tag
//! --sign` + `wt tag push` stores a verified tag, a `require_signed` tag
//! protection refuses unsigned tags (CLI and REST) and the tags a release
//! would create, `[signing] auto` signs tags pushed by `wt push`, a
//! release of the signed tag is verified, `wt release verify` checks
//! downloaded assets against the checksums and the tag signature (and
//! catches a tampered file), and the attached SBOM is valid SPDX 2.3 JSON.

use serde_json::{json, Value};
use worktree_e2e::fsutil;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, TREE};

/// The REST record of `tag` (`domain.Tag`), if the server has it.
fn server_tag(api: &worktree_e2e::Api, tree_ref: &str, tag: &str) -> Option<Value> {
    let tags = api.get(&format!("/api/v1/trees/{tree_ref}/tags")).ok_json();
    find_objects(&tags, &["name", "target"])
        .into_iter()
        .find(|t| t["name"] == tag)
        .cloned()
}

#[test]
fn release_integrity() {
    require_e2e!();
    let env = TestEnv::with_server("release_integrity");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("README.md", "# trust\n"),
            ("src/lib.rs", "pub fn f() {}\n"),
        ],
    );
    let api = alice.api();
    let tree_ref = format!("alice/{TREE}");

    // --- a registered signing key; v* tags must be signed
    alice.wt().args(["identity", "generate"]).ok();
    a.wt().args(["identity", "register"]).ok();
    a.wt()
        .args(["protect", "tag", "set", "v*", "--require-signed"])
        .ok();
    let rules = a.wt().args(["protect", "tag", "list"]).json();
    assert!(
        find_objects(&rules, &["pattern"])
            .iter()
            .any(|r| r["pattern"] == "v*" && r["require_signed"] == true),
        "the tag protection requires signatures:\n{}",
        pretty(&rules)
    );

    // --- a signed tag is pushed and verified
    let created = a
        .wt()
        .args(["tag", "v1.0.0", "-m", "First release", "--sign"])
        .json();
    assert!(
        !find_objects(&created, &["signature"]).is_empty() && !created["signature"].is_null(),
        "the local tag is signed:\n{}",
        pretty(&created)
    );
    let verify = a.wt().args(["tag", "verify", "v1.0.0"]).ok();
    assert!(
        verify.mentions("good signature"),
        "local verification\n{verify}"
    );
    a.wt().args(["tag", "push", "v1.0.0"]).ok();
    let stored =
        server_tag(&api, &tree_ref, "v1.0.0").unwrap_or_else(|| panic!("v1.0.0 on the server"));
    assert_eq!(
        stored["verified"],
        Value::Bool(true),
        "stored as verified:\n{}",
        pretty(&stored)
    );
    assert_eq!(str_field(&stored, "signer"), "alice");
    let remote = a.wt().args(["tag", "verify", "v1.0.0", "--remote"]).ok();
    assert!(
        remote.mentions("good signature"),
        "the server's copy verifies too\n{remote}"
    );

    // --- unsigned tags are refused under require_signed
    a.wt().args(["tag", "v1.0.1"]).ok();
    a.wt().args(["tag", "verify", "v1.0.1"]).fails();
    let refused = a.wt().args(["tag", "push", "v1.0.1"]).fails();
    assert!(
        refused.mentions("must be signed"),
        "the refusal names the requirement\n{refused}"
    );
    assert!(server_tag(&api, &tree_ref, "v1.0.1").is_none());
    api.post(
        &format!("/api/v1/trees/{tree_ref}/tags"),
        json!({"name": "v1.0.2", "target": "main"}),
    )
    .assert_error(403, "signature_required");
    // Tags outside the protection are unaffected.
    a.wt().args(["tag", "nightly"]).ok();
    a.wt().args(["tag", "push", "nightly"]).ok();
    assert_eq!(
        server_tag(&api, &tree_ref, "nightly").map(|t| t["verified"].clone()),
        Some(Value::Bool(false))
    );
    // A release cannot create an unsigned v* tag either.
    a.wt()
        .args(["release", "create", "v2.0.0", "--snapshot", "main"])
        .fails();

    // --- [signing] auto signs new tags; `wt push` sends them
    a.wt()
        .args(["config", "set", "signing.key", "default"])
        .ok();
    a.wt().args(["config", "set", "signing.auto", "true"]).ok();
    a.wt().args(["tag", "v1.1.0", "-m", "auto-signed"]).ok();
    a.push();
    assert_eq!(
        server_tag(&api, &tree_ref, "v1.1.0").map(|t| t["verified"].clone()),
        Some(Value::Bool(true)),
        "the auto-signed tag was pushed and verified"
    );

    // --- a release of the signed tag, with assets and an SBOM
    let rel = a
        .wt()
        .args(["release", "create", "v1.0.0", "--name", "One", "--sbom"])
        .json();
    assert_eq!(
        rel["verified"],
        Value::Bool(true),
        "release of a verified tag:\n{}",
        pretty(&rel)
    );
    assert_eq!(str_field(&rel["sbom"], "name"), "sbom.spdx.json");
    let binary: Vec<u8> = (0..50_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let bin = env.path("files").join("app.bin");
    fsutil::write(&bin, &binary);
    let notes = env.path("files").join("NOTES.txt");
    fsutil::write(&notes, "release notes\n");
    let up = a
        .wt()
        .args(["release", "upload", "v1.0.0"])
        .arg(&bin)
        .arg(&notes)
        .json();
    assert!(
        find_objects(&up, &["sha256"])
            .iter()
            .all(|x| x["sha256"].as_str().is_some_and(|s| s.len() == 64)),
        "assets carry sha256:\n{}",
        pretty(&up)
    );
    let sums = api
        .get(&format!(
            "/api/v1/trees/{tree_ref}/releases/v1.0.0/checksums"
        ))
        .text();
    assert_eq!(sums.lines().count(), 3, "checksums:\n{sums}");
    assert!(sums.contains("  app.bin\n") && sums.contains("  sbom.spdx.json\n"));
    let listed = a.wt().args(["release", "checksums", "v1.0.0"]).ok();
    assert!(listed.stdout.contains("  NOTES.txt"), "{listed}");

    // --- download and verify (alice, then a viewer with no local keys)
    let dl = env.path("dl-alice");
    a.wt()
        .args(["release", "download", "v1.0.0", "-o"])
        .arg(&dl)
        .ok();
    let report = a
        .wt()
        .args(["release", "verify", "v1.0.0", "-d"])
        .arg(&dl)
        .json();
    assert_eq!(
        report["signature"]["status"],
        "valid",
        "{}",
        pretty(&report)
    );
    assert!(
        find_objects(&report, &["status", "expected"])
            .iter()
            .all(|x| x["status"] == "ok"),
        "every asset matches:\n{}",
        pretty(&report)
    );

    let sbom: Value = serde_json::from_str(&fsutil::read_string(&dl.join("sbom.spdx.json")))
        .expect("the SBOM is valid JSON");
    assert_eq!(sbom["spdxVersion"], "SPDX-2.3");
    assert_eq!(sbom["SPDXID"], "SPDXRef-DOCUMENT");
    let files: Vec<String> = sbom["files"]
        .as_array()
        .map(|f| f.iter().map(|x| str_field(x, "fileName")).collect())
        .unwrap_or_default();
    assert!(
        files.contains(&"./README.md".to_string()) && files.contains(&"./src/lib.rs".to_string()),
        "SBOM files: {files:?}"
    );

    let bob = env.user("bob");
    bob.register();
    add_member(&api, "alice", "bob", "viewer");
    let bob_dl = env.path("dl-bob");
    bob.wt()
        .args(["release", "download", "v1.0.0", "--in", &tree_ref, "-o"])
        .arg(&bob_dl)
        .ok();
    let ok = bob
        .wt()
        .args(["release", "verify", "v1.0.0", "--in", &tree_ref, "-d"])
        .arg(&bob_dl)
        .ok();
    assert!(
        ok.mentions("good signature"),
        "verified against the signer's registered key\n{ok}"
    );

    // --- a tampered download fails verification
    fsutil::write(&bob_dl.join("app.bin"), b"tampered");
    let bad = bob
        .wt()
        .args(["release", "verify", "v1.0.0", "--in", &tree_ref, "-d"])
        .arg(&bob_dl)
        .fails();
    assert!(bad.mentions("mismatch"), "tampering detected\n{bad}");
}
