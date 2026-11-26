//! history-v2 (ENGINEERING_PLAN §16.10): the local operation log (`wt undo`,
//! `wt undo --list`, `wt redo`), `wt push --force` with lease semantics and
//! its permissions (`branch:force_push`, `no_force_push`, require_linear
//! needing `--bypass-protection`), the server reflog, and `wt tag delete
//! --remote` with tag protections. Reflog restore (specs/tree/Undo.md
//! §2.2, §4.1): `wt undo` of a push, `wt reflog --remote` and
//! `POST {tree}/reflog/restore`.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

/// Operations of `wt --json undo --list` (`{id, op, description, ...}`).
fn undoable(repo: &worktree_e2e::Repo) -> Vec<Value> {
    let v = repo.wt().args(["undo", "--list"]).json();
    find_objects(&v, &["id", "op", "description"])
        .into_iter()
        .cloned()
        .collect()
}

#[test]
fn force_push_undo() {
    require_e2e!();
    let env = TestEnv::with_server("force_push_undo");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("app.txt", "v1\n")]);
    let alice_api = alice.api();
    let c1 = a.head_id();
    let tree_path = format!("/api/v1/trees/alice/{TREE}");

    // --- local undo / redo of a snapshot
    let c2 = a.commit(&[("app.txt", "v2\n"), ("new.txt", "new\n")], "second");
    let ops = undoable(&a);
    assert!(!ops.is_empty(), "undo list:\n{ops:#?}");
    assert_eq!(str_field(&ops[0], "op"), "snapshot");
    a.wt().arg("undo").ok();
    assert_eq!(a.head_id(), c1, "undo moves main back");
    assert_eq!(a.read("app.txt"), "v1\n");
    assert!(!a.exists("new.txt"));
    a.assert_clean();
    a.wt().arg("redo").ok();
    assert_eq!(a.head_id(), c2, "redo restores the snapshot");
    assert_eq!(a.read("new.txt"), "new\n");
    // Dirty working trees are refused unless --force (which discards).
    a.write("app.txt", "dirty\n");
    let out = a.wt().arg("undo").fails();
    assert!(out.mentions("uncommitted"), "{out}");
    a.wt().args(["undo", "--force"]).ok();
    assert_eq!(a.head_id(), c1);
    assert_eq!(a.read("app.txt"), "v1\n");
    a.wt().arg("redo").ok();
    assert_eq!(a.head_id(), c2);

    // --- stash push is undone with its changes given back
    a.write("app.txt", "wip\n");
    a.wt().args(["stash", "push"]).ok();
    assert_eq!(a.read("app.txt"), "v2\n");
    a.wt().arg("undo").ok();
    assert_eq!(
        a.read("app.txt"),
        "wip\n",
        "undoing the stash push restores the changes"
    );
    a.write("app.txt", "v2\n");
    a.assert_clean();

    // --- publish c2; `wt undo` undoes the push on the server
    a.push();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(c2.clone())
    );
    let out = a.wt().arg("undo").fails(); // no terminal: needs --yes
    assert!(out.mentions("--yes"), "{out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(c2.clone())
    );
    let out = a.wt().args(["undo", "--yes"]).ok();
    assert!(out.mentions("Undid push"), "{out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(c1.clone())
    );
    assert_eq!(a.head_id(), c2, "the local branch keeps its snapshots");
    let remote = a.wt().args(["reflog", "--remote", "-b", "main"]).json();
    let newest = find_objects(&remote, &["kind", "old_tip", "new_tip"])[0].clone();
    assert_eq!(str_field(&newest, "kind"), "restore", "{}", pretty(&remote));
    assert_eq!(str_field(&newest, "old_tip"), c2);
    assert_eq!(str_field(&newest, "new_tip"), c1);
    let out = a.wt().arg("redo").fails();
    assert!(out.mentions("wt push"), "{out}");

    // --- rewrite main: undo c2 locally, bring c2 back on the server from its
    // reflog, then a plain push is refused and --force works
    a.wt().arg("undo").ok();
    assert_eq!(a.head_id(), c1);
    let restored = alice_api
        .post(
            &format!("{tree_path}/reflog/restore"),
            json!({"branch": "main", "tip": c2, "expected_tip": c1}),
        )
        .ok_json();
    assert_eq!(restored["forced"], false, "{}", pretty(&restored));
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(c2.clone())
    );
    let c3 = a.commit(&[("app.txt", "v3 rewritten\n")], "rewritten");
    let out = a.wt().arg("push").fails();
    assert!(out.mentions("wt pull"), "{out}");
    a.wt().arg("fetch").ok(); // the lease needs the restored tip
    let out = a.wt().args(["push", "--force"]).ok();
    assert!(out.mentions("forced update"), "{out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(c3.clone())
    );

    // The server reflog keeps the rewound tip.
    let reflog = alice_api
        .get(&format!("{tree_path}/reflog?branch=main"))
        .ok_json();
    let entries = find_objects(&reflog, &["kind", "old_tip", "new_tip"]);
    let forced = entries
        .iter()
        .find(|e| str_field(e, "kind") == "force_push")
        .unwrap_or_else(|| panic!("no force push in reflog:\n{}", pretty(&reflog)));
    assert_eq!(str_field(forced, "old_tip"), c2);
    assert_eq!(str_field(forced, "new_tip"), c3);
    assert_eq!(str_field(forced, "actor"), "alice");

    // --- lease: bob (admin) cannot overwrite a tip he has not fetched
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "admin");
    let dave = env.user("dave");
    dave.register();
    add_member(&alice_api, "alice", "dave", "developer");
    let b = bob.clone_tree("alice", TREE, "bob-proj");
    let d = dave.clone_tree("alice", TREE, "dave-proj");
    let c4 = a.commit(&[("app.txt", "v4\n")], "fourth");
    a.push();
    b.commit(&[("bob.txt", "bob\n")], "bob diverges");
    let out = b.wt().args(["push", "--force"]).fails();
    assert!(out.mentions("refusing to force-push"), "{out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(c4.clone())
    );
    b.wt().arg("fetch").ok();
    b.wt().args(["push", "--force"]).ok();
    let b1 = b.head_id();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(b1.clone())
    );

    // Developers lack branch:force_push.
    d.wt().arg("fetch").ok();
    d.commit(&[("dave.txt", "dave\n")], "dave diverges");
    let out = d.wt().args(["push", "--force"]).fails();
    assert!(
        out.mentions("force_push") || out.mentions("access denied"),
        "{out}"
    );

    // --- no_force_push is absolute; require_linear needs the owner bypass
    a.wt()
        .args(["protect", "set", "main", "--no-force-push"])
        .ok();
    a.wt().arg("fetch").ok();
    a.commit(&[("app.txt", "v5\n")], "alice diverges");
    let out = a
        .wt()
        .args(["push", "--force", "--bypass-protection"])
        .fails();
    assert!(out.mentions("no_force_push"), "{out}");
    a.wt()
        .args(["protect", "set", "main", "--require-linear"])
        .ok();
    let out = a.wt().args(["push", "--force"]).fails();
    assert!(out.mentions("bypass"), "{out}");
    // Undoing bob's force push rewinds main: protection applies to it too.
    let out = b.wt().args(["undo", "--yes"]).fails();
    assert!(out.mentions("protected"), "{out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(b1.clone())
    );
    a.wt().args(["push", "--force", "--bypass-protection"]).ok();
    let alice_tip = a.head_id();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(alice_tip.clone())
    );
    // Bob's push is no longer the server tip: undoing it is refused.
    let out = b.wt().args(["undo", "--yes"]).fails();
    assert!(out.mentions("has moved since your push"), "{out}");
    b.commit(&[("bob2.txt", "x\n")], "bob again");
    b.wt().arg("fetch").ok();
    let out = b
        .wt()
        .args(["push", "--force", "--bypass-protection"])
        .fails();
    assert!(out.mentions("protected"), "admins cannot bypass: {out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(alice_tip)
    );
    let audit = alice_api
        .get("/api/v1/audit?tenant=alice&action=branch.protection_bypassed")
        .ok_json();
    assert!(
        find_objects(&audit, &["action"])
            .iter()
            .any(|e| str_field(e, "action") == "branch.protection_bypassed"),
        "bypass audited:\n{}",
        pretty(&audit)
    );

    // --- remote tag deletion with tag protections
    a.wt().args(["tag", "v1"]).ok();
    a.wt().args(["tag", "rel1"]).ok();
    a.wt().args(["tag", "push", "v1"]).ok();
    a.wt().args(["tag", "push", "rel1"]).ok();
    a.wt()
        .args(["protect", "tag", "set", "rel*", "--no-delete"])
        .ok();
    let tags = |api: &worktree_e2e::Api| -> Vec<String> {
        let v = api.get(&format!("{tree_path}/tags")).ok_json();
        find_objects(&v, &["name", "target"])
            .iter()
            .map(|t| str_field(t, "name"))
            .collect()
    };
    let out = a.wt().args(["tag", "delete", "rel1", "--remote"]).fails();
    assert!(out.mentions("cannot be deleted"), "{out}");
    let out = a.wt().args(["tag", "delete", "v1", "--remote"]).ok();
    assert!(out.mentions("locally and on origin"), "{out}");
    assert_eq!(tags(&alice_api), vec!["rel1".to_string()]);
    let local = a.wt().args(["tag", "list"]).json();
    assert!(
        find_objects(&local, &["name", "target"])
            .iter()
            .all(|t| str_field(t, "name") != "v1"),
        "local tag deleted too:\n{}",
        pretty(&local)
    );
    // The next push does not re-create it.
    a.wt().arg("push").ok();
    assert_eq!(tags(&alice_api), vec!["rel1".to_string()]);
}

