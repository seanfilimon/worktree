//! Device authorization and refresh tokens (§16.6): `wt login --device` on
//! a second machine is approved over REST by the same user signed in
//! elsewhere (as the admin panel does), shows up as a device session, can
//! be denied, renews a rejected access token transparently with its saved
//! refresh token; REST refresh tokens rotate and a reused one revokes the
//! whole login.

use std::time::Duration;

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{fsutil, require_e2e, Running, TestEnv, User};

use crate::iam_common::{api_with_token, wt_json};

/// The `XXXX-XXXX` user code `wt login --device` printed, if any yet.
fn user_code(out: &str) -> Option<String> {
    let rest = &out[out.find("enter the code ")? + "enter the code ".len()..];
    let code: String = rest.chars().take(9).collect();
    let ok = code.len() == 9
        && code.char_indices().all(|(i, c)| {
            if i == 4 {
                c == '-'
            } else {
                c.is_ascii_uppercase()
            }
        });
    ok.then_some(code)
}

/// Start `wt --json login <server> --device --no-browser` as `machine` and
/// return the running process plus the user code it printed.
fn start_device_login(machine: &User, server: &str, extra: &[&str]) -> (Running, String) {
    let mut login = machine
        .wt()
        .args(["--json", "login", server, "--device", "--no-browser"])
        .args(extra)
        .env("WT_NO_BROWSER", "1")
        .timeout(Duration::from_secs(180))
        .spawn();
    let code = login.wait_for(Duration::from_secs(60), user_code);
    (login, code)
}

/// Replace the access token in `machine`'s credentials.toml with a
/// well-formed but unknown one, keeping the refresh token.
fn break_access_token(machine: &User) -> String {
    let path = machine.credentials_path();
    let text = std::fs::read_to_string(&path).expect("credentials.toml");
    let old = machine.saved_token().expect("saved token");
    assert!(
        text.contains("refresh_token = \"wtrt_"),
        "device login saves a refresh token:\n{text}"
    );
    let bogus = format!("wt_{}", "A".repeat(43));
    fsutil::write(&path, text.replace(&old, &bogus));
    old
}

