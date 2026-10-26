//! `wt api` — call any REST API endpoint of the server with your
//! credentials (docs/ENGINEERING_PLAN.md §14.10).
//!
//! The request goes to the server chosen like every server-side command
//! (`--server` / `WT_SERVER`, else the worktree's origin, else the only saved
//! login) with that server's saved token (or `WT_TOKEN`). JSON responses are
//! pretty-printed with their key order kept; other bodies are written
//! verbatim. A non-2xx answer exits 1 with the server's error message. The
//! routes are described by `GET /api/v1/openapi.yaml`.

use std::io::{Read, Write};
use std::time::Duration;

use clap::Args;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Method;

use crate::context::{block_on, Context};
use crate::error::{CliError, Result};

/// Per-request timeout (generous: bodies may be release assets).
const TIMEOUT: Duration = Duration::from_secs(600);

/// Longest non-JSON error body quoted in an error message.
const MAX_ERROR_TEXT: usize = 300;

#[derive(Debug, Args)]
#[command(after_help = "\
Examples:
  wt api GET tenants
  wt api GET /api/v1/trees/acme/web/branches
  wt api POST tenants -d '{\"slug\":\"acme\",\"name\":\"Acme\"}'
  wt api PUT trees/acme/web/protections -d @rule.json
  wt api PUT trees/acme/web/releases/v1/assets/app.tar.gz -d @app.tar.gz -H 'Content-Type: application/gzip'
  wt api GET openapi.yaml --raw > openapi.yaml")]
pub struct ApiArgs {
    /// HTTP method (GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS)
    #[arg(value_name = "METHOD")]
    pub method: String,

    /// Request path with an optional query string; paths not starting with
    /// '/' are relative to /api/v1/ (`tenants` = `/api/v1/tenants`)
    #[arg(value_name = "PATH")]
    pub path: String,

    /// Request body: inline JSON, @FILE to read a file, or @- for stdin.
    /// Sent as application/json (and validated as JSON) unless a
    /// Content-Type header is given
    #[arg(short = 'd', long = "data", value_name = "JSON|@FILE")]
    pub data: Option<String>,

    /// Extra request header "Name: value" (repeatable; overrides defaults
    /// such as Authorization)
    #[arg(short = 'H', long = "header", value_name = "K:V")]
    pub headers: Vec<String>,

    /// Print the response body exactly as received (no JSON formatting)
    #[arg(long)]
    pub raw: bool,
}

pub fn run(ctx: &Context, args: ApiArgs) -> Result<()> {
    let method = parse_method(&args.method)?;
    let path = api_path(&args.path)?;
    let mut headers = parse_headers(&args.headers)?;
    let body = match &args.data {
        Some(data) => Some(read_body(ctx, data)?),
        None => None,
    };
    if let Some(body) = &body {
        if !headers.contains_key(CONTENT_TYPE) {
            if serde_json::from_slice::<serde::de::IgnoredAny>(body).is_err() {
                return Err(CliError::usage("the request body is not valid JSON")
                    .with_hint("pass -H 'Content-Type: <type>' to send another format"));
            }
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
    }

    let client = ctx.rest()?;
    let server = client.server().to_string();
    if !headers.contains_key(AUTHORIZATION) {
        // device-auth (§16.6): renew an access token about to expire.
        let token =
            block_on(worktree_sdk::remote::fresh_credential(&server))??.map(|cred| cred.token);
        if let Some(token) = token {
            let value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| CliError::failure("the saved token is not a valid header value"))?;
            headers.insert(AUTHORIZATION, value);
        }
    }

    let url = format!("{server}{path}");
    let response = block_on(send(method, &url, headers, body))?
        .map_err(|e| CliError::failure(format!("cannot reach {server}: {e}")))?;

    if !(200..300).contains(&response.status) {
        return Err(CliError::failure(error_message(
            response.status,
            &response.body,
        )));
    }
    let output = render_body(&response, args.raw, ctx.out.is_json());
    write_stdout(&output)
}

