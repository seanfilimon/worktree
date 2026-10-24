//! Blocking REST + SSE client for the server's `/api/v1` (plan §7).

use std::fmt;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use reqwest::blocking::{Client, RequestBuilder};
use reqwest::Method;
use serde_json::{json, Value};

use crate::json::pretty;

fn client(timeout: Option<Duration>) -> Client {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(10))
        .no_proxy()
        .build()
        .expect("build reqwest client")
}

#[derive(Clone)]
pub struct Api {
    base: String,
    token: Option<String>,
    /// Who is calling (for reports).
    who: String,
    client: Client,
}

impl fmt::Debug for Api {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Api")
            .field("base", &self.base)
            .field("who", &self.who)
            .finish()
    }
}

impl Api {
    /// Anonymous client for `base` (e.g. `http://127.0.0.1:7420`).
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            token: None,
            who: "anonymous".into(),
            client: client(Some(Duration::from_secs(30))),
        }
    }

    pub fn with_token(&self, who: &str, token: &str) -> Self {
        Self {
            token: Some(token.to_string()),
            who: who.to_string(),
            ..self.clone()
        }
    }

    /// `POST /api/v1/auth/login` and return an authenticated client.
    #[track_caller]
    pub fn login(base: &str, username: &str, password: &str) -> Self {
        let anon = Self::new(base);
        let resp = anon.post(
            "/api/v1/auth/login",
            json!({"username": username, "password": password}),
        );
        resp.assert_ok();
        let body = resp.json();
        let token = body
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("login response has no `token`\n{resp}"));
        anon.with_token(username, token)
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let mut rb = self.client.request(method, self.url(path));
        if let Some(t) = &self.token {
            rb = rb.bearer_auth(t);
        }
        rb
    }

    fn send(&self, method: Method, path: &str, body: Option<Value>) -> Resp {
        let mut rb = self.request(method.clone(), path);
        if let Some(b) = &body {
            rb = rb.json(b);
        }
        let started = Instant::now();
        match rb.send() {
            Ok(r) => {
                let status = r.status().as_u16();
                let content_type = r
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                let bytes = r.bytes().map(|b| b.to_vec()).unwrap_or_default();
                Resp {
                    who: self.who.clone(),
                    method: method.to_string(),
                    url: self.url(path),
                    request_body: body,
                    status,
                    content_type,
                    body: bytes,
                    elapsed: started.elapsed(),
                }
            }
            Err(e) => panic!("[{}] {method} {} failed: {e}", self.who, self.url(path)),
        }
    }

    pub fn get(&self, path: &str) -> Resp {
        self.send(Method::GET, path, None)
    }

    pub fn delete(&self, path: &str) -> Resp {
        self.send(Method::DELETE, path, None)
    }

    pub fn post(&self, path: &str, body: Value) -> Resp {
        self.send(Method::POST, path, Some(body))
    }

    /// POST without a body.
    pub fn post_empty(&self, path: &str) -> Resp {
        self.send(Method::POST, path, None)
    }

    pub fn put(&self, path: &str, body: Value) -> Resp {
        self.send(Method::PUT, path, Some(body))
    }

    pub fn patch(&self, path: &str, body: Value) -> Resp {
        self.send(Method::PATCH, path, Some(body))
    }

    /// Open an SSE stream (e.g. `/api/v1/events`). Connection and reading
    /// happen on a background thread; events arrive on a channel.
    pub fn sse(&self, path: &str) -> SseStream {
        let (tx, rx) = mpsc::channel();
        let rb = {
            let c = client(None);
            let mut rb = c.get(self.url(path)).header("Accept", "text/event-stream");
            if let Some(t) = &self.token {
                rb = rb.bearer_auth(t);
            }
            rb
        };
        let url = self.url(path);
        std::thread::spawn(move || {
            let resp = match rb.send() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(SseMsg::Error(format!("connect: {e}")));
                    return;
                }
            };
            let status = resp.status().as_u16();
            if status != 200 {
                let body = resp.text().unwrap_or_default();
                let _ = tx.send(SseMsg::Error(format!("HTTP {status}: {body}")));
                return;
            }
            if tx.send(SseMsg::Connected).is_err() {
                return;
            }
            let reader = BufReader::new(resp);
            let mut event = String::new();
            let mut data: Vec<String> = Vec::new();
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = tx.send(SseMsg::Error(format!("read: {e}")));
                        return;
                    }
                };
                let line = line.trim_end_matches('\r');
                if line.is_empty() {
                    if !data.is_empty() || !event.is_empty() {
                        let ev = SseEvent {
                            event: if event.is_empty() {
                                "message".into()
                            } else {
                                std::mem::take(&mut event)
                            },
                            data: data.join("\n"),
                        };
                        data.clear();
                        if tx.send(SseMsg::Event(ev)).is_err() {
                            return;
                        }
                    }
                } else if line.starts_with(':') {
                    // comment / heartbeat
                } else if let Some(v) = line.strip_prefix("event:") {
                    event = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("data:") {
                    data.push(v.strip_prefix(' ').unwrap_or(v).to_string());
                }
            }
            let _ = tx.send(SseMsg::Error(format!("stream {url} closed")));
        });
        SseStream {
            rx,
            connected: false,
            seen: Vec::new(),
            error: None,
        }
    }
}

