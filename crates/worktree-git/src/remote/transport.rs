use serde::{Deserialize, Serialize};

/// Describes the transport protocol used to communicate with a Git remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitTransport {
    /// HTTP(S) transport with the full URL.
    Https(String),
    /// SSH transport (`ssh://` or scp-like `user@host:path`) with the full URL.
    Ssh(String),
    /// A repository on the local filesystem (`file://` URL or plain path).
    Local(String),
}

impl GitTransport {
    /// Return the underlying URL string regardless of transport kind.
    pub fn url(&self) -> &str {
        match self {
            GitTransport::Https(url) | GitTransport::Ssh(url) | GitTransport::Local(url) => url,
        }
    }

    /// Attempt to detect the transport type from a URL string.
    ///
    /// Returns `None` if the URL scheme is not supported (e.g. `ftp://`).
    pub fn from_url(url: impl Into<String>) -> Option<Self> {
        let url = url.into();
        if url.is_empty() {
            None
        } else if url.starts_with("https://") || url.starts_with("http://") {
            Some(GitTransport::Https(url))
        } else if url.starts_with("ssh://") || url.starts_with("git+ssh://") || is_scp_like(&url) {
            Some(GitTransport::Ssh(url))
        } else if url.starts_with("file://") || !url.contains("://") {
            Some(GitTransport::Local(url))
        } else {
            None
        }
    }

    /// Returns `true` if this transport uses HTTP(S).
    pub fn is_https(&self) -> bool {
        matches!(self, GitTransport::Https(_))
    }

    /// Returns `true` if this transport uses SSH.
    pub fn is_ssh(&self) -> bool {
        matches!(self, GitTransport::Ssh(_))
    }

    /// Returns `true` for local filesystem repositories.
    pub fn is_local(&self) -> bool {
        matches!(self, GitTransport::Local(_))
    }
}

/// `[user@]host:path` (Git's scp-like syntax), excluding Windows drive
/// paths such as `C:\repo` or `C:/repo`.
fn is_scp_like(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let host = &url[..colon];
    let is_drive = host.len() == 1 && host.chars().all(|c| c.is_ascii_alphabetic());
    !host.is_empty() && !is_drive && !host.contains(['/', '\\']) && !url[colon..].starts_with("://")
}

impl std::fmt::Display for GitTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitTransport::Https(url) => write!(f, "https: {}", url),
            GitTransport::Ssh(url) => write!(f, "ssh: {}", url),
            GitTransport::Local(url) => write!(f, "local: {}", url),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_url() {
        let t = GitTransport::Https("https://github.com/user/repo.git".into());
        assert_eq!(t.url(), "https://github.com/user/repo.git");
        assert!(t.is_https());
        assert!(!t.is_ssh());
    }

    #[test]
    fn ssh_url() {
        let t = GitTransport::Ssh("git@github.com:user/repo.git".into());
        assert_eq!(t.url(), "git@github.com:user/repo.git");
        assert!(t.is_ssh());
        assert!(!t.is_https());
    }

    #[test]
    fn from_url_detection() {
        assert!(GitTransport::from_url("https://example.com/repo.git")
            .unwrap()
            .is_https());
        assert!(GitTransport::from_url("http://localhost:8080/repo.git")
            .unwrap()
            .is_https());
        assert!(GitTransport::from_url("git@example.com:repo.git")
            .unwrap()
            .is_ssh());
        assert!(GitTransport::from_url("ssh://git@example.com/repo.git")
            .unwrap()
            .is_ssh());
        assert!(GitTransport::from_url("example.com:repo.git")
            .unwrap()
            .is_ssh());
        assert!(GitTransport::from_url("ftp://example.com").is_none());
        assert!(GitTransport::from_url("").is_none());
    }

    #[test]
    fn local_paths() {
        for url in [
            "file:///srv/repo.git",
            "/srv/repo.git",
            "../relative/repo",
            r"C:\Users\me\repo.git",
            "C:/Users/me/repo.git",
            r"\\server\share\repo.git",
        ] {
            let t = GitTransport::from_url(url).unwrap();
            assert!(t.is_local(), "{url} should be local");
            assert_eq!(t.url(), url);
        }
    }

    #[test]
    fn display() {
        assert_eq!(GitTransport::Local("/r".into()).to_string(), "local: /r");
    }
}
