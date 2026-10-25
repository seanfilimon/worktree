//! Snapshot signing (ENGINEERING_PLAN §13.6): `wt identity generate` +
//! `register`, `[signing] key/auto` via `wt config set`, a branch protected
//! with `--require-signed` accepts the signed push and rejects an unsigned
//! one, `wt log --show-signature` / `wt identity verify` report "verified",
//! and a signature by an unregistered key is refused on any branch.

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, Repo, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

/// `signature_status` of the newest snapshot per `wt --json log
/// --show-signature -n 1`.
#[track_caller]
fn head_signature(repo: &Repo) -> Value {
    let v = repo
        .wt()
        .args(["log", "--show-signature", "-n", "1"])
        .json();
    find_objects(&v, &["id", "signature_status"])
        .first()
        .map(|s| (*s).clone())
        .unwrap_or_else(|| panic!("no signature_status in\n{}", pretty(&v)))
}

#[test]
fn signing() {
    require_e2e!();
    let env = TestEnv::with_server("signing");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# signed\n")]);
    let alice_api = alice.api();

    // --- key: generate, register
    let generated = alice.wt().args(["identity", "generate"]).json();
    let fingerprint = find_objects(&generated, &["fingerprint"])
        .first()
        .and_then(|k| k["fingerprint"].as_str().map(String::from))
        .unwrap_or_else(|| panic!("fingerprint in\n{}", pretty(&generated)));
    alice.wt().args(["identity", "generate"]).fails(); // exists; needs --force
    let registered = a.wt().args(["identity", "register"]).json();
    let key_id = find_objects(&registered, &["id"])
        .first()
        .and_then(|k| k["id"].as_str().map(String::from))
        .unwrap_or_else(|| panic!("key id in\n{}", pretty(&registered)));
    let listed = alice.wt().args(["identity", "list"]).json();
    assert!(
        listed.to_string().contains(&fingerprint),
        "identity list shows the key:\n{}",
        pretty(&listed)
    );
    let keys = alice_api.get("/api/v1/accounts/alice/keys").ok_json();
    assert!(
        keys.to_string().contains(&key_id),
        "the server lists the registered key:\n{}",
        pretty(&keys)
    );

    a.wt()
        .args(["config", "set", "signing.key", "default"])
        .ok();
    a.wt().args(["config", "set", "signing.auto", "true"]).ok();
    assert_eq!(
        a.wt()
            .args(["config", "get", "signing.auto"])
            .ok()
            .stdout
            .trim(),
        "true"
    );

    // --- main requires signed snapshots
    a.wt()
        .args(["protect", "set", "main", "--require-signed"])
        .ok();

    let signed = a.commit(&[("signed.txt", "signed by alice\n")], "signed change");
    let sig = head_signature(&a);
    assert_eq!(str_field(&sig, "id"), signed);
    assert_eq!(
        str_field(&sig, "signature_status"),
        "verified",
        "{}",
        pretty(&sig)
    );
    a.push();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(signed.clone()),
        "the signed push is accepted"
    );
    let rest = alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}/snapshots/{signed}"))
        .ok_json();
    assert_eq!(
        find_objects(&rest, &["verified"])
            .first()
            .map(|s| s["verified"].clone()),
        Some(Value::Bool(true)),
        "the server stored the snapshot as verified:\n{}",
        pretty(&rest)
    );
    let text = a.wt().args(["log", "--show-signature", "-n", "1"]).ok();
    assert!(text.mentions("verified"), "human log\n{text}");
    let verify = a.wt().args(["identity", "verify"]).ok();
    assert!(
        verify.mentions("good signature"),
        "identity verify\n{verify}"
    );

    // --- bob (no key): an unsigned snapshot is refused on main
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    // bob has no local copy of alice's key: the signature is intact but
    // only verifiable against her registered keys.
    let status = str_field(&head_signature(&b), "signature_status");
    assert!(
        status == "verified" || status == "unverified",
        "alice's signature in bob's clone: {status}"
    );
    let verify = b
        .wt()
        .args(["identity", "verify", &signed, "--account", "alice"])
        .ok();
    assert!(
        verify.mentions("good signature"),
        "verified against alice's registered key\n{verify}"
    );
    b.commit(&[("unsigned.txt", "not signed\n")], "unsigned change");
    assert_eq!(
        str_field(&head_signature(&b), "signature_status"),
        "unsigned"
    );
    let out = b.wt().arg("push").fails();
    assert!(
        out.mentions("sign"),
        "rejection explains the signature requirement\n{out}"
    );
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(signed.clone()),
        "main unchanged after the unsigned push"
    );
    // The same snapshot is fine on an unprotected branch.
    b.wt().args(["switch", "-c", "bob-feature"]).ok();
    b.wt().args(["push", "--branch", "bob-feature"]).ok();

    // --- carol signs with a key the server does not know: refused anywhere
    let carol = env.user("carol");
    carol.register();
    add_member(&alice_api, "alice", "carol", "developer");
    let c = carol.clone_tree("alice", TREE, TREE);
    carol.wt().args(["identity", "generate"]).ok();
    c.wt().args(["switch", "-c", "carol-feature"]).ok();
    c.write("carol.txt", "signed with an unregistered key\n");
    c.wt()
        .args(["snapshot", "-m", "carol: signed", "--sign"])
        .ok();
    let out = c.wt().args(["push", "--branch", "carol-feature"]).fails();
    assert!(
        out.mentions("signature"),
        "an unverifiable signature is refused\n{out}"
    );
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "carol-feature"),
        None,
        "carol-feature was not created"
    );
}
