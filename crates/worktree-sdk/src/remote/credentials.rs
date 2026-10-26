//! Saved login tokens: `$WT_HOME/credentials.toml`.
//!
//! ```toml
//! [servers."http://localhost:7420"]
//! username = "alice"
//! token = "wt_..."
//! # since v0.8 (device-auth, §16.6): logins are token pairs
//! refresh_token = "wtrt_..."
//! expires_at = "2026-10-26T12:00:00Z"
//! ```
//!
//! `token` is the bearer token (an access token valid one hour for logins
//! since v0.8); `refresh_token` renews it and is replaced on every renewal.
//! Keys are normalized server URLs (see [`normalize_server_url`]). The file
//! is rewritten atomically (temp file + rename) and, on Unix, created with
//! mode `0600` because it holds bearer and refresh tokens.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::server_url::normalize_server_url;
use super::Credential;
use crate::config::wt_home;
use crate::error::Result;

/// File name of the credential store inside `$WT_HOME`.
pub const CREDENTIALS_FILE: &str = "credentials.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialsFile {
    #[serde(default)]
    servers: BTreeMap<String, Credential>,
}

/// A credential file at an explicit path. [`CredentialStore::default_location`]
/// is the per-user store; tests and tools may point at any other file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialStore {
    path: PathBuf,
    /// Honour `WT_TOKEN` (only the per-user default store does).
    env: bool,
}

/// Environment variable holding a bearer token (an API token or a
/// service-account key) used for every server instead of saved logins,
/// e.g. in CI. `WT_TOKEN_USER` optionally names the account for display.
pub const TOKEN_ENV: &str = "WT_TOKEN";

fn env_credential() -> Option<Credential> {
    let token = std::env::var(TOKEN_ENV).ok()?.trim().to_string();
    if token.is_empty() {
        return None;
    }
    let username = std::env::var("WT_TOKEN_USER")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| "token".into());
    Some(Credential::new(username, token))
}