#[test]
fn device_auth() {
    require_e2e!();
    let env = TestEnv::with_server("device_auth");
    let url = env.url();
    let dana = env.user("dana");
    dana.register();
    // Dana is also signed in on the web (another client).
    let web = dana.api();
    // A second machine with its own WT_HOME and no saved login.
    let laptop = env.user("dana-laptop");

    // --- device flow, approved over REST
    let (login, code) = start_device_login(&laptop, &url, &["--scope", "tree:read"]);
    let path = format!("/api/v1/auth/device/{code}");
    env.server()
        .api()
        .get(&path)
        .assert_error(401, "unauthenticated");
    // Codes are accepted in any case.
    let req = web
        .get(&format!("/api/v1/auth/device/{}", code.to_lowercase()))
        .ok_json();
    assert_eq!(req["status"], "pending", "{}", pretty(&req));
    assert_eq!(req["mfa_required"], false, "{}", pretty(&req));
    assert!(
        req["client_name"]
            .as_str()
            .is_some_and(|n| n.starts_with("wt CLI")),
        "{}",
        pretty(&req)
    );
    assert_eq!(req["scopes"], json!(["tree:read"]), "{}", pretty(&req));
    let approved = web.post(&format!("{path}/approve"), json!({})).ok_json();
    assert_eq!(approved["status"], "approved", "{}", pretty(&approved));
    web.post(&format!("{path}/deny"), json!({}))
        .assert_error(409, "conflict");

    let out = login.wait();
    out.assert_success();
    let session = out.json();
    assert_eq!(session["username"], "dana", "{out}");
    assert!(
        !out.combined().contains("wtd_"),
        "the device code is never printed\n{out}"
    );
    let who = laptop.wt().args(["whoami", &url]).ok();
    assert!(who.mentions("dana"), "{who}");

    // The device session is listed (scoped to tree:read, so list it as Dana).
    let sessions = wt_json(&dana, &["session", "list"]);
    let device = find_objects(&sessions, &["id", "current"])
        .into_iter()
        .find(|s| {
            s["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("device: wt CLI"))
        })
        .cloned()
        .unwrap_or_else(|| panic!("no device session:\n{}", pretty(&sessions)));
    assert_eq!(
        device["scopes"],
        json!(["tree:read"]),
        "{}",
        pretty(&device)
    );
    let table = dana.wt().args(["session", "list"]).ok();
    assert!(table.mentions("device (wt CLI"), "{table}");

    // The redeemed code is gone.
    web.get(&path).assert_error(404, "not_found");

    // --- transparent refresh: the server rejects the saved access token
    let old = break_access_token(&laptop);
    let who = laptop.wt().args(["whoami", &url]).ok();
    assert!(who.mentions("dana"), "gRPC renews the token\n{who}");
    let renewed = laptop.saved_token().expect("token");
    assert!(renewed != old && renewed.starts_with("wt_"), "{renewed}");
    break_access_token(&laptop);
    let mine = laptop.wt().args(["--json", "session", "list"]).ok();
    assert!(
        mine.mentions("device: wt CLI"),
        "REST renews the token\n{mine}"
    );
    // The rotation replaced the device session (one row per login).
    let sessions = wt_json(&dana, &["session", "list"]);
    let devices = find_objects(&sessions, &["id", "current"])
        .into_iter()
        .filter(|s| {
            s["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("device: "))
        })
        .count();
    assert_eq!(devices, 1, "{}", pretty(&sessions));

    // --- a denied device login fails
    let desktop = env.user("dana-desktop");
    let (login, code) = start_device_login(&desktop, &url, &[]);
    let denied = web
        .post(&format!("/api/v1/auth/device/{code}/deny"), json!({}))
        .ok_json();
    assert_eq!(denied["status"], "denied", "{}", pretty(&denied));
    let out = login.wait();
    assert!(!out.success(), "a denied login must fail\n{out}");
    assert!(out.mentions("denied"), "{out}");
    assert!(desktop.saved_token().is_none(), "nothing saved");

    // --- REST refresh rotation and reuse detection
    let anon = env.server().api();
    let first = anon
        .post(
            "/api/v1/auth/login",
            json!({"username": "dana", "password": dana.password}),
        )
        .ok_json();
    let refresh1 = string(&first, "refresh_token");
    let access1 = string(&first, "token");
    assert_eq!(first["expires_in"], 3600, "{}", pretty(&first));
    let second = anon
        .post("/api/v1/auth/refresh", json!({"refresh_token": refresh1}))
        .ok_json();
    let (access2, refresh2) = (string(&second, "token"), string(&second, "refresh_token"));
    assert_ne!(refresh2, refresh1);
    api_with_token(&env, "dana (access 1)", &access1)
        .get("/api/v1/auth/me")
        .assert_status(401);
    api_with_token(&env, "dana (access 2)", &access2)
        .get("/api/v1/auth/me")
        .assert_ok();
    let third = anon
        .post("/api/v1/auth/refresh", json!({"refresh_token": refresh2}))
        .ok_json();
    let (access3, refresh3) = (string(&third, "token"), string(&third, "refresh_token"));
    // Reusing the first (already rotated twice) token revokes the login.
    anon.post("/api/v1/auth/refresh", json!({"refresh_token": refresh1}))
        .assert_error(401, "invalid_grant");
    api_with_token(&env, "dana (access 3)", &access3)
        .get("/api/v1/auth/me")
        .assert_status(401);
    anon.post("/api/v1/auth/refresh", json!({"refresh_token": refresh3}))
        .assert_error(401, "invalid_grant");
    // Other logins are unaffected.
    web.get("/api/v1/auth/me").assert_ok();
    laptop.wt().args(["whoami", &url]).ok();
}

#[track_caller]
fn string(v: &Value, key: &str) -> String {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| panic!("no {key} in\n{}", pretty(v)))
        .to_string()
}
