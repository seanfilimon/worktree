//! REST authentication: `POST /api/v1/auth/login`, `/api/v1/auth/login/mfa`
//! and `/api/v1/auth/register`.
//!
//! A completed login returns `{token, account: {id, username, tenant_id, ...}}`.
//! For an account with MFA enabled the first step instead returns
//! `{mfa_required: true, mfa_token, account: {username}}`; the session is then
//! issued by exchanging that short-lived token plus a TOTP or recovery code
//! at `/auth/login/mfa` (ENGINEERING_PLAN §12.2). Errors use the server-wide
//! envelope `{"error": {"code": "...", "message": "..."}}`.
//!
//! Since v0.8 (§16.6) completed logins also carry `refresh_token` and
//! `expires_in`: `token` is an access token valid one hour, renewed with
//! the single-use refresh token at `POST /api/v1/auth/refresh`. Both are
//! saved in `credentials.toml`; [`fresh_credential`] renews an access token
//! that is about to expire and [`refresh`] renews one the server rejected,
//! so callers never see the expiry.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::credentials::CredentialStore;
use super::server_url::normalize_server_url;
use super::{Credential, LoginInfo, LoginOutcome, MfaChallenge};
use crate::error::{Result, SdkError};

const LOGIN_PATH: &str = "/api/v1/auth/login";
const LOGIN_MFA_PATH: &str = "/api/v1/auth/login/mfa";
const REGISTER_PATH: &str = "/api/v1/auth/register";
pub(crate) const REFRESH_PATH: &str = "/api/v1/auth/refresh";
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest raw (non-JSON) error body echoed back in an error message.
const MAX_ERROR_BODY: usize = 200;
/// Access tokens this close to their expiry are renewed before use.
const REFRESH_AHEAD: chrono::Duration = chrono::Duration::seconds(120);
/// How long to wait before re-reading the credential file when the server
/// rejected a refresh token another process may just have rotated.
const REFRESH_RACE_WAIT: Duration = Duration::from_millis(300);

