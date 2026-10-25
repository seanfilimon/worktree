//! Admin-issued password reset codes (§12.2, §12.3): `wt admin reset-code`,
//! `wt password-reset CODE --password-stdin`; the code is single use, old
//! sessions and the old password stop working, the new password works.

use serde_json::json;
use worktree_e2e::json::pretty;
use worktree_e2e::{require_e2e, TestEnv};

use crate::iam_common::{cli_api, new_password, wt_json};

#[test]
fn password_reset() {
    require_e2e!();
    let env = TestEnv::with_server("password_reset");
    let url = env.url();
    let mut dave = env.user("dave");
    dave.register();
    let old_password = dave.password.clone();
    let rest_session = dave.api();
    let cli_session = cli_api(&env, &dave);
    rest_session.get("/api/v1/auth/me").assert_ok();
    cli_session.get("/api/v1/auth/me").assert_ok();

    // --- only server admins issue codes
    dave.wt().args(["admin", "reset-code", "dave"]).fails();
    let admin = env.admin_user();
    admin.login();
    let issued = wt_json(&admin, &["admin", "reset-code", "dave", "--ttl-hours", "2"]);
    let code = issued["code"]
        .as_str()
        .unwrap_or_else(|| panic!("reset-code output:\n{}", pretty(&issued)))
        .to_string();
    assert!(code.starts_with("wtr_"), "reset code format: {code}");
    assert!(
        issued["expires_at"].as_str().is_some_and(|s| !s.is_empty()),
        "{}",
        pretty(&issued)
    );

    // --- use it
    let fresh = new_password();
    dave.wt()
        .args([
            "--server",
            &url,
            "password-reset",
            &code,
            "--password-stdin",
        ])
        .stdin(format!("{fresh}\n"))
        .ok();

    // Old sessions are gone, the old password is rejected.
    rest_session.get("/api/v1/auth/me").assert_status(401);
    cli_session.get("/api/v1/auth/me").assert_status(401);
    dave.wt().args(["session", "list"]).fails();
    env.server()
        .api()
        .post(
            "/api/v1/auth/login",
            json!({"username": "dave", "password": old_password}),
        )
        .assert_status(401);
    let out = dave.login_with(&[]);
    assert!(!out.success(), "old password must not log in\n{out}");

    // The code is single use.
    dave.wt()
        .args([
            "--server",
            &url,
            "password-reset",
            &code,
            "--password-stdin",
        ])
        .stdin(format!("{}\n", new_password()))
        .fails();

    // --- the new password works (CLI and REST)
    dave.password = fresh;
    dave.login();
    wt_json(&dave, &["session", "list"]);
    dave.api().get("/api/v1/auth/me").assert_ok();

    // A bogus code is rejected without touching the account.
    let bogus = env.server().api().post(
        "/api/v1/auth/password-reset",
        json!({"code": "wtr_bogus", "new_password": new_password()}),
    );
    assert!(
        (400..500).contains(&bogus.status),
        "bogus reset code rejected\n{bogus}"
    );
    dave.api().get("/api/v1/auth/me").assert_ok();
}
