//! Two-factor login (§12.2): `wt mfa enable` driven over stdin with a TOTP
//! code computed from the printed secret; `wt login` then needs a code
//! (`--mfa-code` or `WT_MFA_CODE`); TOTP codes cannot be replayed; recovery
//! codes work exactly once and can be regenerated; `wt mfa disable`.

use std::time::Duration;

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::totp::secret_from_otpauth;
use worktree_e2e::{require_e2e, TestEnv, Totp, User};

use crate::iam_common::{cli_api, recovery_codes, wt_json};

/// `{totp_enabled, recovery_codes_left}` from `wt --json mfa status`.
fn mfa_status(user: &User) -> (bool, Option<i64>) {
    let v = wt_json(user, &["mfa", "status"]);
    let state = find_objects(&v, &["totp_enabled"])
        .first()
        .map(|s| (*s).clone())
        .unwrap_or_else(|| panic!("mfa status:\n{}", pretty(&v)));
    (
        state["totp_enabled"].as_bool().unwrap_or(false),
        state["recovery_codes_left"].as_i64(),
    )
}

#[test]
fn mfa_login() {
    require_e2e!();
    let env = TestEnv::with_server("mfa_login");
    let url = env.url();
    let erin = env.user("erin");
    erin.register();
    assert!(!mfa_status(&erin).0, "MFA starts disabled");

    // --- enroll: read the secret from the output, answer the code prompt
    let mut enroll = erin.wt().args(["mfa", "enable"]).spawn();
    let secret = enroll.wait_for(Duration::from_secs(60), secret_from_otpauth);
    let mut totp = Totp::new(&secret);
    enroll.write_stdin(format!("{}\n", totp.next_code()));
    let out = enroll.wait();
    out.assert_success();
    assert!(
        (out.mentions("otpauth://totp/W0rkTree:erin")
            || out.mentions("otpauth://totp/W0rkTree%3Aerin"))
            && out.mentions("issuer=W0rkTree"),
        "otpauth URL per §12.2\n{out}"
    );
    let codes = recovery_codes(&out.combined());
    assert_eq!(
        codes.len(),
        10,
        "10 recovery codes printed: {codes:?}\n{out}"
    );
    assert_eq!(mfa_status(&erin), (true, Some(10)));

    // --- the first login step now asks for a second factor
    let first = env.server().api().post(
        "/api/v1/auth/login",
        json!({"username": "erin", "password": erin.password}),
    );
    let body = first.ok_json();
    assert_eq!(body["mfa_required"], true, "{}", pretty(&body));
    assert!(body["mfa_token"].as_str().is_some_and(|t| !t.is_empty()));
    assert!(
        body.get("token")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty),
        "no session before the second factor:\n{}",
        pretty(&body)
    );

    // --- wt login without / with a wrong code fails
    let out = erin.login_with(&[]);
    assert!(!out.success(), "login without a code must fail\n{out}");
    let out = erin.login_with(&["--mfa-code", &totp.wrong_code()]);
    assert!(!out.success(), "login with a wrong code must fail\n{out}");

    // --- correct TOTP code
    let code = totp.next_code();
    erin.login_with(&["--mfa-code", &code]).assert_success();
    let who = erin.wt().args(["whoami", &url]).ok();
    assert!(who.mentions("erin"), "{who}");
    let sessions = wt_json(&erin, &["session", "list"]);
    let current = find_objects(&sessions, &["current"])
        .into_iter()
        .find(|s| s["current"] == true)
        .cloned()
        .unwrap_or_else(|| panic!("no current session:\n{}", pretty(&sessions)));
    assert_eq!(
        current["mfa"],
        true,
        "the session records the second factor:\n{}",
        pretty(&current)
    );
    let state = cli_api(&env, &erin).get("/api/v1/auth/mfa").ok_json();
    assert_eq!(state["totp_enabled"], true, "{}", pretty(&state));

    // Replaying the same TOTP code is refused.
    let out = erin.login_with(&["--mfa-code", &code]);
    assert!(!out.success(), "a TOTP code is single use\n{out}");

    // --- recovery codes: each works once (WT_MFA_CODE or --mfa-code)
    erin.wt()
        .args(["login", &url, "-u", "erin", "--password-stdin"])
        .env("WT_MFA_CODE", &codes[0])
        .stdin(format!("{}\n", erin.password))
        .ok();
    let out = erin.login_with(&["--mfa-code", &codes[0]]);
    assert!(!out.success(), "recovery code reuse must fail\n{out}");
    erin.login_with(&["--mfa-code", &codes[1]]).assert_success();
    assert_eq!(mfa_status(&erin), (true, Some(8)));

    // --- regenerating recovery codes invalidates the old ones
    let regen = erin
        .wt()
        .args(["mfa", "recovery-codes"])
        .stdin(format!("{}\n", totp.next_code()))
        .ok();
    let fresh = recovery_codes(&regen.combined());
    assert_eq!(fresh.len(), 10, "10 new recovery codes: {fresh:?}\n{regen}");
    assert!(
        fresh.iter().all(|c| !codes.contains(c)),
        "new codes differ from the old ones"
    );
    assert_eq!(mfa_status(&erin), (true, Some(10)));
    let out = erin.login_with(&["--mfa-code", &codes[2]]);
    assert!(!out.success(), "old recovery codes are void\n{out}");
    erin.login_with(&["--mfa-code", &fresh[0]]).assert_success();

    // --- disable
    erin.wt()
        .args(["mfa", "disable", "--password-stdin"])
        .stdin(format!("{}\n", erin.password))
        .confirmed();
    assert!(!mfa_status(&erin).0, "MFA disabled");
    erin.login();
    erin.api().get("/api/v1/auth/me").assert_ok();
}