/// A finished response.
#[derive(Debug)]
struct Response {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

async fn send(
    method: Method,
    url: &str,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
) -> std::result::Result<Response, String> {
    let http = reqwest::Client::builder()
        .user_agent(concat!("wt/", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = http.request(method, url).headers(headers);
    if let Some(body) = body {
        req = req.body(body);
    }
    let resp = req.send().await.map_err(|e| error_chain(&e))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = resp.bytes().await.map_err(|e| error_chain(&e))?.to_vec();
    Ok(Response {
        status,
        content_type,
        body,
    })
}

/// `e` and its sources, joined with ": ".
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        let part = s.to_string();
        if !text.contains(&part) {
            text.push_str(": ");
            text.push_str(&part);
        }
        source = s.source();
    }
    text
}

fn parse_method(method: &str) -> Result<Method> {
    let upper = method.to_ascii_uppercase();
    match upper.as_str() {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" => {
            Method::from_bytes(upper.as_bytes())
                .map_err(|_| CliError::usage(format!("invalid HTTP method '{method}'")))
        }
        _ => Err(CliError::usage(format!(
            "unsupported HTTP method '{method}' (use GET, POST, PUT, PATCH, DELETE, HEAD or OPTIONS)"
        ))),
    }
}

/// The request path: absolute paths are kept, relative ones are taken
/// relative to `/api/v1/`.
fn api_path(path: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(CliError::usage("PATH must not be empty"));
    }
    if path.contains("://") {
        return Err(CliError::usage(format!(
            "PATH must be a path such as /api/v1/tenants, not a URL ('{path}')"
        ))
        .with_hint("choose the server with --server"));
    }
    if path.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CliError::usage(
            "PATH must not contain whitespace (percent-encode it)",
        ));
    }
    Ok(if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/api/v1/{path}")
    })
}

fn parse_headers(raw: &[String]) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    for header in raw {
        let (name, value) = header.split_once(':').ok_or_else(|| {
            CliError::usage(format!(
                "header '{header}' must be of the form 'Name: value'"
            ))
        })?;
        let name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| CliError::usage(format!("invalid header name in '{header}'")))?;
        let value = HeaderValue::from_str(value.trim())
            .map_err(|_| CliError::usage(format!("invalid header value in '{header}'")))?;
        headers.append(name, value);
    }
    Ok(headers)
}

/// The bytes of `-d`: inline text, `@file` (cwd-relative) or `@-` (stdin).
fn read_body(ctx: &Context, data: &str) -> Result<Vec<u8>> {
    match data.strip_prefix('@') {
        Some("-") => {
            let mut buf = Vec::new();
            std::io::stdin().lock().read_to_end(&mut buf)?;
            Ok(buf)
        }
        Some(file) => {
            let path = ctx.absolute(std::path::Path::new(file));
            std::fs::read(&path)
                .map_err(|e| CliError::failure(format!("cannot read {}: {e}", path.display())))
        }
        None => Ok(data.as_bytes().to_vec()),
    }
}

/// The message of a non-2xx answer: the error envelope's code and message
/// when present, else (the start of) the body.
fn error_message(status: u16, body: &[u8]) -> String {
    #[derive(serde::Deserialize)]
    struct Envelope {
        error: Detail,
    }
    #[derive(serde::Deserialize)]
    struct Detail {
        #[serde(default)]
        code: String,
        #[serde(default)]
        message: String,
    }
    if let Ok(env) = serde_json::from_slice::<Envelope>(body) {
        let detail = env.error;
        return match (detail.code.is_empty(), detail.message.is_empty()) {
            (false, false) => format!("HTTP {status} {}: {}", detail.code, detail.message),
            (false, true) => format!("HTTP {status} {}", detail.code),
            (true, false) => format!("HTTP {status}: {}", detail.message),
            (true, true) => format!("HTTP {status}"),
        };
    }
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    if text.is_empty() {
        return format!("HTTP {status}");
    }
    let mut short: String = text.chars().take(MAX_ERROR_TEXT).collect();
    if short.len() < text.len() {
        short.push_str("...");
    }
    format!("HTTP {status}: {short}")
}

/// What to print for a successful response.
fn render_body(resp: &Response, raw: bool, json_mode: bool) -> Vec<u8> {
    if resp.body.is_empty() {
        return if json_mode {
            b"null\n".to_vec()
        } else {
            Vec::new()
        };
    }
    if raw {
        return resp.body.clone();
    }
    let looks_json = resp.content_type.contains("json")
        || resp.content_type.is_empty()
        || resp.content_type.starts_with("text/plain");
    if looks_json && serde_json::from_slice::<serde::de::IgnoredAny>(&resp.body).is_ok() {
        if let Ok(text) = std::str::from_utf8(&resp.body) {
            let mut out = pretty_json(text);
            out.push('\n');
            return out.into_bytes();
        }
    }
    resp.body.clone()
}