impl CredentialStore {
    /// A store backed by `path` (created on first write).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            env: false,
        }
    }

    /// `$WT_HOME/credentials.toml`; `WT_TOKEN`, when set, overrides it.
    pub fn default_location() -> Self {
        Self {
            path: wt_home().join(CREDENTIALS_FILE),
            env: true,
        }
    }

    /// Path of the backing file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The credential saved for `server` (any URL form; it is normalized).
    pub fn get(&self, server: &str) -> Result<Option<Credential>> {
        let key = normalize_server_url(server)?;
        if self.env {
            if let Some(cred) = env_credential() {
                return Ok(Some(cred));
            }
        }
        Ok(self.read()?.servers.remove(&key))
    }

    /// Save (or replace) the credential for `server`.
    pub fn put(&self, server: &str, cred: &Credential) -> Result<()> {
        let key = normalize_server_url(server)?;
        let mut file = self.read()?;
        file.servers.insert(key, cred.clone());
        self.write(&file)
    }

    /// Forget the credential for `server`. Returns whether one existed.
    pub fn remove(&self, server: &str) -> Result<bool> {
        let key = normalize_server_url(server)?;
        let mut file = self.read()?;
        if file.servers.remove(&key).is_none() {
            return Ok(false);
        }
        self.write(&file)?;
        Ok(true)
    }

    /// Normalized URLs of every server with a saved credential, sorted.
    pub fn servers(&self) -> Result<Vec<String>> {
        Ok(self.read()?.servers.into_keys().collect())
    }

    fn read(&self) -> Result<CredentialsFile> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CredentialsFile::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn write(&self, file: &CredentialsFile) -> Result<()> {
        let text = toml::to_string_pretty(file)?;
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self
            .path
            .with_extension(format!("toml.tmp.{}", std::process::id()));
        let written =
            write_private(&tmp, text.as_bytes()).and_then(|()| std::fs::rename(&tmp, &self.path));
        if let Err(e) = written {
            // Best effort: never leave a stray copy of the tokens behind.
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}

/// Write `data` to a new file readable only by the current user (Unix).
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    #[cfg(unix)]
    {
        // `mode` only applies on creation; tighten a pre-existing temp file too.
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(data)?;
    f.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(user: &str, token: &str) -> Credential {
        Credential::new(user, token)
    }

    fn store() -> (tempfile::TempDir, CredentialStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::new(dir.path().join("nested").join(CREDENTIALS_FILE));
        (dir, store)
    }

    #[test]
    fn missing_file_means_no_credentials() {
        let (_dir, store) = store();
        assert_eq!(store.get("http://localhost:7420").unwrap(), None);
        assert!(store.servers().unwrap().is_empty());
        assert!(!store.remove("http://localhost:7420").unwrap());
        assert!(
            !store.path().exists(),
            "remove of nothing must not create the file"
        );
    }

    #[test]
    fn put_get_remove_round_trip_with_normalized_keys() {
        let (_dir, store) = store();
        store.put("localhost:7420/", &cred("alice", "t1")).unwrap();
        store
            .put("https://wt.example.com", &cred("bob", "t2"))
            .unwrap();

        assert_eq!(
            store.get("http://localhost:7420").unwrap(),
            Some(cred("alice", "t1"))
        );
        assert_eq!(
            store.get("HTTP://LOCALHOST:7420").unwrap(),
            Some(cred("alice", "t1"))
        );
        assert_eq!(
            store.servers().unwrap(),
            vec!["http://localhost:7420", "https://wt.example.com"]
        );

        store
            .put("http://localhost:7420", &cred("alice", "t3"))
            .unwrap();
        assert_eq!(
            store.get("localhost:7420").unwrap(),
            Some(cred("alice", "t3"))
        );

        assert!(store.remove("localhost:7420").unwrap());
        assert_eq!(store.get("localhost:7420").unwrap(), None);
        assert_eq!(
            store.get("https://wt.example.com").unwrap(),
            Some(cred("bob", "t2"))
        );
    }

    #[test]
    fn file_format_is_keyed_by_server() {
        let (_dir, store) = store();
        store.put("localhost:7420", &cred("alice", "tok")).unwrap();
        let text = std::fs::read_to_string(store.path()).unwrap();
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            parsed["servers"]["http://localhost:7420"]["username"].as_str(),
            Some("alice")
        );
        assert_eq!(
            parsed["servers"]["http://localhost:7420"]["token"].as_str(),
            Some("tok")
        );
        // No temp files left behind.
        let leftovers: Vec<_> = std::fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() != CREDENTIALS_FILE)
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn corrupt_file_is_a_serialization_error() {
        let (_dir, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), "this is = = not toml").unwrap();
        assert!(matches!(
            store.get("localhost:7420"),
            Err(crate::SdkError::Serialization(_))
        ));
    }

    #[test]
    fn refresh_tokens_round_trip_and_old_files_still_parse() {
        let (_dir, store) = store();
        let mut c = cred("alice", "wt_access");
        c.refresh_token = Some("wtrt_refresh".into());
        c.expires_at = Some(
            chrono::DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        );
        store.put("localhost:7420", &c).unwrap();
        assert_eq!(store.get("localhost:7420").unwrap(), Some(c));
        let text = std::fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("refresh_token = \"wtrt_refresh\""), "{text}");

        // A pre-v0.8 file (no refresh token) and a plain token are unchanged.
        std::fs::write(
            store.path(),
            "[servers.\"http://localhost:7420\"]\nusername = \"bob\"\ntoken = \"wt_old\"\n",
        )
        .unwrap();
        assert_eq!(
            store.get("localhost:7420").unwrap(),
            Some(cred("bob", "wt_old"))
        );
        store.put("localhost:7420", &cred("bob", "wt_new")).unwrap();
        let text = std::fs::read_to_string(store.path()).unwrap();
        assert!(
            !text.contains("refresh_token") && !text.contains("expires_at"),
            "{text}"
        );
    }

    #[test]
    fn invalid_server_url_is_rejected() {
        let (_dir, store) = store();
        assert!(matches!(
            store.get("ftp://x"),
            Err(crate::SdkError::InvalidArgument(_))
        ));
        assert!(matches!(
            store.put("", &cred("a", "b")),
            Err(crate::SdkError::InvalidArgument(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn file_is_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, store) = store();
        store.put("localhost:7420", &cred("alice", "tok")).unwrap();
        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
