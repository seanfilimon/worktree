//! Two users collaborating through the server: push, membership, clone,
//! fetch, fast-forward pull, concurrent divergent pushes, merge on pull,
//! convergence.

use worktree_e2e::fsutil::assert_same_files;
use worktree_e2e::json::{self, parents, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

#[test]
fn push_clone_pull() {
    require_e2e!();
    let env = TestEnv::with_server("push_clone_pull");
    let url = env.url();

    // --- alice publishes
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("README.md", "# shared project\n"),
            ("src/app.txt", "app v1\n"),
        ],
    );
    let who = alice.wt().args(["whoami", &url]).ok();
    assert!(who.mentions("alice"), "whoami after register\n{who}");
    let remotes = a.wt().args(["remote", "list"]).json();
    assert!(
        json::all_strings(&remotes)
            .iter()
            .any(|s| s.trim_end_matches('/') == url),
        "remote list shows origin:\n{}",
        json::pretty(&remotes)
    );
    let alice_api = alice.api();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main").as_deref(),
        Some(a.head_id().as_str()),
        "server main tip after push"
    );
    a.push(); // up to date: still succeeds
    let summary = alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}"))
        .ok_json();
    assert_eq!(
        summary["snapshot_count"].as_u64(),
        Some(1),
        "tree summary:\n{}",
        json::pretty(&summary)
    );

    // --- bob joins alice's tenant and clones
    let bob = env.user("bob");
    bob.register();
    // Not a member yet: cloning a private tree must fail.
    if summary["tree"]["visibility"].as_str() == Some("private") {
        bob.wt()
            .args(["clone", &url, &format!("alice/{TREE}")])
            .arg(bob.dir.join("too-early"))
            .fails();
    }
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    assert_same_files(&a.dir, &b.dir);
    assert_eq!(b.head_id(), a.head_id());
    assert_eq!(b.current_branch(), "main");
    b.assert_clean();

    // --- bob pushes; alice fetches (no local change) then pulls (fast-forward)
    let bob_tip = b.commit(&[("src/app.txt", "app v2 from bob\n")], "bob: app v2");
    b.push();
    let alice_before = a.head_id();
    a.wt().arg("fetch").ok();
    assert_eq!(
        a.head_id(),
        alice_before,
        "fetch must not move the local branch"
    );
    assert_eq!(
        a.read("src/app.txt"),
        "app v1\n",
        "fetch must not touch files"
    );
    assert_eq!(
        str_field(&a.log(&["origin/main", "-n", "1"])[0], "id"),
        bob_tip,
        "origin/main updated by fetch"
    );
    a.pull();
    assert_eq!(a.head_id(), bob_tip, "pull fast-forwards");
    assert_eq!(a.read("src/app.txt"), "app v2 from bob\n");
    a.assert_clean();

    // --- concurrent divergent pushes: exactly one wins
    a.commit(&[("alice.txt", "from alice\n")], "alice: add alice.txt");
    b.commit(&[("bob.txt", "from bob\n")], "bob: add bob.txt");
    let (ra, rb) = std::thread::scope(|s| {
        let ha = s.spawn(|| a.wt().arg("push").run());
        let hb = s.spawn(|| b.wt().arg("push").run());
        (
            ha.join().expect("alice push thread"),
            hb.join().expect("bob push thread"),
        )
    });
    assert!(
        ra.success() != rb.success(),
        "exactly one of two divergent pushes must succeed\n{ra}\n\n{rb}"
    );
    let (winner, loser, lost) = if ra.success() {
        (&a, &b, &rb)
    } else {
        (&b, &a, &ra)
    };
    assert!(
        lost.mentions("pull"),
        "rejected push should hint at `wt pull`\n{lost}"
    );
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(winner.head_id()),
        "server tip is the winner's"
    );

    // --- loser pulls (three-way merge), pushes; winner pulls (fast-forward)
    let loser_tip = loser.head_id();
    loser.pull();
    let merge = loser.head();
    let mp = parents(&merge);
    assert_eq!(
        mp.len(),
        2,
        "pull of a diverged branch creates a merge:\n{}",
        json::pretty(&merge)
    );
    assert!(
        mp.contains(&loser_tip) && mp.contains(&winner.head_id()),
        "merge parents: {mp:?}"
    );
    assert!(loser.exists("alice.txt") && loser.exists("bob.txt"));
    loser.assert_clean();
    loser.push();
    winner.pull();

    // --- convergence
    assert_eq!(
        a.head_id(),
        b.head_id(),
        "both users converge on the same head"
    );
    assert_eq!(
        str_field(&a.head(), "manifest"),
        str_field(&b.head(), "manifest"),
        "identical manifests"
    );
    assert_same_files(&a.dir, &b.dir);
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(a.head_id())
    );
    a.assert_clean();
    b.assert_clean();
    assert_eq!(a.log(&[]).len(), b.log(&[]).len(), "same history length");
}
