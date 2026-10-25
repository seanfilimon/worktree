//! Where `wt git import` reads from: a local repository path is opened in
//! place; any URL (`https://`, `ssh://`, scp-like `user@host:path`,
//! `file://`) is first mirrored into a temporary bare repository that is
//! deleted afterwards.

use std::path::{Path, PathBuf};

use crate::error::{GitCompatError, Result};
use crate::remote::auth::GitAuth;
use crate::remote::transport::GitTransport;
use crate::remote::{fetch_mirror, FetchReport};

/// Whether `source` names a repository on the local filesystem by path
/// (not a `file://` URL).
pub fn is_path(source: &str) -> bool {
    matches!(GitTransport::from_url(source), Some(GitTransport::Local(_)))
        && !source.starts_with("file://")
}

/// Whether `source` is on the local filesystem (a path or a `file://` URL).
pub fn is_local(source: &str) -> bool {
    matches!(GitTransport::from_url(source), Some(GitTransport::Local(_)))
}

/// Filesystem path of a local source (`file://` prefix removed).
pub fn local_path(source: &str) -> PathBuf {
    match source.strip_prefix("file://") {
        // file:///C:/repo -> C:/repo on Windows, /repo elsewhere.
        Some(rest) if cfg!(windows) && rest.len() > 2 && rest.as_bytes()[2] == b':' => {
            PathBuf::from(&rest[1..])
        }
        Some(rest) => PathBuf::from(rest),
        None => PathBuf::from(source),
    }
}

/// `file://` URL for a local path (forward slashes, as libgit2 expects).
pub fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// An opened import source.
pub struct GitSource {
    path: PathBuf,
    temp: Option<TempMirror>,
    /// Refs mirrored from a URL (`None` for a local path).
    pub fetched: Option<FetchReport>,
}

impl GitSource {
    /// Open `source`: a local path is used directly (relative paths resolve
    /// against `cwd`); a URL is mirrored into a temporary bare repository.
    pub fn open(source: &str, cwd: &Path, auth: &GitAuth) -> Result<Self> {
        if GitTransport::from_url(source).is_none() {
            return Err(GitCompatError::RemoteError(format!(
                "unsupported Git URL: {source}"
            )));
        }
        if is_path(source) {
            let path = cwd.join(source);
            git2::Repository::open(&path)?;
            return Ok(Self {
                path,
                temp: None,
                fetched: None,
            });
        }
        let temp = TempMirror::new()?;
        let mirror = temp.dir.join("mirror.git");
        let fetched = fetch_mirror(&mirror, source, auth)?;
        Ok(Self {
            path: mirror,
            temp: Some(temp),
            fetched: Some(fetched),
        })
    }

    /// The Git repository to read.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the source was mirrored from a URL.
    pub fn is_mirror(&self) -> bool {
        self.temp.is_some()
    }
}

/// A temporary directory removed on drop (read-only pack files included).
struct TempMirror {
    dir: PathBuf,
}

impl TempMirror {
    fn new() -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("wt-git-import-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }
}

impl Drop for TempMirror {
    fn drop(&mut self) {
        if let Err(e) = remove_tree(&self.dir) {
            tracing::warn!(path = %self.dir.display(), error = %e, "could not remove the temporary Git mirror");
        }
    }
}

/// `remove_dir_all` that first clears read-only flags (libgit2 writes pack
/// files read-only, which Windows refuses to delete).
pub(crate) fn remove_tree(dir: &Path) -> std::io::Result<()> {
    fn writable(dir: &Path) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = entry.metadata()?;
            if meta.is_dir() {
                writable(&path)?;
            } else {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perms.set_readonly(false);
                    std::fs::set_permissions(&path, perms)?;
                }
            }
        }
        Ok(())
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => {
            writable(dir)?;
            std::fs::remove_dir_all(dir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{GitFixture, Spec};

    #[test]
    fn classifies_sources() {
        assert!(is_path("../repo"));
        assert!(is_path("C:\\src\\repo"));
        assert!(is_path("/srv/repo.git"));
        assert!(!is_path("file:///srv/repo.git"));
        assert!(is_local("file:///srv/repo.git"));
        assert!(!is_local("https://example.com/r.git"));
        assert!(!is_path("git@example.com:org/r.git"));
        assert_eq!(local_path("file:///srv/r"), PathBuf::from("/srv/r"));
        assert_eq!(local_path("../r"), PathBuf::from("../r"));
        if cfg!(windows) {
            assert_eq!(local_path("file:///C:/r"), PathBuf::from("C:/r"));
            assert_eq!(file_url(Path::new("C:\\a\\b")), "file:///C:/a/b");
        } else {
            assert_eq!(file_url(Path::new("/a/b")), "file:///a/b");
        }
    }

    #[test]
    fn url_sources_are_mirrored_and_cleaned_up() {
        let fx = GitFixture::new();
        let c1 = fx.commit(
            Some("refs/heads/main"),
            &[Spec::File("a", b"1")],
            &[],
            "c1",
            1,
        );
        let cwd = tempfile::tempdir().unwrap();
        let source = GitSource::open(&file_url(fx.path()), cwd.path(), &GitAuth::auto()).unwrap();
        assert!(source.is_mirror());
        let mirror = source.path().to_path_buf();
        let git = git2::Repository::open(&mirror).unwrap();
        assert_eq!(git.refname_to_id("refs/heads/main").unwrap(), c1);
        drop(git);
        assert_eq!(source.fetched.as_ref().unwrap().branches, vec!["main"]);
        drop(source);
        assert!(!mirror.exists(), "temporary mirror removed");

        // Plain paths are opened in place, relative to cwd.
        let parent = fx.path().parent().unwrap();
        let name = fx.path().file_name().unwrap().to_str().unwrap();
        let source = GitSource::open(name, parent, &GitAuth::auto()).unwrap();
        assert!(!source.is_mirror());
        assert_eq!(source.path(), fx.path());

        assert!(GitSource::open("ftp://x/y", cwd.path(), &GitAuth::auto()).is_err());
        assert!(GitSource::open("missing", cwd.path(), &GitAuth::auto()).is_err());
    }
}
