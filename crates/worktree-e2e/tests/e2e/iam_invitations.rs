//! Tenant invitations (§12.2, §12.3): invite by username or email
//! (optionally into a team), preview, accept once, only by the invitee;
//! revoked invitations cannot be accepted.

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{require_e2e, TestEnv, User};

use crate::iam_common::{find_by, member_role, mentions, username_of, wt_json};

/// (`code`, `invitation.id`) from `wt --json invite create`.
fn invite(owner: &User, args: &[&str]) -> (String, String) {
    let mut full = vec!["invite", "create", "alice"];
    full.extend_from_slice(args);
    let v = wt_json(owner, &full);
    let code = v["code"]
        .as_str()
        .unwrap_or_else(|| panic!("invite create:\n{}", pretty(&v)))
        .to_string();
    assert!(code.starts_with("wti_"), "invitation code format: {code}");
    let id = v["invitation"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("invite create:\n{}", pretty(&v)))
        .to_string();
    (code, id)
}

fn members(owner: &User) -> Value {
    wt_json(owner, &["tenant", "members", "alice"])
}

#[test]
fn invitations() {
    require_e2e!();
    let env = TestEnv::with_server("invitations");
    let alice = env.user("alice");
    alice.register();
    let users: Vec<User> = ["carol", "dave", "erin"]
        .iter()
        .map(|n| {
            let u = env.user(n);
            u.register();
            u
        })
        .collect();
    let (carol, dave, erin) = (&users[0], &users[1], &users[2]);

    // --- invite carol by username as a viewer
    let (code, id) = invite(&alice, &["--user", "carol", "--role", "viewer"]);
    let list = wt_json(&alice, &["invite", "list", "alice"]);
    let inv = find_by(&list, "id", &id)
        .unwrap_or_else(|| panic!("invitation listed:\n{}", pretty(&list)));
    assert_eq!(inv["username"], "carol");
    assert_eq!(inv["role"], "viewer");
    assert!(
        !serde_json::to_string(&list).unwrap().contains(&code),
        "the code is not stored/listed"
    );

    // Public preview.
    let preview = wt_json(carol, &["invite", "show", &code]);
    assert!(
        find_by(&preview, "slug", "alice").is_some() && mentions(&preview, "viewer"),
        "preview names the tenant and role:\n{}",
        pretty(&preview)
    );
    let anon = env
        .server()
        .api()
        .get(&format!("/api/v1/invitations/{code}"));
    anon.assert_ok();

    // Someone else cannot use carol's invitation.
    dave.wt().args(["invite", "accept", &code]).fails();
    assert_eq!(member_role(&members(&alice), "dave"), None);
    assert_eq!(member_role(&members(&alice), "carol"), None);

    // carol accepts: viewer of alice.
    carol.wt().args(["invite", "accept", &code]).ok();
    assert_eq!(
        member_role(&members(&alice), "carol").as_deref(),
        Some("viewer"),
        "{}",
        pretty(&members(&alice))
    );
    let carol_me = carol.api().get("/api/v1/auth/me").ok_json();
    assert!(
        find_objects(&carol_me, &["tenant", "role"])
            .iter()
            .any(|m| m["tenant"]["slug"] == "alice" && m["role"] == "viewer"),
        "carol's memberships:\n{}",
        pretty(&carol_me)
    );
    // One-time.
    carol.wt().args(["invite", "accept", &code]).fails();
    let list = wt_json(&alice, &["invite", "list", "alice"]);
    let inv = find_by(&list, "id", &id).expect("accepted invitation still listed");
    assert!(
        inv.get("accepted_at").is_some_and(|v| !v.is_null()),
        "accepted_at set:\n{}",
        pretty(inv)
    );
    // Viewers cannot invite.
    carol
        .wt()
        .args([
            "invite", "create", "alice", "--user", "erin", "--role", "admin",
        ])
        .fails();

    // --- invite by email into a team
    alice
        .wt()
        .args(["team", "create", "alice", "qa", "--name", "QA"])
        .ok();
    let (code, _) = invite(
        &alice,
        &[
            "--email",
            &dave.email,
            "--role",
            "developer",
            "--team",
            "qa",
        ],
    );
    dave.wt().args(["invite", "accept", &code]).ok();
    assert_eq!(
        member_role(&members(&alice), "dave").as_deref(),
        Some("developer")
    );
    let team = wt_json(&alice, &["team", "show", "alice", "qa"]);
    assert!(
        find_objects(&team, &["account", "role"])
            .into_iter()
            .any(|m| username_of(m) == Some("dave")),
        "dave joined the team:\n{}",
        pretty(&team)
    );

    // --- revoked invitations are dead
    let (code, id) = invite(&alice, &["--email", &erin.email, "--role", "viewer"]);
    alice
        .wt()
        .args(["invite", "revoke", "alice", &id])
        .confirmed();
    erin.wt().args(["invite", "accept", &code]).fails();
    assert_eq!(member_role(&members(&alice), "erin"), None);
    let list = wt_json(&alice, &["invite", "list", "alice"]);
    if let Some(inv) = find_by(&list, "id", &id) {
        assert_eq!(inv["revoked"], true, "{}", pretty(inv));
    }

    // Garbage codes are rejected.
    erin.wt()
        .args(["invite", "accept", "wti_not-a-real-code"])
        .fails();
}
