//! Forks (ENGINEERING_PLAN §16.2, specs/tree/Forks.md): `wt fork` across
//! tenants with the license redistribution rule, cloning and pushing to a
//! fork, a merge request from the fork (`wt mr create --from`) merged
//! upstream, `wt fork sync`, and a fork that outlives its upstream.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

#[test]
fn forks() {
    require_e2e!();
    let env = TestEnv::with_server("forks");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("README.md", "# upstream\n"),
            ("secret/key.txt", "s3cr3t\n"),
        ],
    );
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    // tree:fork needs viewer on the upstream; bob forks into his own tenant.
    add_member(&alice_api, "alice", "bob", "viewer");
    let bob_api = bob.api();
    let up_path = format!("/api/v1/trees/alice/{TREE}");
    let fork_path = format!("/api/v1/trees/bob/{TREE}");

    // --- license rule: proprietary files may not leave alice's tenant
    alice_api
        .put(
            &format!("{up_path}/licenses"),
            json!({"path_glob": "secret/**", "spdx": "LicenseRef-Proprietary"}),
        )
        .ok_json();
    let out = bob.wt().args(["fork", &format!("alice/{TREE}")]).fails();
    assert!(
        out.mentions("license") && out.mentions("secret/key.txt"),
        "fork refused by the license rule\n{out}"
    );
    bob_api.get(&fork_path).assert_error(404, "not_found");
    // A redistribute grant for bob's tenant lifts it.
    alice_api
        .put(
            &format!("{up_path}/licenses"),
            json!({"path_glob": "secret/*", "spdx": "LicenseRef-Proprietary", "grant": "redistribute", "grantee": "bob"}),
        )
        .ok_json();

    // --- fork
    let out = bob.wt().args(["fork", &format!("alice/{TREE}")]).ok();
    assert!(out.mentions("Forked"), "{out}");
    let fork = bob_api.get(&fork_path).ok_json();
    let up = alice_api.get(&up_path).ok_json();
    let up_id = str_field(&up["tree"], "id");
    assert_eq!(
        str_field(&fork["tree"], "fork_of"),
        up_id,
        "fork_of names the upstream\n{}",
        pretty(&fork)
    );
    let forks = bob
        .wt()
        .args(["--json", "fork", "list", &format!("alice/{TREE}")])
        .ok()
        .json();
    let listed = find_objects(&forks, &["tenant", "name"]);
    assert_eq!(listed.len(), 1, "forks:\n{}", pretty(&forks));
    assert_eq!(listed[0]["tenant"], json!("bob"));
    let upstream = bob
        .wt()
        .args(["--json", "fork", "upstream", &format!("bob/{TREE}")])
        .ok()
        .json();
    assert_eq!(upstream["fork"], json!(true));
    assert_eq!(upstream["tenant"], json!("alice"));
    assert_eq!(upstream["upstream_missing"], json!(false));
    assert!(
        find_objects(&upstream, &["name", "status"])
            .iter()
            .any(|b| b["name"] == json!("main") && b["status"] == json!("up_to_date")),
        "main is up to date:\n{}",
        pretty(&upstream)
    );

    // --- bob clones his fork, works on a branch and pushes it
    let b = bob.clone_tree("bob", TREE, "fork");
    assert_eq!(b.read("README.md"), "# upstream\n");
    b.wt().args(["switch", "-c", "feat"]).ok();
    let feat_tip = b.commit(&[("fork.txt", "from the fork\n")], "fork: add file");
    b.wt().args(["push", "--branch", "feat"]).ok();
    assert_eq!(
        server_tip(&bob_api, "bob", TREE, "feat"),
        Some(feat_tip.clone())
    );

    // --- merge request upstream from the fork (bob is only a viewer there)
    let out = bob
        .wt()
        .args([
            "--json",
            "mr",
            "create",
            &format!("alice/{TREE}"),
            "--from",
            &format!("bob/{TREE}:feat"),
            "--target",
            "main",
            "--title",
            "From my fork",
        ])
        .ok()
        .json();
    let fork_id = str_field(&fork["tree"], "id");
    assert_eq!(
        str_field(&out, "source_tree_id"),
        fork_id,
        "{}",
        pretty(&out)
    );
    let number = out["number"].as_u64().expect("number");
    let mr_path = format!("{up_path}/merge-requests/{number}");
    let src = alice_api.get(&format!("{mr_path}/source")).ok_json();
    assert_eq!(src["cross_tree"], json!(true));
    assert_eq!(str_field(&src, "tip"), feat_tip);
    let target_tip = str_field(&src, "target_tip");
    // `wt mr show --diff` reads the fork's changes.
    let shown = alice
        .wt()
        .args([
            "--json",
            "mr",
            "show",
            &number.to_string(),
            &format!("alice/{TREE}"),
            "--diff",
        ])
        .ok()
        .json();
    assert!(
        pretty(&shown).contains("fork.txt"),
        "the diff shows the fork's file:\n{}",
        pretty(&shown)
    );
    let merged = alice_api.post_empty(&format!("{mr_path}/merge")).ok_json();
    assert_eq!(str_field(&merged, "state"), "merged", "{}", pretty(&merged));
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(target_tip.clone()),
        "upstream main fast-forwarded to the fork's work"
    );
    a.pull();
    assert_eq!(a.read("fork.txt"), "from the fork\n");

    // --- upstream moves on; bob syncs his fork and pulls
    a.commit(&[("upstream.txt", "after the merge\n")], "upstream: more");
    a.push();
    let behind = bob_api.get(&format!("{fork_path}/fork")).ok_json();
    assert!(
        find_objects(&behind, &["name", "status"])
            .iter()
            .any(|b| b["name"] == json!("main") && b["status"] == json!("behind")),
        "main is behind:\n{}",
        pretty(&behind)
    );
    let synced = bob
        .wt()
        .args(["--json", "fork", "sync", &format!("bob/{TREE}")])
        .ok()
        .json();
    assert_eq!(synced["updated"], json!(1), "{}", pretty(&synced));
    b.wt().args(["switch", "main"]).ok();
    b.pull();
    assert_eq!(b.read("upstream.txt"), "after the merge\n");
    assert_eq!(b.read("fork.txt"), "from the fork\n");
    // The merged work keeps its fork ids: main now descends from feat.
    let log: Vec<String> = b
        .log(&[])
        .iter()
        .filter_map(|s| s.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    assert!(
        log.contains(&feat_tip),
        "fork main history contains feat:\n{log:?}"
    );
    b.assert_clean();

    // --- deleting the upstream leaves the fork intact
    alice_api.delete(&up_path).assert_status(204);
    let gone = bob_api.get(&format!("{fork_path}/fork")).ok_json();
    assert_eq!(gone["upstream_missing"], json!(true), "{}", pretty(&gone));
    assert_eq!(
        str_field(&bob_api.get(&fork_path).ok_json()["tree"], "fork_of"),
        up_id
    );
    let out = bob
        .wt()
        .args(["fork", "sync", &format!("bob/{TREE}")])
        .fails();
    assert!(out.mentions("upstream"), "{out}");
    let c = bob.clone_tree("bob", TREE, "fork2");
    assert_eq!(c.read("upstream.txt"), "after the merge\n");
    c.assert_clean();
}
