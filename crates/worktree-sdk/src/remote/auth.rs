//! REST authentication: `POST /api/v1/auth/login` and `/api/v1/auth/register`.
//!
//! Both return `{token, account: {id, username, tenant_id, ...}}`; errors use
//! the server-wide envelope `{"error": {"code": "...", "message": "..."}}`.

use std::time::Duration;

use serde::Deserialize;

use super::credentials::CredentialStore;
use super::server_url::normalize_server_url;
use super::{Credential, LoginInfo};
use crate::error::{Result, SdkError};

const LOGIN_PATH: &str = "/api/v1/auth/login";
const REGISTER_PATH: &str = "/api/v1/auth/register";
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest raw (non-JSON) error body echoed back in an error message.
const MAX_ERROR_BODY: usize = 200;

#[derive(Debug, Deserialize)]
struct AuthResponse {
    token: String,
    account: AccountJson,
}

#[derive(Debug, Deserialize)]
struct AccountJson {
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
}

/// Log in and save the returned token in `store`.
pub(crate) async fn login(
    store: &CredentialStore,
    server: &str,
    username: &str,
    password: &str,
) -> Result<LoginInfo> {
    require_non_empty("username", username)?;
    require_non_empty("password", password)?;
    let body = serde_json::json!({ "username": username, "password": password });
    authenticate(store, server, LOGIN_PATH, &body).await
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
    authenticate(store, server, REGISTER_PATH, &body).await
}

fn require_non_empty(what: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(SdkError::InvalidArgument(format!(
            "{what} must not be empty"
        )));
    }
    Ok(())
}

async fn authenticate(
    store: &CredentialStore,
    server: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<LoginInfo> {
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
        // On the auth endpoints 401 means "bad username or password", not
        // "you need to log in".
        return Err(match err {
            SdkError::NotLoggedIn(_) => SdkError::PermissionDenied(format!(
                "authentication failed: {}",
                api_error_message(&text).unwrap_or_else(|| "invalid username or password".into())
            )),
            other => other,
        });
    }

    let parsed: AuthResponse = serde_json::from_str(&text)
        .map_err(|e| SdkError::Remote(format!("unexpected response from {server}{path}: {e}")))?;
    if parsed.token.is_empty() {
        return Err(SdkError::Remote(format!(
            "{server}{path} returned an empty token"
        )));
    }

    let info = LoginInfo {
        server: server.clone(),
        // The personal tenant's slug is the username.
        tenant: parsed.account.username.clone(),
        username: parsed.account.username,
        account_id: parsed.account.id,
        token: parsed.token,
    };
    store.put(
        &server,
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
    use super::*;

    const SERVER: &str = "http://localhost:7420";

    fn envelope(code: &str, message: &str) -> String {
        serde_json::json!({ "error": { "code": code, "message": message } }).to_string()
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
        assert_eq!(parsed.account.id, "a1");
        assert_eq!(parsed.account.username, "alice");
    }

    #[tokio::test]
    async fn empty_inputs_are_rejected_before_any_request() {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::new(dir.path().join("c.toml"));
        // Port 1 is never listening; validation must fail first.
        let err = login(&store, "http://127.0.0.1:1", "", "pw")
            .await
            .unwrap_err();
        assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
        let err = register(&store, "http://127.0.0.1:1", "u", " ", "pw")
            .await
            .unwrap_err();
        assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
    }

    #[tokio::test]
    async fn unreachable_server_is_a_remote_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::new(dir.path().join("c.toml"));
        let err = login(&store, "http://127.0.0.1:1", "u", "pw")
            .await
            .unwrap_err();
        assert!(
            matches!(err, SdkError::Remote(ref m) if m.contains("cannot reach")),
            "{err:?}"
        );
        assert!(!store.path().exists());
    }
}
