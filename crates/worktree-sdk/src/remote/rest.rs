//! Typed JSON client for the server's REST API (`/api/v1`, ENGINEERING_PLAN
//! §7, §11, §12).
//!
//! The CLI's server-management commands (`wt tenant`, `wt team`, `wt mr`,
//! ...) are thin wrappers over this client: each defines its own serde types
//! and calls [`RestClient::get`], [`RestClient::post`] and friends.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::auth::map_api_error;
use super::client::error_chain;
use super::credentials::CredentialStore;
use super::server_url::normalize_server_url;
use crate::error::{Result, SdkError};
use crate::repo::Repository;

/// Default request timeout (the event stream has none).
const TIMEOUT: Duration = Duration::from_secs(60);

/// Timeout of raw uploads ([`RestClient::put_bytes`]).
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Percent-encode one path segment (tenant slugs, names, branch names...).
pub fn seg(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

/// Percent-encode a '/'-separated path, keeping the slashes.
pub fn path_segs(path: &str) -> String {
    path.split('/').map(seg).collect::<Vec<_>>().join("/")
}

/// An authenticated (or anonymous) REST client bound to one server.
#[derive(Debug, Clone)]
pub struct RestClient {
    server: String,
    username: Option<String>,
    token: Option<String>,
    http: reqwest::Client,
}

impl RestClient {
    /// A client for `server` using `token` (None = anonymous).
    pub fn new(server: &str, token: Option<String>) -> Result<Self> {
        let server = normalize_server_url(server)?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("worktree-sdk/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| SdkError::Remote(format!("cannot create HTTP client: {e}")))?;
        Ok(Self {
            server,
            username: None,
            token,
            http,
        })
    }

    /// A client for `server` with its saved credential; `NotLoggedIn` if
    /// there is none.
    pub fn for_server(server: &str) -> Result<Self> {
        let server = normalize_server_url(server)?;
        let cred = CredentialStore::default_location()
            .get(&server)?
            .ok_or_else(|| SdkError::NotLoggedIn(server.clone()))?;
        let mut client = Self::new(&server, Some(cred.token))?;
        client.username = Some(cred.username);
        Ok(client)
    }

    /// Pick the server for a command: `explicit` (`--server`) if given, else
    /// the URL of `remote` in `repo`, else the only server with a saved
    /// credential. Errors explain how to choose when ambiguous.
    pub fn resolve(
        explicit: Option<&str>,
        repo: Option<&Repository>,
        remote: &str,
    ) -> Result<Self> {
        if let Some(server) = explicit {
            return Self::for_server(server);
        }
        if let Some(section) = repo.and_then(|r| r.config().remote.get(remote)) {
            return Self::for_server(&section.url);
        }
        let servers = CredentialStore::default_location().servers()?;
        match servers.as_slice() {
            [only] => Self::for_server(only),
            [] => Err(SdkError::NotLoggedIn(
                "any server (run `wt login <server>` or pass --server)".into(),
            )),
            many => Err(SdkError::InvalidArgument(format!(
                "logged in to several servers ({}); pass --server",
                many.join(", ")
            ))),
        }
    }

    /// Normalized server base URL.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// Username of the saved credential, if any.
    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    /// GET `path` (relative to the server, e.g. "/api/v1/tenants").
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.json(Method::GET, path, &[], None::<&()>).await
    }

    /// GET with query parameters (empty values are omitted).
    pub async fn get_query<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        self.json(Method::GET, path, query, None::<&()>).await
    }

    pub async fn post<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.json(Method::POST, path, &[], Some(body)).await
    }

    pub async fn put<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.json(Method::PUT, path, &[], Some(body)).await
    }

    pub async fn patch<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.json(Method::PATCH, path, &[], Some(body)).await
    }

    /// Send a request whose success response has no body (204).
    pub async fn send<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&B>,
    ) -> Result<()> {
        self.request(method, path, query, body).await.map(|_| ())
    }

    /// DELETE `path` (204 expected).
    pub async fn delete(&self, path: &str) -> Result<()> {
        self.send(Method::DELETE, path, &[], None::<&()>).await
    }

    /// Raw response bytes (blob downloads, `/metrics`).
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let text = self.request(Method::GET, path, &[], None::<&()>).await?;
        Ok(text)
    }

    // --- deps-releases (ENGINEERING_PLAN §13.5): raw uploads (release assets) ---

    /// PUT `body` verbatim with `content_type` (e.g. a release asset) and
    /// decode the JSON response. Uploads may take up to ten minutes.
    pub async fn put_bytes<T: DeserializeOwned>(
        &self,
        path: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> Result<T> {
        let mut req = self
            .http
            .put(self.url(path))
            .timeout(UPLOAD_TIMEOUT)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.map_err(|e| self.unreachable(&e))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| SdkError::Remote(format!("reading response from {}: {e}", self.server)))?;
        if !status.is_success() {
            return Err(map_api_error(
                &self.server,
                status.as_u16(),
                &String::from_utf8_lossy(&bytes),
            ));
        }
        let bytes = if bytes.is_empty() {
            b"null".to_vec()
        } else {
            bytes.to_vec()
        };
        serde_json::from_slice(&bytes).map_err(|e| {
            SdkError::Remote(format!(
                "unexpected response from {}{path}: {e}",
                self.server
            ))
        })
    }

    // --- end deps-releases ---

    /// Subscribe to `GET /api/v1/events` and call `on_event(type, data)` for
    /// every event until it returns false or the stream ends.
    pub async fn events<F>(&self, query: &[(&str, String)], mut on_event: F) -> Result<()>
    where
        F: FnMut(&str, serde_json::Value) -> bool,
    {
        let mut req = self
            .http
            .get(self.url("/api/v1/events"))
            .query(&non_empty(query));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.map_err(|e| self.unreachable(&e))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(map_api_error(&self.server, status.as_u16(), &body));
        }
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| SdkError::Remote(format!("event stream: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buf.find("\n\n") {
                let frame: String = buf.drain(..end + 2).collect();
                if let Some((kind, data)) = parse_sse_frame(&frame) {
                    let value =
                        serde_json::from_str(&data).unwrap_or(serde_json::Value::String(data));
                    if !on_event(&kind, value) {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    async fn json<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&B>,
    ) -> Result<T> {
        let bytes = self.request(method, path, query, body).await?;
        let bytes = if bytes.is_empty() {
            b"null".to_vec()
        } else {
            bytes
        };
        serde_json::from_slice(&bytes).map_err(|e| {
            SdkError::Remote(format!(
                "unexpected response from {}{path}: {e}",
                self.server
            ))
        })
    }

    async fn request<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&B>,
    ) -> Result<Vec<u8>> {
        let mut req = self
            .http
            .request(method, self.url(path))
            .timeout(TIMEOUT)
            .query(&non_empty(query));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| self.unreachable(&e))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| SdkError::Remote(format!("reading response from {}: {e}", self.server)))?;
        if status.is_success() || status == StatusCode::NOT_MODIFIED {
            return Ok(bytes.to_vec());
        }
        Err(map_api_error(
            &self.server,
            status.as_u16(),
            &String::from_utf8_lossy(&bytes),
        ))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.server, path)
    }

    fn unreachable(&self, e: &reqwest::Error) -> SdkError {
        SdkError::Remote(format!("cannot reach {}: {}", self.server, error_chain(e)))
    }
}

