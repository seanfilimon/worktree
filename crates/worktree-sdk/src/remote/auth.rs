//! REST authentication: `POST /api/v1/auth/login`, `/api/v1/auth/login/mfa`
//! and `/api/v1/auth/register`.
//!
//! A completed login returns `{token, account: {id, username, tenant_id, ...}}`.
//! For an account with MFA enabled the first step instead returns
//! `{mfa_required: true, mfa_token, account: {username}}`; the session is then
//! issued by exchanging that short-lived token plus a TOTP or recovery code
//! at `/auth/login/mfa` (ENGINEERING_PLAN §12.2). Errors use the server-wide
//! envelope `{"error": {"code": "...", "message": "..."}}`.

use std::time::Duration;

use serde::Deserialize;

use super::credentials::CredentialStore;
use super::server_url::normalize_server_url;
use super::{Credential, LoginInfo, LoginOutcome, MfaChallenge};
use crate::error::{Result, SdkError};

const LOGIN_PATH: &str = "/api/v1/auth/login";
const LOGIN_MFA_PATH: &str = "/api/v1/auth/login/mfa";
const REGISTER_PATH: &str = "/api/v1/auth/register";
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest raw (non-JSON) error body echoed back in an error message.
const MAX_ERROR_BODY: usize = 200;

/// Response of the auth endpoints. `token` is empty and `mfa_required` set
/// when the login needs a second factor.
#[derive(Debug, Deserialize)]
struct AuthResponse {
    #[serde(default)]
    token: String,
    #[serde(default)]
    mfa_required: bool,
    #[serde(default)]
    mfa_token: String,
    account: AccountJson,
}

#[derive(Debug, Deserialize)]
struct AccountJson {
    /// Absent from MFA challenges.
    #[serde(default)]
    id: String,
    username: String,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
    /// tenant-lifecycle (v0.8): e.g. `tenant_moved` {old_slug, new_slug}.
    #[serde(default)]
    details: std::collections::HashMap<String, String>,
}

/// Log in to an account without a second factor and save the token in
/// `store`. Accounts with MFA enabled fail with an error saying so; they
/// need [`login_start`] + [`login_mfa`].
pub(crate) async fn login(
    store: &CredentialStore,
    server: &str,
    username: &str,
    password: &str,
) -> Result<LoginInfo> {
    match login_start(store, server, username, password).await? {
        LoginOutcome::LoggedIn(info) => Ok(info),
        LoginOutcome::MfaRequired(challenge) => Err(SdkError::PermissionDenied(format!(
            "account '{}' has two-factor authentication (MFA) enabled; \
             logging in needs a TOTP or recovery code",
            challenge.username
        ))),
    }
}

/// First login step: a session (saved in `store`) or an MFA challenge to
/// complete with [`login_mfa`].
pub(crate) async fn login_start(
    store: &CredentialStore,
    server: &str,
    username: &str,
    password: &str,
) -> Result<LoginOutcome> {
    require_non_empty("username", username)?;
    require_non_empty("password", password)?;
    let body = serde_json::json!({ "username": username, "password": password });
    let (server, resp) = post_auth(server, LOGIN_PATH, &body).await?;
    if !resp.mfa_required {
        return save_session(store, server, LOGIN_PATH, resp).map(LoginOutcome::LoggedIn);
    }
    if resp.mfa_token.is_empty() {
        return Err(SdkError::Remote(format!(
            "{server}{LOGIN_PATH} asked for MFA without an mfa_token"
        )));
    }
    Ok(LoginOutcome::MfaRequired(MfaChallenge {
        server,
        username: resp.account.username,
        mfa_token: resp.mfa_token,
    }))
}

/// Second login step: exchange `challenge` plus a TOTP or recovery code for
/// a session and save it in `store`.
pub(crate) async fn login_mfa(
    store: &CredentialStore,
    challenge: &MfaChallenge,
    code: &str,
) -> Result<LoginInfo> {
    require_non_empty("code", code)?;
    let body = serde_json::json!({ "mfa_token": challenge.mfa_token, "code": code.trim() });
    let (server, resp) = post_auth(&challenge.server, LOGIN_MFA_PATH, &body).await?;
    save_session(store, server, LOGIN_MFA_PATH, resp)
}

