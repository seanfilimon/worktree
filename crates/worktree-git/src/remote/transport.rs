use serde::{Deserialize, Serialize};

/// Describes the transport protocol used to communicate with a Git remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitTransport {
    /// HTTPS transport with the full URL.
    Https(String),
    /// SSH transport with the full URL.
    Ssh(String),
}

impl GitTransport {
    /// Return the underlying URL string regardless of transport kind.
    pub fn url(&self) -> &str {
        match self {
            GitTransport::Https(url) => url.as_str(),
            GitTransport::Ssh(url) => url.as_str(),
        }
    }

    /// Attempt to detect the transport type from a URL string.
    ///
    /// Returns `None` if the URL scheme is not recognized.
    pub fn from_url(url: impl Into<String>) -> Option<Self> {
        let url = url.into();
        if url.starts_with("https://") || url.starts_with("http://") {
            Some(GitTransport::Https(url))
        } else if url.starts_with("ssh://") || url.starts_with("git@") {
            Some(GitTransport::Ssh(url))
        } else {
            None
        }
    }

    /// Returns `true` if this transport uses HTTPS.
    pub fn is_https(&self) -> bool {
        matches!(self, GitTransport::Https(_))
    }

    /// Returns `true` if this transport uses SSH.
    pub fn is_ssh(&self) -> bool {
        matches!(self, GitTransport::Ssh(_))
    }
}

impl std::fmt::Display for GitTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitTransport::Https(url) => write!(f, "https: {}", url),
            GitTransport::Ssh(url) => write!(f, "ssh: {}", url),
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
        assert!(GitTransport::from_url("git@example.com:repo.git")
            .unwrap()
            .is_ssh());
        assert!(GitTransport::from_url("ftp://example.com").is_none());
    }
}