fn non_empty<'a>(query: &'a [(&'a str, String)]) -> Vec<(&'a str, &'a str)> {
    query
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (*k, v.as_str()))
        .collect()
}

/// Parse one SSE frame into (event type, data). Comment-only frames
/// (heartbeats) yield None.
fn parse_sse_frame(frame: &str) -> Option<(String, String)> {
    let mut kind = String::from("message");
    let mut data: Vec<&str> = Vec::new();
    for line in frame.lines() {
        if let Some(v) = line.strip_prefix("event:") {
            kind = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("data:") {
            data.push(v.strip_prefix(' ').unwrap_or(v));
        }
    }
    (!data.is_empty()).then(|| (kind, data.join("\n")))
}

/// The `{"error":{"code","message"}}` envelope, for callers that need the
/// code (e.g. to detect `mfa_required`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_segments_and_paths() {
        assert_eq!(seg("a b/c"), "a%20b%2Fc");
        assert_eq!(path_segs("feature/x y"), "feature/x%20y");
    }

    #[test]
    fn parses_sse_frames() {
        assert_eq!(
            parse_sse_frame("event: branch.pushed\ndata: {\"a\":1}\n\n"),
            Some(("branch.pushed".into(), "{\"a\":1}".into()))
        );
        assert_eq!(parse_sse_frame(": heartbeat\n\n"), None);
    }

    #[test]
    fn drops_empty_query_values() {
        let q = [("a", "1".to_string()), ("b", String::new())];
        assert_eq!(non_empty(&q), vec![("a", "1")]);
    }
}
