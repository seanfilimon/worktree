use std::path::{Path, PathBuf};

use crate::error::Result;

/// The authentication method to use when communicating with a Git remote.
#[derive(Debug, Clone)]
enum AuthMethod {
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
/// Wraps different authentication strategies (SSH keys, credential helpers)
/// behind a uniform interface that can produce `git2::RemoteCallbacks`.
#[derive(Debug, Clone)]
pub struct GitAuth {
    /// The underlying authentication method.
    method: AuthMethod,
}

impl GitAuth {
    /// Create a `GitAuth` that authenticates via an SSH private key file.
    ///
    /// The key file must exist and be readable. A passphrase-protected key
    /// will require additional handling (not yet implemented).
    ///
    /// # Errors
    ///
    /// Returns an error if the key file does not exist or is not readable.
    pub fn from_ssh_key(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(crate::error::GitCompatError::RemoteError(format!(
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
    ///
    /// # Errors
    ///
    /// Currently infallible, but returns `Result` for forward-compatibility
    /// with credential helper discovery/validation.
    pub fn from_credential_helper() -> Result<Self> {
        Ok(Self {
            method: AuthMethod::CredentialHelper,
        })
    }

    /// Build `git2::RemoteCallbacks` configured with this authentication method.
    ///
    /// The returned callbacks can be attached to fetch/push options for
    /// authenticated remote operations.
    pub fn callbacks(&self) -> git2::RemoteCallbacks<'_> {
        let mut callbacks = git2::RemoteCallbacks::new();

        match &self.method {
            AuthMethod::SshKey { private_key_path } => {
                let key_path = private_key_path.clone();
                callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                    let username = username_from_url.unwrap_or("git");
                    git2::Cred::ssh_key(username, None, &key_path, None)
                });
            }
            AuthMethod::CredentialHelper => {
                callbacks.credentials(|url, username_from_url, allowed_types| {
                    if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
                        let config = git2::Config::open_default()?;
                        git2::Cred::credential_helper(&config, url, username_from_url)
                    } else if allowed_types.contains(git2::CredentialType::DEFAULT) {
                        git2::Cred::default()
                    } else {
                        Err(git2::Error::from_str(
                            "no supported credential type available",
                        ))
                    }
                });
            }
        }

        callbacks
    }
}
