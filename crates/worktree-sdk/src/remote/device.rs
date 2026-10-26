//! OAuth 2.0 device authorization grant (RFC 8628) for `wt login --device`
//! (ENGINEERING_PLAN §16.6).
//!
//! [`start`] asks the server for a device code and a user code
//! (`POST /api/v1/auth/device`); the user approves the code in the admin
//! panel (`/admin/device`) while [`wait`] polls
//! `POST /api/v1/auth/device/token` every `interval` seconds. The poll
//! answers `authorization_pending` until the user decides, `slow_down` when
//! polled too fast (the interval grows by 5 seconds), `access_denied`,
//! `expired_token`, or the token pair, which is saved like a password login.

use std::time::Duration;

use serde::Deserialize;

use super::auth::{map_api_error, post_anonymous, save_session, AuthResponse};
use super::credentials::CredentialStore;
use super::server_url::normalize_server_url;
use super::LoginInfo;
use crate::error::{Result, SdkError};

const DEVICE_PATH: &str = "/api/v1/auth/device";
const DEVICE_TOKEN_PATH: &str = "/api/v1/auth/device/token";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Added to the polling interval on `slow_down` (RFC 8628 §3.5).
/// (One second in unit tests, to keep them fast.)
const SLOW_DOWN_STEP: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 5 });
/// Bounds on the server-announced interval and lifetime.
const MIN_INTERVAL: Duration = Duration::from_secs(1);
const MAX_INTERVAL: Duration = Duration::from_secs(60);
const MAX_EXPIRES_IN: u64 = 3600;

/// A started device authorization: show `user_code` and
/// `verification_uri` (or open `verification_uri_complete`), then poll.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceAuthorization {
    /// Normalized server URL (not part of the server's answer).
    #[serde(skip)]
    pub server: String,
    /// Secret the device polls with; never show it.
    pub device_code: String,
    /// `XXXX-XXXX`, for the user to confirm in the browser.
    pub user_code: String,
    pub verification_uri: String,
    /// The verification URL with the user code filled in.
    #[serde(default)]
    pub verification_uri_complete: String,
    /// Seconds until the codes expire.
    pub expires_in: u64,
    /// Seconds between polls.
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// What [`wait`] reports before each poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePoll {
    /// Waiting for the user; the next poll follows after `interval`.
    Waiting { interval: Duration },
    /// The server asked to poll more slowly; the interval grew.
    SlowedDown { interval: Duration },
}

