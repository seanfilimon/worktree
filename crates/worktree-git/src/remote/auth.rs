use std::path::{Path, PathBuf};

use git2::{Cred, CredentialType, RemoteCallbacks};

use crate::error::{GitCompatError, Result};

/// libgit2 calls the credentials callback again after every rejected
/// attempt; give up after this many so bad credentials fail fast.
const MAX_ATTEMPTS: u32 = 3;

/// The authentication method to use when communicating with a Git remote.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthMethod {
    /// SSH agent for SSH, credential helper for HTTPS, then libgit2 defaults.
    Auto,
    /// Authenticate using an SSH private key file.
    SshKey {
        /// Path to the private key on disk.
        private_key_path: PathBuf,
    },
    /// Authenticate using the system's credential helper (e.g. `git credential-manager`).
    CredentialHelper,
}

/// Handles authentication for Git remote operations.
///
/// Wraps different authentication strategies (SSH agent/keys, credential
/// helpers) behind a uniform interface that produces `git2::RemoteCallbacks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitAuth {
    /// The underlying authentication method.
    method: AuthMethod,
}

impl Default for GitAuth {
    /// [`GitAuth::auto`].
    fn default() -> Self {
        Self::auto()
    }
}

impl GitAuth {
    /// Pick credentials based on what the server accepts: the SSH agent for
    /// SSH keys, the configured credential helper for user/password, and
    /// libgit2's default credentials (e.g. Negotiate/NTLM) otherwise.
    pub fn auto() -> Self {
        Self {
            method: AuthMethod::Auto,
        }
    }

    /// Create a `GitAuth` that authenticates via an SSH private key file
    /// (passphrase-less keys only).
    ///
    /// # Errors
    ///
    /// Returns an error if the key file does not exist.
    pub fn from_ssh_key(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Err(GitCompatError::RemoteError(format!(
                "SSH key not found: {}",
                path.display()
            )));
        }
        Ok(Self {
            method: AuthMethod::SshKey {
                private_key_path: path.to_path_buf(),
            },
        })
    }

    /// Create a `GitAuth` that delegates to the system's Git credential helper.
    pub fn from_credential_helper() -> Self {
        Self {
            method: AuthMethod::CredentialHelper,
        }
    }

    /// Build `git2::RemoteCallbacks` configured with this authentication method.
    ///
    /// The returned callbacks can be attached to fetch/push options for
    /// authenticated remote operations.
    pub fn callbacks(&self) -> RemoteCallbacks<'_> {
        let mut callbacks = RemoteCallbacks::new();
        let mut attempts = 0;
        callbacks.credentials(move |url, username_from_url, allowed| {
            attempts += 1;
            if attempts > MAX_ATTEMPTS {
                return Err(git2::Error::from_str(&format!(
                    "authentication failed for {url} after {MAX_ATTEMPTS} attempts"
                )));
            }
            self.credential(url, username_from_url, allowed)
        });
        callbacks
    }

    fn credential(
        &self,
        url: &str,
        username_from_url: Option<&str>,
        allowed: CredentialType,
    ) -> std::result::Result<Cred, git2::Error> {
        let username = username_from_url.unwrap_or("git");
        if allowed.contains(CredentialType::USERNAME) {
            return Cred::username(username);
        }
        match &self.method {
            AuthMethod::SshKey { private_key_path }
                if allowed.contains(CredentialType::SSH_KEY) =>
            {
                Cred::ssh_key(username, None, private_key_path, None)
            }
            AuthMethod::Auto if allowed.contains(CredentialType::SSH_KEY) => {
                Cred::ssh_key_from_agent(username)
            }
            AuthMethod::Auto | AuthMethod::CredentialHelper
                if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) =>
            {
                let config = git2::Config::open_default()?;
                Cred::credential_helper(&config, url, username_from_url)
            }
            _ if allowed.contains(CredentialType::DEFAULT) => Cred::default(),
            _ => Err(git2::Error::from_str(&format!(
                "no supported credential type for {url} (server allows {allowed:?})"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_auto() {
        assert_eq!(GitAuth::default(), GitAuth::auto());
        assert_ne!(GitAuth::auto(), GitAuth::from_credential_helper());
    }

    #[test]
    fn missing_ssh_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = GitAuth::from_ssh_key(&dir.path().join("id_ed25519")).unwrap_err();
        assert!(matches!(err, GitCompatError::RemoteError(m) if m.contains("SSH key not found")));
        // A directory is not a key file either.
        assert!(GitAuth::from_ssh_key(dir.path()).is_err());
    }

    #[test]
    fn existing_ssh_key_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("id_ed25519");
        std::fs::write(&key, "not really a key").unwrap();
        assert!(GitAuth::from_ssh_key(&key).is_ok());
    }

    #[test]
    fn username_requests_use_url_username() {
        let auth = GitAuth::auto();
        let cred = auth
            .credential(
                "ssh://alice@host/repo",
                Some("alice"),
                CredentialType::USERNAME,
            )
            .unwrap();
        assert!(cred.has_username());
    }

    #[test]
    fn unsupported_credential_types_error() {
        let auth = GitAuth::from_credential_helper();
        let err = auth
            .credential("https://host/repo", None, CredentialType::SSH_MEMORY)
            .err()
            .expect("unsupported");
        assert!(err.message().contains("no supported credential type"));
    }
}