/// Register a new account and save the returned token in `store`.
pub(crate) async fn register(
    store: &CredentialStore,
    server: &str,
    username: &str,
    email: &str,
    password: &str,
) -> Result<LoginInfo> {
    require_non_empty("username", username)?;
    require_non_empty("email", email)?;
    require_non_empty("password", password)?;
    let body = serde_json::json!({ "username": username, "email": email, "password": password });
    let (server, resp) = post_auth(server, REGISTER_PATH, &body).await?;
    save_session(store, server, REGISTER_PATH, resp)
}

fn require_non_empty(what: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(SdkError::InvalidArgument(format!(
            "{what} must not be empty"
        )));
    }
    Ok(())
}

/// POST `body` to an auth endpoint; returns the normalized server URL and
/// the parsed response.
async fn post_auth(
    server: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<(String, AuthResponse)> {
    let server = normalize_server_url(server)?;
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(concat!("worktree-sdk/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| SdkError::Remote(format!("cannot create HTTP client: {e}")))?;

    let response = client
        .post(format!("{server}{path}"))
        .json(body)
        .send()
        .await
        .map_err(|e| {
            SdkError::Remote(format!(
                "cannot reach {server}: {}",
                super::client::error_chain(&e)
            ))
        })?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| SdkError::Remote(format!("reading response from {server}: {e}")))?;

    if !status.is_success() {
        let err = map_api_error(&server, status.as_u16(), &text);
        // On the auth endpoints 401 means "bad credentials", not "you need
        // to log in".
        return Err(match err {
            SdkError::NotLoggedIn(_) => SdkError::PermissionDenied(format!(
                "authentication failed: {}",
                api_error_message(&text).unwrap_or_else(|| "invalid credentials".into())
            )),
            other => other,
        });
    }

    let parsed: AuthResponse = serde_json::from_str(&text)
        .map_err(|e| SdkError::Remote(format!("unexpected response from {server}{path}: {e}")))?;
    Ok((server, parsed))
}

/// Save the token of a completed login in `store` and describe the session.
fn save_session(
    store: &CredentialStore,
    server: String,
    path: &str,
    resp: AuthResponse,
) -> Result<LoginInfo> {
    if resp.token.is_empty() {
        return Err(SdkError::Remote(format!(
            "{server}{path} returned an empty token"
        )));
    }
    let info = LoginInfo {
        server,
        // The personal tenant's slug is the username.
        tenant: resp.account.username.clone(),
        username: resp.account.username,
        account_id: resp.account.id,
        token: resp.token,
    };
    store.put(
        &info.server,
        &Credential {
            username: info.username.clone(),
            token: info.token.clone(),
        },
    )?;
    Ok(info)
}

fn api_error_message(body: &str) -> Option<String> {
    serde_json::from_str::<ErrorEnvelope>(body)
        .ok()
        .map(|e| e.error.message)
        .filter(|m| !m.is_empty())
}

/// Map a non-2xx REST response to an [`SdkError`].
///
/// `unauthenticated`/401 → `NotLoggedIn`, `forbidden`/403 →
/// `PermissionDenied`, everything else → `Remote` with the server's code and
/// message. Bodies that are not the JSON error envelope are summarized.
pub(crate) fn map_api_error(server: &str, status: u16, body: &str) -> SdkError {
    let (code, message) = match serde_json::from_str::<ErrorEnvelope>(body) {
        Ok(mut env) => {
            // tenant-lifecycle (v0.8 §16.4): renamed tenants.
            if env.error.code == "tenant_moved" {
                if let Some(new) = env.error.details.remove("new_slug") {
                    let old = env.error.details.remove("old_slug").unwrap_or_default();
                    return SdkError::TenantMoved { old, new };
                }
            }
            (env.error.code, env.error.message)
        }
        Err(_) => (String::new(), summarize_body(body)),
    };
    if code == "unauthenticated" || status == 401 {
        return SdkError::NotLoggedIn(server.to_string());
    }
    if code == "forbidden" || status == 403 {
        return SdkError::PermissionDenied(if message.is_empty() {
            "forbidden".into()
        } else {
            message
        });
    }
    let code = if code.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("{code}, HTTP {status}")
    };
    if message.is_empty() {
        SdkError::Remote(format!("{server}: request failed ({code})"))
    } else {
        SdkError::Remote(format!("{message} ({code})"))
    }
}

fn summarize_body(body: &str) -> String {
    let trimmed = body.trim();
    match trimmed.char_indices().nth(MAX_ERROR_BODY) {
        Some((idx, _)) => format!("{}...", &trimmed[..idx]),
        None => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::{json, Value};

    use super::*;

    const SERVER: &str = "http://localhost:7420";

    fn envelope(code: &str, message: &str) -> String {
        serde_json::json!({ "error": { "code": code, "message": message } }).to_string()
    }

    /// A fake auth server: "plain" logs in directly, "mfa" needs the code
    /// 123456 (or the recovery code abcd-efgh) via `/auth/login/mfa`.
    async fn fake_server() -> String {
        async fn login(Json(body): Json<Value>) -> Response {
            match (body["username"].as_str(), body["password"].as_str()) {
                (Some("plain"), Some("pw")) => Json(json!({
                    "token": "tok-plain",
                    "account": {"id": "a1", "username": "plain", "tenant_id": "t1"}
                }))
                .into_response(),
                (Some("mfa"), Some("pw")) => Json(json!({
                    "mfa_required": true,
                    "mfa_token": "mfa-tok",
                    "account": {"username": "mfa"}
                }))
                .into_response(),
                _ => unauthorized("invalid username or password"),
            }
        }
        async fn login_mfa(Json(body): Json<Value>) -> Response {
            let code_ok = matches!(body["code"].as_str(), Some("123456" | "abcd-efgh"));
            if body["mfa_token"] == "mfa-tok" && code_ok {
                Json(json!({"token": "tok-mfa", "account": {"id": "a2", "username": "mfa"}}))
                    .into_response()
            } else {
                unauthorized("invalid MFA code")
            }
        }
        fn unauthorized(message: &str) -> Response {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": {"code": "unauthenticated", "message": message}})),
            )
                .into_response()
        }
        let app = Router::new()
            .route(LOGIN_PATH, post(login))
            .route(LOGIN_MFA_PATH, post(login_mfa));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        format!("http://{addr}")
    }

    fn temp_store() -> (tempfile::TempDir, CredentialStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::new(dir.path().join("c.toml"));
        (dir, store)
    }

    #[test]
    fn unauthenticated_maps_to_not_logged_in() {
        match map_api_error(SERVER, 401, &envelope("unauthenticated", "token expired")) {
            SdkError::NotLoggedIn(s) => assert_eq!(s, SERVER),
            other => panic!("{other:?}"),
        }
        // Status alone is enough.
        assert!(matches!(
            map_api_error(SERVER, 401, "nope"),
            SdkError::NotLoggedIn(_)
        ));
    }

    #[test]
    fn forbidden_maps_to_permission_denied() {
        match map_api_error(
            SERVER,
            403,
            &envelope("forbidden", "registration is disabled"),
        ) {
            SdkError::PermissionDenied(m) => assert_eq!(m, "registration is disabled"),
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(map_api_error(SERVER, 403, ""), SdkError::PermissionDenied(m) if m == "forbidden")
        );
    }

    #[test]
    fn other_codes_map_to_remote_with_code_and_message() {
        match map_api_error(SERVER, 409, &envelope("conflict", "username already taken")) {
            SdkError::Remote(m) => {
                assert!(m.contains("username already taken"), "{m}");
                assert!(m.contains("conflict"), "{m}");
                assert!(m.contains("409"), "{m}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn non_json_bodies_are_summarized() {
        let long = "x".repeat(1000);
        match map_api_error(SERVER, 502, &long) {
            SdkError::Remote(m) => {
                assert!(m.len() < 300, "{m}");
                assert!(m.contains("HTTP 502"), "{m}");
            }
            other => panic!("{other:?}"),
        }
        match map_api_error(SERVER, 500, "") {
            SdkError::Remote(m) => assert!(m.contains(SERVER) && m.contains("HTTP 500"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn maps_tenant_moved() {
        let body = r#"{"error":{"code":"tenant_moved","message":"renamed","details":{"old_slug":"acme","new_slug":"acme-inc"}}}"#;
        match map_api_error(SERVER, 301, body) {
            SdkError::TenantMoved { old, new } => {
                assert_eq!((old.as_str(), new.as_str()), ("acme", "acme-inc"))
            }
            other => panic!("{other:?}"),
        }
        // Without details it stays a generic remote error.
        match map_api_error(SERVER, 301, &envelope("tenant_moved", "renamed")) {
            SdkError::Remote(m) => assert!(m.contains("tenant_moved"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn summarize_respects_char_boundaries() {
        let s = "é".repeat(500);
        let out = summarize_body(&s);
        assert!(out.ends_with("..."));
        assert_eq!(out.chars().count(), MAX_ERROR_BODY + 3);
    }

    #[test]
    fn parses_auth_response_ignoring_extra_fields() {
        let body = r#"{"token":"t","account":{"id":"a1","username":"alice","email":"a@x","tenant_id":"t1","is_admin":false}}"#;
        let parsed: AuthResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.token, "t");
        assert!(!parsed.mfa_required);
        assert_eq!(parsed.account.id, "a1");
        assert_eq!(parsed.account.username, "alice");
    }

    #[test]
    fn parses_mfa_challenge() {
        let body = r#"{"mfa_required":true,"mfa_token":"m","account":{"username":"alice"}}"#;
        let parsed: AuthResponse = serde_json::from_str(body).unwrap();
        assert!(parsed.mfa_required);
        assert_eq!(parsed.mfa_token, "m");
        assert!(parsed.token.is_empty() && parsed.account.id.is_empty());
    }

    #[tokio::test]
    async fn empty_inputs_are_rejected_before_any_request() {
        let (_dir, store) = temp_store();
        // Port 1 is never listening; validation must fail first.
        let err = login(&store, "http://127.0.0.1:1", "", "pw")
            .await
            .unwrap_err();
        assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
        let err = register(&store, "http://127.0.0.1:1", "u", " ", "pw")
            .await
            .unwrap_err();
        assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
        let challenge = MfaChallenge {
            server: "http://127.0.0.1:1".into(),
            username: "u".into(),
            mfa_token: "m".into(),
        };
        let err = login_mfa(&store, &challenge, "  ").await.unwrap_err();
        assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
    }

    #[tokio::test]
    async fn unreachable_server_is_a_remote_error() {
        let (_dir, store) = temp_store();
        let err = login(&store, "http://127.0.0.1:1", "u", "pw")
            .await
            .unwrap_err();
        assert!(
            matches!(err, SdkError::Remote(ref m) if m.contains("cannot reach")),
            "{err:?}"
        );
        assert!(!store.path().exists());
    }

    #[tokio::test]
    async fn login_without_mfa_saves_the_session() {
        let server = fake_server().await;
        let (_dir, store) = temp_store();
        let LoginOutcome::LoggedIn(info) =
            login_start(&store, &server, "plain", "pw").await.unwrap()
        else {
            panic!("expected a session")
        };
        assert_eq!(
            (info.username.as_str(), info.account_id.as_str()),
            ("plain", "a1")
        );
        assert_eq!(store.get(&server).unwrap().unwrap().token, "tok-plain");
        // The single-step API keeps working for such accounts.
        let info = login(&store, &server, "plain", "pw").await.unwrap();
        assert_eq!(info.token, "tok-plain");
    }

    #[tokio::test]
    async fn mfa_login_needs_the_second_step() {
        let server = fake_server().await;
        let (_dir, store) = temp_store();
        let LoginOutcome::MfaRequired(challenge) =
            login_start(&store, &server, "mfa", "pw").await.unwrap()
        else {
            panic!("expected an MFA challenge")
        };
        assert_eq!(challenge.server, server);
        assert_eq!(challenge.username, "mfa");
        assert_eq!(challenge.mfa_token, "mfa-tok");
        assert!(store.get(&server).unwrap().is_none(), "nothing saved yet");

        match login_mfa(&store, &challenge, "000000").await.unwrap_err() {
            SdkError::PermissionDenied(m) => assert!(m.contains("invalid MFA code"), "{m}"),
            other => panic!("{other:?}"),
        }
        let info = login_mfa(&store, &challenge, " 123456\n").await.unwrap();
        assert_eq!(
            (info.username.as_str(), info.token.as_str()),
            ("mfa", "tok-mfa")
        );
        assert_eq!(store.get(&server).unwrap().unwrap().token, "tok-mfa");
        let info = login_mfa(&store, &challenge, "abcd-efgh").await.unwrap();
        assert_eq!(info.account_id, "a2");
    }

    #[tokio::test]
    async fn single_step_login_explains_mfa() {
        let server = fake_server().await;
        let (_dir, store) = temp_store();
        match login(&store, &server, "mfa", "pw").await.unwrap_err() {
            SdkError::PermissionDenied(m) => assert!(m.contains("MFA"), "{m}"),
            other => panic!("{other:?}"),
        }
        match login(&store, &server, "plain", "wrong").await.unwrap_err() {
            SdkError::PermissionDenied(m) => assert!(m.contains("invalid username"), "{m}"),
            other => panic!("{other:?}"),
        }
        assert!(!store.path().exists());
    }
}
