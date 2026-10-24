//! Canonical form of server base URLs.
//!
//! The same normalized string is used as the key in `credentials.toml`, as
//! the REST base and as the gRPC endpoint, so `localhost:7420`,
//! `HTTP://LocalHost:7420/` and `http://localhost:7420` all refer to one
//! server.

use url::Url;

use crate::error::{Result, SdkError};

/// Normalize a server base URL to `scheme://host[:port]`.
///
/// * a missing scheme defaults to `http://`;
/// * the scheme and host are lower-cased and default ports dropped;
/// * trailing slashes are removed.
///
/// Only `http` and `https` are accepted. Paths, queries, fragments and
/// embedded credentials are rejected (`InvalidArgument`): gRPC requests
/// always target the root of the authority, so a path prefix could never
/// work for sync.
pub fn normalize_server_url(server: &str) -> Result<String> {
    let trimmed = server.trim();
    if trimmed.is_empty() {
        return Err(SdkError::InvalidArgument("server URL is empty".into()));
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let invalid =
        |why: &str| SdkError::InvalidArgument(format!("invalid server URL '{server}': {why}"));

    let url = Url::parse(&with_scheme).map_err(|e| invalid(&e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid("scheme must be http or https"));
    }
    let host = url
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| invalid("missing host"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid(
            "credentials must not be embedded in the URL; use `wt login`",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(invalid("query strings and fragments are not allowed"));
    }
    if !url.path().trim_matches('/').is_empty() {
        return Err(invalid("the server URL must not contain a path"));
    }

    Ok(match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_default_scheme_and_strips_trailing_slash() {
        assert_eq!(
            normalize_server_url("localhost:7420").unwrap(),
            "http://localhost:7420"
        );
        assert_eq!(
            normalize_server_url("http://localhost:7420/").unwrap(),
            "http://localhost:7420"
        );
        assert_eq!(
            normalize_server_url("  http://localhost:7420//  ").unwrap(),
            "http://localhost:7420"
        );
    }

    #[test]
    fn lowercases_and_drops_default_ports() {
        assert_eq!(
            normalize_server_url("HTTP://Example.COM:80").unwrap(),
            "http://example.com"
        );
        assert_eq!(
            normalize_server_url("https://wt.example.com:443/").unwrap(),
            "https://wt.example.com"
        );
        assert_eq!(
            normalize_server_url("https://wt.example.com:8443").unwrap(),
            "https://wt.example.com:8443"
        );
    }

    #[test]
    fn keeps_ip_hosts() {
        assert_eq!(
            normalize_server_url("127.0.0.1:7420").unwrap(),
            "http://127.0.0.1:7420"
        );
        assert_eq!(
            normalize_server_url("http://[::1]:7420").unwrap(),
            "http://[::1]:7420"
        );
    }

    #[test]
    fn rejects_bad_urls() {
        for bad in [
            "",
            "   ",
            "ftp://example.com",
            "http://",
            "http://user:pw@example.com",
            "http://example.com/wt",
            "http://example.com?x=1",
            "http://example.com#frag",
        ] {
            let err = normalize_server_url(bad).unwrap_err();
            assert!(
                matches!(err, SdkError::InvalidArgument(_)),
                "{bad:?} -> {err:?}"
            );
        }
    }

    #[test]
    fn is_idempotent() {
        let once = normalize_server_url("Example.com:7420/").unwrap();
        assert_eq!(normalize_server_url(&once).unwrap(), once);
    }
}