/// A completed HTTP exchange.
#[derive(Debug, Clone)]
pub struct Resp {
    pub who: String,
    pub method: String,
    pub url: String,
    pub request_body: Option<Value>,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub elapsed: Duration,
}

impl Resp {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    #[track_caller]
    pub fn assert_ok(&self) -> &Self {
        assert!(self.is_success(), "expected 2xx\n{self}");
        self
    }

    #[track_caller]
    pub fn assert_status(&self, status: u16) -> &Self {
        assert_eq!(
            self.status, status,
            "unexpected HTTP status (expected {status})\n{self}"
        );
        self
    }

    /// Parse the body as JSON (panics with the full exchange on failure).
    #[track_caller]
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("response body is not JSON ({e})\n{self}"))
    }

    /// 2xx + JSON body.
    #[track_caller]
    pub fn ok_json(&self) -> Value {
        self.assert_ok();
        self.json()
    }

    /// `error.code` of an error body (`{"error":{"code":..,"message":..}}`).
    pub fn error_code(&self) -> Option<String> {
        let v: Value = serde_json::from_slice(&self.body).ok()?;
        v.get("error")?.get("code")?.as_str().map(String::from)
    }

    /// Assert an error response with the given HTTP status and error code.
    #[track_caller]
    pub fn assert_error(&self, status: u16, code: &str) {
        self.assert_status(status);
        assert_eq!(
            self.error_code().as_deref(),
            Some(code),
            "unexpected error code (expected {code:?})\n{self}"
        );
    }
}

impl fmt::Display for Resp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "[{}] {} {} -> {} ({:?})",
            self.who, self.method, self.url, self.status, self.elapsed
        )?;
        if let Some(b) = &self.request_body {
            writeln!(f, "request: {b}")?;
        }
        let text = self.text();
        let shown = match serde_json::from_str::<Value>(&text) {
            Ok(v) => pretty(&v),
            Err(_) => text.chars().take(8 * 1024).collect(),
        };
        write!(f, "response ({}):\n{shown}", self.content_type)
    }
}

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

impl SseEvent {
    #[track_caller]
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.data)
            .unwrap_or_else(|e| panic!("SSE data is not JSON ({e}): {:?}", self.data))
    }
}

enum SseMsg {
    Connected,
    Event(SseEvent),
    Error(String),
}

pub struct SseStream {
    rx: Receiver<SseMsg>,
    connected: bool,
    seen: Vec<SseEvent>,
    error: Option<String>,
}

impl SseStream {
    fn pump(&mut self, timeout: Duration) -> Option<SseEvent> {
        match self.rx.recv_timeout(timeout) {
            Ok(SseMsg::Connected) => {
                self.connected = true;
                None
            }
            Ok(SseMsg::Event(e)) => {
                self.connected = true;
                self.seen.push(e.clone());
                Some(e)
            }
            Ok(SseMsg::Error(e)) => {
                self.error = Some(e);
                None
            }
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                if self.error.is_none() {
                    self.error = Some("SSE reader thread ended".into());
                }
                None
            }
        }
    }

    /// Wait until response headers arrive (200). Panics on an HTTP error.
    /// Returns false on timeout (some servers only flush with the first event).
    #[track_caller]
    pub fn wait_connected(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !self.connected {
            if let Some(e) = &self.error {
                panic!("SSE stream failed: {e}");
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            self.pump(deadline - now);
        }
        true
    }

    /// Wait for an event of type `event` (by SSE `event:` field or the
    /// JSON `type` field in `data`).
    #[track_caller]
    pub fn wait_for(&mut self, event: &str, timeout: Duration) -> SseEvent {
        let matches = |e: &SseEvent| {
            e.event == event
                || serde_json::from_str::<Value>(&e.data)
                    .ok()
                    .and_then(|v| v.get("type").and_then(Value::as_str).map(|t| t == event))
                    .unwrap_or(false)
        };
        if let Some(e) = self.seen.iter().find(|e| matches(e)) {
            return e.clone();
        }
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline || self.error.is_some() {
                panic!(
                    "did not receive SSE event {event:?} within {timeout:?} (error: {:?}); events seen:\n{}",
                    self.error,
                    self.seen.iter().map(|e| format!("  {}: {}", e.event, e.data)).collect::<Vec<_>>().join("\n")
                );
            }
            if let Some(e) = self.pump(deadline - now) {
                if matches(&e) {
                    return e;
                }
            }
        }
    }
}