/// Re-indent a valid JSON document with two spaces, keeping key order and
/// number spelling exactly as received.
fn pretty_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = text.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
    };
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                // Keep empty containers on one line.
                while chars.peek().is_some_and(|n| n.is_whitespace()) {
                    chars.next();
                }
                let close = if c == '{' { '}' } else { ']' };
                if chars.peek() == Some(&close) {
                    out.push(close);
                    chars.next();
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

fn write_stdout(bytes: &[u8]) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(bytes).and_then(|()| stdout.flush()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    #[test]
    fn parses_arguments() {
        let Command::Api(args) = parse(&[
            "api", "post", "tenants", "-d", "{}", "-H", "X-A: 1", "--header", "X-B:2", "--raw",
        ]) else {
            panic!("not api");
        };
        assert_eq!(args.method, "post");
        assert_eq!(args.path, "tenants");
        assert_eq!(args.data.as_deref(), Some("{}"));
        assert_eq!(args.headers, ["X-A: 1", "X-B:2"]);
        assert!(args.raw);
        assert_eq!(
            parse_err(&["api", "GET"]),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn methods() {
        assert_eq!(parse_method("get").expect("get"), Method::GET);
        assert_eq!(parse_method("Patch").expect("patch"), Method::PATCH);
        let err = parse_method("FETCH").expect_err("unsupported");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn paths_default_to_api_v1() {
        assert_eq!(api_path("tenants").expect("rel"), "/api/v1/tenants");
        assert_eq!(
            api_path("trees/acme/web/snapshots?limit=5").expect("query"),
            "/api/v1/trees/acme/web/snapshots?limit=5"
        );
        assert_eq!(api_path("/healthz").expect("abs"), "/healthz");
        assert_eq!(api_path("/api/v1/auth/me").expect("abs"), "/api/v1/auth/me");
        assert!(api_path("").is_err());
        assert!(api_path("http://x/api").is_err());
        assert!(api_path("a b").is_err());
    }

    #[test]
    fn headers() {
        let h = parse_headers(&["Content-Type: text/plain".into(), "X-Y:  z ".into()])
            .expect("headers");
        assert_eq!(h.get(CONTENT_TYPE).expect("ct"), "text/plain");
        assert_eq!(h.get("x-y").expect("x-y"), "z");
        assert!(parse_headers(&["novalue".into()]).is_err());
        assert!(parse_headers(&["bad name: v".into()]).is_err());
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            error_message(
                404,
                br#"{"error":{"code":"not_found","message":"tenant \"x\" not found"}}"#
            ),
            "HTTP 404 not_found: tenant \"x\" not found"
        );
        assert_eq!(error_message(502, b""), "HTTP 502");
        assert_eq!(error_message(500, b"boom\n"), "HTTP 500: boom");
        let long = "x".repeat(1000);
        assert!(error_message(500, long.as_bytes()).ends_with("..."));
    }

    #[test]
    fn pretty_json_keeps_order_and_numbers() {
        assert_eq!(
            pretty_json(r#"{"b":1,"a":[1.50,{}],"s":"x,{y}: \"z\"","e":[ ]}"#),
            "{\n  \"b\": 1,\n  \"a\": [\n    1.50,\n    {}\n  ],\n  \"s\": \"x,{y}: \\\"z\\\"\",\n  \"e\": []\n}"
        );
        assert_eq!(pretty_json("[]"), "[]");
        assert_eq!(pretty_json("42"), "42");
    }

    #[test]
    fn renders_bodies() {
        let resp = |ct: &str, body: &[u8]| Response {
            status: 200,
            content_type: ct.into(),
            body: body.to_vec(),
        };
        assert_eq!(
            render_body(&resp("application/json", br#"{"a":1}"#), false, false),
            b"{\n  \"a\": 1\n}\n"
        );
        assert_eq!(
            render_body(&resp("application/json", br#"{"a":1}"#), true, false),
            br#"{"a":1}"#
        );
        assert_eq!(
            render_body(&resp("application/yaml", b"openapi: 3.1.0\n"), false, false),
            b"openapi: 3.1.0\n"
        );
        assert_eq!(render_body(&resp("", b""), false, false), b"");
        assert_eq!(render_body(&resp("", b""), false, true), b"null\n");
    }
}
