//! The server tree a helper remote points at, parsed from its URL.

use worktree_sdk::remote::normalize_server_url;

use crate::error::{GitCompatError, Result};

/// `<server>/<tenant>/<tree>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Normalized server base URL (`https://host[:port]`).
    pub server: String,
    /// Tenant slug.
    pub tenant: String,
    /// Tree name on the server.
    pub tree: String,
}

impl Target {
    /// Parse the address Git hands to the helper.
    ///
    /// * `wt::https://host/tenant/tree` reaches the helper as
    ///   `https://host/tenant/tree` (likewise `http://`);
    /// * `wt://host/tenant/tree` reaches it unchanged and means HTTPS.
    ///
    /// The path must be exactly `<tenant>/<tree>` (a trailing slash is
    /// ignored). Embedded credentials, query strings and fragments are
    /// rejected: authentication uses `wt login` / `WT_TOKEN`.
    pub fn parse(address: &str) -> Result<Self> {
        let invalid = |why: &str| {
            GitCompatError::RemoteError(format!(
                "invalid W0rkTree remote URL {address:?}: {why} \
                 (expected wt::https://<host>/<tenant>/<tree>)"
            ))
        };
        let address_trimmed = address.trim();
        let (scheme, rest) = address_trimmed
            .split_once("://")
            .ok_or_else(|| invalid("missing scheme"))?;
        let scheme = match scheme.to_ascii_lowercase().as_str() {
            "wt" | "https" => "https",
            "http" => "http",
            _ => return Err(invalid("the scheme must be https, http or wt")),
        };
        if rest.contains(['?', '#']) {
            return Err(invalid("query strings and fragments are not allowed"));
        }
        let (authority, path) = rest
            .split_once('/')
            .ok_or_else(|| invalid("missing <tenant>/<tree>"))?;
        let segments: Vec<&str> = path.trim_end_matches('/').split('/').collect();
        let [tenant, tree] = segments.as_slice() else {
            return Err(invalid("the path must be <tenant>/<tree>"));
        };
        if tenant.is_empty() || tree.is_empty() {
            return Err(invalid("the path must be <tenant>/<tree>"));
        }
        let server = normalize_server_url(&format!("{scheme}://{authority}"))
            .map_err(|e| invalid(&e.to_string()))?;
        Ok(Self {
            server,
            tenant: (*tenant).to_owned(),
            tree: (*tree).to_owned(),
        })
    }

    /// `<tenant>/<tree>`, as `wt clone` takes it.
    pub fn spec(&self) -> String {
        format!("{}/{}", self.tenant, self.tree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(server: &str, tenant: &str, tree: &str) -> Target {
        Target {
            server: server.into(),
            tenant: tenant.into(),
            tree: tree.into(),
        }
    }

    #[test]
    fn parses_http_and_https() {
        assert_eq!(
            Target::parse("https://wt.example.com/acme/web").unwrap(),
            target("https://wt.example.com", "acme", "web")
        );
        assert_eq!(
            Target::parse("http://LocalHost:7420/acme/web/").unwrap(),
            target("http://localhost:7420", "acme", "web")
        );
        assert_eq!(
            Target::parse("https://wt.example.com:443/a/b")
                .unwrap()
                .server,
            "https://wt.example.com"
        );
    }

    #[test]
    fn wt_scheme_means_https() {
        let parsed = Target::parse("wt://wt.example.com:8443/acme/web").unwrap();
        assert_eq!(parsed, target("https://wt.example.com:8443", "acme", "web"));
        assert_eq!(parsed.spec(), "acme/web");
    }

    #[test]
    fn rejects_everything_else() {
        for bad in [
            "",
            "wt.example.com/acme/web",
            "ssh://wt.example.com/acme/web",
            "https://wt.example.com",
            "https://wt.example.com/",
            "https://wt.example.com/acme",
            "https://wt.example.com/acme/",
            "https://wt.example.com//web",
            "https://wt.example.com/acme/web/extra",
            "https://wt.example.com/acme/web?x=1",
            "https://wt.example.com/acme/web#frag",
            "https://user:secret@wt.example.com/acme/web",
            "https:///acme/web",
        ] {
            let err = Target::parse(bad).expect_err(bad);
            assert!(
                matches!(&err, GitCompatError::RemoteError(m) if m.contains("wt::https://")),
                "{bad:?}: {err}"
            );
        }
    }
}
