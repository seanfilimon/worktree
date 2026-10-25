//! Teams and per-tree grants (§12.1 step 4): a user who is not a member of
//! the tenant gets push access to one tree through a team grant, and loses
//! it when the grant (or his team membership) goes away.

use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{publish_tree, server_tip, TREE};
use crate::iam_common::{
    assert_push_denied, find_by, find_grant, mentions, rest_member_role, string_list, tree_ref,
    username_of, wt_json,
};

#[test]
fn teams_and_grants() {
    require_e2e!();
    let env = TestEnv::with_server("teams_and_grants");
    let alice = env.user("alice");
    publish_tree(&env, &alice, &[("README.md", "# teams\n")]);
    let alice_api = alice.api();
    let tree = tree_ref("alice");
    let bob = env.user("bob");
    bob.register();

    // --- bob is not a member of alice's tenant: no access to the private tree
    assert_eq!(rest_member_role(&alice_api, "alice", "bob"), None);
    bob.wt()
        .args(["clone", &env.url(), &tree])
        .arg(bob.dir.join("before-grant"))
        .fails();

    // --- team with bob in it
    let team = wt_json(
        &alice,
        &[
            "team",
            "create",
            "alice",
            "devs",
            "--name",
            "Developers",
            "--description",
            "Core developers",
        ],
    );
    let created = find_by(&team, "slug", "devs")
        .unwrap_or_else(|| panic!("team create output:\n{}", pretty(&team)));
    assert_eq!(created["name"], "Developers", "{}", pretty(&team));
    alice
        .wt()
        .args(["team", "add", "alice", "devs", "bob"])
        .ok();
    let list = wt_json(&alice, &["team", "list", "alice"]);
    assert!(
        find_by(&list, "slug", "devs").is_some(),
        "team list:\n{}",
        pretty(&list)
    );
    let shown = wt_json(&alice, &["team", "show", "alice", "devs"]);
    let members: Vec<&str> = find_objects(&shown, &["account", "role"])
        .into_iter()
        .filter_map(username_of)
        .collect();
    assert_eq!(members, vec!["bob"], "team members:\n{}", pretty(&shown));
    // Team membership alone grants nothing in the tenant.
    assert_eq!(rest_member_role(&alice_api, "alice", "bob"), None);

    // --- grant the team developer on the tree
    alice
        .wt()
        .args([
            "grant",
            "set",
            &tree,
            "--team",
            "devs",
            "--role",
            "developer",
        ])
        .ok();
    let grants = wt_json(&alice, &["grant", "list", &tree]);
    let g = find_grant(&grants, "team", "devs")
        .unwrap_or_else(|| panic!("team grant missing:\n{}", pretty(&grants)));
    assert_eq!(g["role"], "developer");

    let eff = alice_api
        .get(&format!(
            "/api/v1/iam/effective?tenant=alice&tree={TREE}&principal=user:bob"
        ))
        .ok_json();
    assert!(
        mentions(&eff, "devs") && mentions(&eff, "developer"),
        "effective access shows the team role:\n{}",
        pretty(&eff)
    );
    assert!(
        string_list(&eff, "permissions")
            .iter()
            .any(|p| p == "tree:write"),
        "bob may write through the team grant:\n{}",
        pretty(&eff)
    );
    let mine = wt_json(
        &bob,
        &["iam", "effective", "--tenant", "alice", "--tree", &tree],
    );
    assert!(
        string_list(&mine, "permissions")
            .iter()
            .any(|p| p == "tree:write"),
        "bob's own effective permissions:\n{}",
        pretty(&mine)
    );

    // --- bob clones and pushes
    let b = bob.clone_tree("alice", TREE, TREE);
    assert_eq!(b.read("README.md"), "# teams\n");
    let tip = b.commit(&[("bob.txt", "from bob\n")], "bob: first change");
    b.push();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(tip),
        "bob's push landed"
    );

    // --- removing the grant removes access
    alice
        .wt()
        .args(["grant", "remove", &tree, "--team", "devs"])
        .confirmed();
    let grants = wt_json(&alice, &["grant", "list", &tree]);
    assert!(
        find_grant(&grants, "team", "devs").is_none(),
        "grant removed:\n{}",
        pretty(&grants)
    );
    b.commit(&[("bob.txt", "from bob, again\n")], "bob: second change");
    assert_push_denied(&b, &alice_api, "alice", None);

    // --- a direct user grant works the same way
    alice
        .wt()
        .args([
            "grant",
            "set",
            &tree,
            "--user",
            "bob",
            "--role",
            "developer",
        ])
        .ok();
    let grants = wt_json(&alice, &["grant", "list", &tree]);
    assert_eq!(
        find_grant(&grants, "user", "bob").map(|g| g["role"].clone()),
        Some("developer".into()),
        "user grant:\n{}",
        pretty(&grants)
    );
    let tip = b.head_id();
    b.push();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), Some(tip));
    alice
        .wt()
        .args(["grant", "remove", &tree, "--user", "bob"])
        .confirmed();

    // --- team grant again; then leaving the team removes access
    alice
        .wt()
        .args([
            "grant",
            "set",
            &tree,
            "--team",
            "devs",
            "--role",
            "developer",
        ])
        .ok();
    let tip = b.commit(&[("bob.txt", "third\n")], "bob: third change");
    b.push();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), Some(tip));
    alice
        .wt()
        .args(["team", "remove", "alice", "devs", "bob"])
        .confirmed();
    b.commit(&[("bob.txt", "fourth\n")], "bob: fourth change");
    assert_push_denied(&b, &alice_api, "alice", None);

    // --- deleting the team deletes its grants
    alice
        .wt()
        .args(["team", "delete", "alice", "devs"])
        .confirmed();
    let list = wt_json(&alice, &["team", "list", "alice"]);
    assert!(
        find_by(&list, "slug", "devs").is_none(),
        "team deleted:\n{}",
        pretty(&list)
    );
    let grants = wt_json(&alice, &["grant", "list", &tree]);
    assert!(
        find_grant(&grants, "team", "devs").is_none(),
        "team grants go with the team:\n{}",
        pretty(&grants)
    );

    // Audit trail of the IAM changes.
    let audit = alice_api.get("/api/v1/audit?limit=500").ok_json();
    let actions: Vec<String> = find_objects(&audit, &["action", "actor"])
        .into_iter()
        .filter_map(|e| e["action"].as_str().map(String::from))
        .collect();
    assert!(
        actions.iter().any(|a| a.contains("team")) && actions.iter().any(|a| a.contains("grant")),
        "team and grant changes are audited: {actions:?}"
    );
}