/// `wt undo` of a push: refused for a push that created the branch, for
/// developers (no `branch:force_push`) and when someone pushed on top;
/// the server reflog shows every move, and `wt push` re-publishes.
#[test]
fn undo_push_refusals() {
    require_e2e!();
    let env = TestEnv::with_server("undo_push_refusals");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("app.txt", "v1\n")]);
    let alice_api = alice.api();
    let c1 = a.head_id();
    let out = a.wt().args(["undo", "--yes"]).fails();
    assert!(out.mentions("created origin/main"), "{out}");

    let dave = env.user("dave");
    dave.register();
    add_member(&alice_api, "alice", "dave", "developer");
    let d = dave.clone_tree("alice", TREE, "dave-proj");
    let d1 = d.commit(&[("dave.txt", "dave\n")], "dave's change");
    d.push();
    let ops = undoable(&d);
    assert_eq!(str_field(&ops[0], "op"), "push", "{ops:#?}");
    // Developers cannot rewind the branch.
    let out = d.wt().args(["undo", "--yes"]).fails();
    assert!(out.mentions("branch:force_push"), "{out}");
    assert!(out.mentions("wt revert"), "{out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(d1.clone())
    );

    // Alice pushes on top: dave's push can no longer be undone.
    a.pull();
    let a2 = a.commit(&[("app.txt", "v2\n")], "alice on top");
    a.push();
    let out = d.wt().args(["undo", "--yes"]).fails();
    assert!(out.mentions("has moved since your push"), "{out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(a2.clone())
    );

    // The owner undoes her own push; the server reflog records it.
    a.wt().args(["undo", "--yes"]).ok();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(d1.clone())
    );
    let out = a.wt().args(["reflog", "--remote"]).ok();
    assert!(out.mentions("restore by alice"), "{out}");
    assert!(out.mentions("push by dave"), "{out}");
    let audit = alice_api
        .get("/api/v1/audit?tenant=alice&action=branch.restore")
        .ok_json();
    assert!(
        find_objects(&audit, &["action"])
            .iter()
            .any(|e| str_field(e, "action") == "branch.restore"),
        "restore audited:\n{}",
        pretty(&audit)
    );
    // `wt push` publishes the snapshot again (a fast-forward from d1).
    a.push();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), Some(a2));
    assert_ne!(c1, d1);
}