/// Response of the auth endpoints. `token` is empty and `mfa_required` set
/// when the login needs a second factor.
#[derive(Debug, Deserialize)]
pub(crate) struct AuthResponse {
    #[serde(default)]
    pub(crate) token: String,
    #[serde(default)]
    mfa_required: bool,
    #[serde(default)]
    mfa_token: String,
    pub(crate) account: AccountJson,
    // --- device-auth (§16.6): token pairs ---
    #[serde(default)]
    pub(crate) refresh_token: String,
    /// Access token lifetime in seconds (0 = unknown / no expiry).
    #[serde(default)]
    pub(crate) expires_in: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccountJson {
    /// Absent from MFA challenges.
    #[serde(default)]
    id: String,
    pub(crate) username: String,
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
    let (status, text) = post_anonymous(&server, path, body).await?;

    if !(200..300).contains(&status) {
        let err = map_api_error(&server, status, &text);
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

/// POST JSON `body` to `server` (normalized) + `path` without credentials;
/// returns the status code and the body text.
pub(crate) async fn post_anonymous(
    server: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<(u16, String)> {
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
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|e| SdkError::Remote(format!("reading response from {server}: {e}")))?;
    Ok((status, text))
}

/// The credential to save for a completed login (`expires_in` seconds of
/// access token lifetime, 0 = unknown).
fn credential_from(username: String, resp: &AuthResponse) -> Credential {
    let mut cred = Credential::new(username, resp.token.clone());
    if !resp.refresh_token.is_empty() {
        cred.refresh_token = Some(resp.refresh_token.clone());
    }
    if resp.expires_in > 0 {
        cred.expires_at = Some(Utc::now() + chrono::Duration::seconds(resp.expires_in));
    }
    cred
}

/// Save the token of a completed login in `store` and describe the session.
pub(crate) fn save_session(
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
    let cred = credential_from(resp.account.username.clone(), &resp);
    let info = LoginInfo {
        server,
        // The personal tenant's slug is the username.
        tenant: resp.account.username.clone(),
        username: resp.account.username,
        account_id: resp.account.id,
        token: resp.token,
    };
    store.put(&info.server, &cred)?;
    Ok(info)
}

// --- device-auth (ENGINEERING_PLAN §16.6): refresh tokens ---

/// Whether `cred`'s access token expires within [`REFRESH_AHEAD`] and can
/// be renewed.
fn needs_refresh(cred: &Credential, now: DateTime<Utc>) -> bool {
    cred.refresh_token.is_some() && cred.expires_at.is_some_and(|at| at - now <= REFRESH_AHEAD)
}

/// The saved credential for `server`, renewed first when its access token
/// is about to expire. A failed renewal keeps the saved credential (the
/// request then fails as "not logged in" and [`refresh`] runs again).
pub(crate) async fn fresh_credential(
    store: &CredentialStore,
    server: &str,
) -> Result<Option<Credential>> {
    let Some(cred) = store.get(server)? else {
        return Ok(None);
    };
    if !needs_refresh(&cred, Utc::now()) {
        return Ok(Some(cred));
    }
    match refresh(store, server, Some(&cred.token)).await {
        Ok(Some(fresh)) => Ok(Some(fresh)),
        Ok(None) => Ok(Some(cred)),
        Err(err) => {
            tracing::debug!(%server, "refreshing the access token failed: {err}");
            Ok(Some(cred))
        }
    }
}

/// Renew the saved credential of `server` with its refresh token and save
/// the new pair. `failed` is the access token the server just rejected:
/// when the saved token already differs (another process renewed it) that
/// one is returned without a request. Returns `None` without a credential
/// or refresh token, or when the server rejects the refresh token (the
/// user must log in again). A rejected token that another process rotated
/// a moment ago is detected by re-reading the file.
pub(crate) async fn refresh(
    store: &CredentialStore,
    server: &str,
    failed: Option<&str>,
) -> Result<Option<Credential>> {
    let server = normalize_server_url(server)?;
    let Some(cred) = store.get(&server)? else {
        return Ok(None);
    };
    if failed.is_some_and(|f| f != cred.token) {
        return Ok(Some(cred));
    }
    let Some(refresh_token) = cred.refresh_token.clone() else {
        return Ok(None);
    };
    let body = serde_json::json!({ "refresh_token": refresh_token });
    let (status, text) = post_anonymous(&server, REFRESH_PATH, &body).await?;
    if (200..300).contains(&status) {
        let resp: AuthResponse = serde_json::from_str(&text).map_err(|e| {
            SdkError::Remote(format!(
                "unexpected response from {server}{REFRESH_PATH}: {e}"
            ))
        })?;
        if resp.token.is_empty() {
            return Err(SdkError::Remote(format!(
                "{server}{REFRESH_PATH} returned an empty token"
            )));
        }
        let fresh = credential_from(cred.username.clone(), &resp);
        store.put(&server, &fresh)?;
        tracing::debug!(%server, "renewed the access token");
        return Ok(Some(fresh));
    }
    if status == 401 || status == 400 {
        // Another process sharing the file may have rotated the token.
        tokio::time::sleep(REFRESH_RACE_WAIT).await;
        if let Some(now) = store.get(&server)? {
            if now.refresh_token.is_some() && now.refresh_token != cred.refresh_token {
                return Ok(Some(now));
            }
        }
        tracing::debug!(%server, "the refresh token was rejected: {}", summarize_body(&text));
        return Ok(None);
    }
    Err(map_api_error(&server, status, &text))
}

/// A bearer token that may be renewed through the credential store: the
/// token of a [`super::RestClient`]. Clones share the token.
#[derive(Debug, Clone)]
pub(crate) struct TokenCell {
    token: Arc<RwLock<Option<String>>>,
    expires_at: Arc<RwLock<Option<DateTime<Utc>>>>,
    /// The store and server to renew through; None = a fixed token.
    renew: Option<(CredentialStore, String)>,
    refreshable: bool,
}

impl TokenCell {
    /// A token that is never renewed (None = anonymous).
    pub(crate) fn fixed(token: Option<String>) -> Self {
        Self {
            token: Arc::new(RwLock::new(token)),
            expires_at: Arc::new(RwLock::new(None)),
            renew: None,
            refreshable: false,
        }
    }

    /// The saved credential `cred` of `server` in `store`, renewed through
    /// the store when it has a refresh token.
    pub(crate) fn saved(store: CredentialStore, server: &str, cred: &Credential) -> Self {
        Self {
            token: Arc::new(RwLock::new(Some(cred.token.clone()))),
            expires_at: Arc::new(RwLock::new(cred.expires_at)),
            renew: Some((store, server.to_string())),
            refreshable: cred.refresh_token.is_some(),
        }
    }

    /// The current token.
    pub(crate) fn get(&self) -> Option<String> {
        self.token.read().map(|t| t.clone()).unwrap_or_default()
    }

    fn set(&self, cred: &Credential) {
        if let Ok(mut t) = self.token.write() {
            *t = Some(cred.token.clone());
        }
        if let Ok(mut e) = self.expires_at.write() {
            *e = cred.expires_at;
        }
    }

    /// Renew the token ahead of its expiry (best effort).
    pub(crate) async fn ensure_fresh(&self) {
        let Some((store, server)) = &self.renew else {
            return;
        };
        let expiring = self
            .expires_at
            .read()
            .ok()
            .and_then(|e| *e)
            .is_some_and(|at| at - Utc::now() <= REFRESH_AHEAD);
        if !self.refreshable || !expiring {
            return;
        }
        let current = self.get();
        match refresh(store, server, current.as_deref()).await {
            Ok(Some(cred)) => self.set(&cred),
            Ok(None) => {}
            Err(err) => tracing::debug!(%server, "refreshing the access token failed: {err}"),
        }
    }

    /// After the server rejected `failed`: renew the token. Returns whether
    /// a different token is now available (retry the request with it).
    pub(crate) async fn renew_after(&self, failed: &str) -> Result<bool> {
        let Some((store, server)) = &self.renew else {
            return Ok(false);
        };
        match refresh(store, server, Some(failed)).await? {
            Some(cred) if cred.token != failed => {
                self.set(&cred);
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

// --- end device-auth ---

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
        Ok(env) => (env.error.code, env.error.message),
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

    // --- device-auth (§16.6): refresh tokens ---

    /// A fake refresh endpoint rotating wtrt_N -> (tok-N+1, wtrt_N+1); any
    /// other token is rejected with invalid_grant.
    pub(crate) async fn refresh_server() -> String {
        async fn refresh(Json(body): Json<Value>) -> Response {
            let presented = body["refresh_token"].as_str().unwrap_or_default();
            match presented
                .strip_prefix("wtrt_")
                .and_then(|n| n.parse::<u32>().ok())
            {
                Some(n) if n < 100 => Json(json!({
                    "token": format!("tok-{}", n + 1),
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "refresh_token": format!("wtrt_{}", n + 1),
                    "refresh_expires_in": 2592000,
                    "account": {"id": "a1", "username": "ada"}
                }))
                .into_response(),
                _ => (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"code": "invalid_grant", "message": "invalid or expired refresh token"}})),
                )
                    .into_response(),
            }
        }
        let app = Router::new().route(REFRESH_PATH, post(refresh));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        format!("http://{addr}")
    }

    fn pair(token: &str, refresh: &str, expires_in_secs: i64) -> Credential {
        let mut c = Credential::new("ada", token);
        c.refresh_token = Some(refresh.into());
        c.expires_at = Some(Utc::now() + chrono::Duration::seconds(expires_in_secs));
        c
    }

    #[test]
    fn login_responses_with_refresh_tokens_are_saved_as_pairs() {
        let (_dir, store) = temp_store();
        let body = r#"{"token":"wt_a","account":{"id":"a1","username":"ada"},"token_type":"Bearer","expires_in":3600,"refresh_token":"wtrt_r","refresh_expires_in":2592000}"#;
        let resp: AuthResponse = serde_json::from_str(body).unwrap();
        save_session(&store, SERVER.into(), LOGIN_PATH, resp).unwrap();
        let cred = store.get(SERVER).unwrap().unwrap();
        assert_eq!(cred.refresh_token.as_deref(), Some("wtrt_r"));
        let left = cred.expires_at.unwrap() - Utc::now();
        assert!(left > chrono::Duration::minutes(59) && left <= chrono::Duration::hours(1));
        // Older servers: no refresh token, no expiry.
        let resp: AuthResponse =
            serde_json::from_str(r#"{"token":"wt_b","account":{"id":"a1","username":"ada"}}"#)
                .unwrap();
        save_session(&store, SERVER.into(), LOGIN_PATH, resp).unwrap();
        assert_eq!(
            store.get(SERVER).unwrap().unwrap(),
            Credential::new("ada", "wt_b")
        );
    }

    #[tokio::test]
    async fn refresh_rotates_and_saves_the_pair() {
        let server = refresh_server().await;
        let (_dir, store) = temp_store();
        assert_eq!(
            refresh(&store, &server, None).await.unwrap(),
            None,
            "no credential"
        );
        store
            .put(&server, &Credential::new("ada", "wt_plain"))
            .unwrap();
        assert_eq!(
            refresh(&store, &server, None).await.unwrap(),
            None,
            "no refresh token"
        );

        store.put(&server, &pair("tok-1", "wtrt_1", 3600)).unwrap();
        let fresh = refresh(&store, &server, Some("tok-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fresh.token, "tok-2");
        assert_eq!(fresh.refresh_token.as_deref(), Some("wtrt_2"));
        assert_eq!(store.get(&server).unwrap().unwrap(), fresh);

        // Another process renewed already: the saved token is used as is.
        let again = refresh(&store, &server, Some("tok-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(again.token, "tok-2");

        // A rejected refresh token means "log in again".
        store
            .put(&server, &pair("tok-x", "wtrt_bad", 3600))
            .unwrap();
        assert_eq!(refresh(&store, &server, Some("tok-x")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn fresh_credential_renews_tokens_about_to_expire() {
        let server = refresh_server().await;
        let (_dir, store) = temp_store();
        store.put(&server, &pair("tok-5", "wtrt_5", 3600)).unwrap();
        assert_eq!(
            fresh_credential(&store, &server)
                .await
                .unwrap()
                .unwrap()
                .token,
            "tok-5",
            "valid for an hour: kept"
        );
        store.put(&server, &pair("tok-5", "wtrt_5", 30)).unwrap();
        assert_eq!(
            fresh_credential(&store, &server)
                .await
                .unwrap()
                .unwrap()
                .token,
            "tok-6",
            "expiring: renewed"
        );
        // A failed renewal keeps the saved credential.
        store.put(&server, &pair("tok-7", "wtrt_bad", -10)).unwrap();
        assert_eq!(
            fresh_credential(&store, &server)
                .await
                .unwrap()
                .unwrap()
                .token,
            "tok-7"
        );
    }

    #[tokio::test]
    async fn token_cell_renews_through_the_store() {
        let server = refresh_server().await;
        let (_dir, store) = temp_store();
        let cred = pair("tok-1", "wtrt_1", 3600);
        store.put(&server, &cred).unwrap();
        let cell = TokenCell::saved(store.clone(), &server, &cred);
        let clone = cell.clone();
        cell.ensure_fresh().await;
        assert_eq!(cell.get().as_deref(), Some("tok-1"), "not expiring yet");
        assert!(cell.renew_after("tok-1").await.unwrap());
        assert_eq!(
            clone.get().as_deref(),
            Some("tok-2"),
            "clones share the token"
        );
        assert!(TokenCell::fixed(Some("x".into()))
            .renew_after("x")
            .await
            .is_ok_and(|r| !r));
    }
}