/// `POST /api/v1/auth/device`.
pub(crate) async fn start(
    server: &str,
    client_name: &str,
    scopes: &[String],
) -> Result<DeviceAuthorization> {
    let server = normalize_server_url(server)?;
    let body = serde_json::json!({
        "client_id": "wt-cli",
        "client_name": client_name,
        "scopes": scopes,
    });
    let (status, text) = post_anonymous(&server, DEVICE_PATH, &body).await?;
    if !(200..300).contains(&status) {
        return Err(map_api_error(&server, status, &text));
    }
    let mut auth: DeviceAuthorization = serde_json::from_str(&text).map_err(|e| {
        SdkError::Remote(format!(
            "unexpected response from {server}{DEVICE_PATH}: {e}"
        ))
    })?;
    if auth.device_code.is_empty() || auth.user_code.is_empty() {
        return Err(SdkError::Remote(format!(
            "{server}{DEVICE_PATH} returned no device or user code"
        )));
    }
    if auth.verification_uri_complete.is_empty() {
        auth.verification_uri_complete = auth.verification_uri.clone();
    }
    auth.server = server;
    Ok(auth)
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

/// Outcome of one poll.
enum Poll {
    Pending,
    SlowDown,
    Done(Box<AuthResponse>),
}

async fn poll_once(auth: &DeviceAuthorization) -> Result<Poll> {
    let body = serde_json::json!({ "device_code": auth.device_code, "grant_type": GRANT_TYPE });
    let (status, text) = post_anonymous(&auth.server, DEVICE_TOKEN_PATH, &body).await?;
    if (200..300).contains(&status) {
        let resp: AuthResponse = serde_json::from_str(&text).map_err(|e| {
            SdkError::Remote(format!(
                "unexpected response from {}{DEVICE_TOKEN_PATH}: {e}",
                auth.server
            ))
        })?;
        return Ok(Poll::Done(Box::new(resp)));
    }
    let err = serde_json::from_str::<ErrorEnvelope>(&text)
        .ok()
        .map(|e| e.error);
    match err.as_ref().map(|e| e.code.as_str()) {
        Some("authorization_pending") => Ok(Poll::Pending),
        Some("slow_down") => Ok(Poll::SlowDown),
        Some("access_denied") => Err(SdkError::PermissionDenied(
            "the device login was denied in the browser".into(),
        )),
        Some("expired_token") | Some("invalid_grant") => Err(SdkError::Remote(format!(
            "the device code expired before it was approved; run `wt login --device {}` again",
            auth.server
        ))),
        _ => Err(match err {
            Some(e) if !e.message.is_empty() && status == 400 => {
                SdkError::Remote(format!("{} ({})", e.message, e.code))
            }
            _ => map_api_error(&auth.server, status, &text),
        }),
    }
}

/// Poll until the user decides or the codes expire; saves the credential.
pub(crate) async fn wait(
    store: &CredentialStore,
    auth: &DeviceAuthorization,
    mut on_poll: impl FnMut(DevicePoll),
) -> Result<LoginInfo> {
    let mut interval = Duration::from_secs(auth.interval).clamp(MIN_INTERVAL, MAX_INTERVAL);
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(auth.expires_in.clamp(1, MAX_EXPIRES_IN));
    let mut event = DevicePoll::Waiting { interval };
    loop {
        on_poll(event);
        tokio::time::sleep(interval).await;
        if tokio::time::Instant::now() >= deadline {
            return Err(SdkError::Remote(format!(
                "the device code expired before it was approved; run `wt login --device {}` again",
                auth.server
            )));
        }
        match poll_once(auth).await? {
            Poll::Pending => event = DevicePoll::Waiting { interval },
            Poll::SlowDown => {
                interval = (interval + SLOW_DOWN_STEP).min(MAX_INTERVAL);
                event = DevicePoll::SlowedDown { interval };
            }
            Poll::Done(resp) => {
                return save_session(store, auth.server.clone(), DEVICE_TOKEN_PATH, *resp)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::{json, Value};

    use super::*;

    fn pending(code: &str) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"code": code, "message": code}})),
        )
            .into_response()
    }

    /// A fake server: the first poll is pending, the second too fast, the
    /// third approved — unless the client name is "deny-me".
    async fn fake_server() -> String {
        let polls = Arc::new(AtomicUsize::new(0));
        let denied = Arc::new(std::sync::Mutex::new(false));
        let d2 = denied.clone();
        let app = Router::new()
            .route(
                DEVICE_PATH,
                post(move |Json(body): Json<Value>| {
                    let denied = d2.clone();
                    async move {
                        *denied.lock().unwrap() = body["client_name"] == "deny-me";
                        assert_eq!(body["client_id"], "wt-cli");
                        Json(json!({
                            "device_code": "wtd_secret",
                            "user_code": "BCDF-GHJK",
                            "verification_uri": "http://srv/admin/device",
                            "verification_uri_complete": "http://srv/admin/device?code=BCDF-GHJK",
                            "expires_in": 600,
                            "interval": 1,
                        }))
                    }
                }),
            )
            .route(
                DEVICE_TOKEN_PATH,
                post(move |Json(body): Json<Value>| {
                    let polls = polls.clone();
                    let denied = denied.clone();
                    async move {
                        assert_eq!(body["device_code"], "wtd_secret");
                        assert_eq!(body["grant_type"], GRANT_TYPE);
                        if *denied.lock().unwrap() {
                            return pending("access_denied");
                        }
                        match polls.fetch_add(1, Ordering::SeqCst) {
                            0 => pending("slow_down"),
                            1 => pending("authorization_pending"),
                            _ => Json(json!({
                                "token": "wt_access",
                                "token_type": "Bearer",
                                "expires_in": 3600,
                                "refresh_token": "wtrt_refresh",
                                "refresh_expires_in": 2592000,
                                "account": {"id": "a1", "username": "ada"}
                            }))
                            .into_response(),
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn device_login_polls_until_approved_and_saves_the_pair() {
        let server = fake_server().await;
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::new(dir.path().join("c.toml"));
        let auth = start(&server, "wt on test", &[]).await.unwrap();
        assert_eq!(auth.user_code, "BCDF-GHJK");
        assert_eq!(auth.server, server);
        assert!(auth.verification_uri_complete.ends_with("?code=BCDF-GHJK"));

        let mut events = Vec::new();
        let info = wait(&store, &auth, |e| events.push(e)).await.unwrap();
        assert_eq!(info.username, "ada");
        assert_eq!(info.token, "wt_access");
        assert_eq!(
            events,
            vec![
                DevicePoll::Waiting {
                    interval: Duration::from_secs(1)
                },
                DevicePoll::SlowedDown {
                    interval: Duration::from_secs(2)
                },
                DevicePoll::Waiting {
                    interval: Duration::from_secs(2)
                },
            ]
        );
        let cred = store.get(&server).unwrap().unwrap();
        assert_eq!(cred.refresh_token.as_deref(), Some("wtrt_refresh"));
        assert!(cred.expires_at.is_some());
    }

    #[tokio::test]
    async fn denied_device_login_is_permission_denied() {
        let server = fake_server().await;
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::new(dir.path().join("c.toml"));
        let auth = start(&server, "deny-me", &[]).await.unwrap();
        let err = wait(&store, &auth, |_| {}).await.unwrap_err();
        assert!(
            matches!(err, SdkError::PermissionDenied(ref m) if m.contains("denied")),
            "{err:?}"
        );
        assert!(store.get(&server).unwrap().is_none());
    }
}
