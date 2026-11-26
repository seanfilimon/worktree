//! The hidden W0rkTree worktree a helper remote keeps inside the Git
//! repository (`$GIT_DIR/wt/<remote>/`).
//!
//! It is an ordinary worktree created with the SDK — `.wt/` with objects,
//! snapshots and remote-tracking refs — whose files are never checked out.
//! Fetches download into it incrementally, pushes are staged in it, and
//! its `.wt/git-map.json` remembers which commit stands for which snapshot
//! in this Git repository. Deleting the directory is safe: the next fetch
//! rebuilds it.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::{InitOptions, Repository};

use super::target::Target;
use crate::error::{GitCompatError, Result};

/// Name of the cache worktree's only remote.
pub(crate) const ORIGIN: &str = worktree_sdk::remote::ORIGIN;

/// File in the cache's `.wt/` remembering pushed annotated tag objects.
const TAG_OBJECTS_FILE: &str = "git-tags.json";

const TAG_OBJECTS_VERSION: u32 = 1;

/// An open cache worktree and the name of its (root) tree.
pub(crate) struct Cache {
    pub repo: Repository,
    pub tree: String,
}

/// Directory name of the cache for the remote Git calls `alias`.
///
/// Remote names map to themselves (bytes outside `[A-Za-z0-9._-]` are
/// percent-encoded); when Git has no name for the remote (`git fetch
/// wt::https://...`) it passes the URL, which maps to `url-<hash>`.
pub(crate) fn dir_name(alias: &str, address: &str) -> String {
    let anonymous = alias.is_empty() || alias == address || alias.contains("://");
    if anonymous {
        let hash = worktree_sdk::store::hash_bytes(address.as_bytes()).to_hex();
        return format!("url-{}", &hash[..16]);
    }
    let mut name = String::with_capacity(alias.len());
    for byte in alias.bytes() {
        let plain = byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') || byte == b'.';
        if plain {
            name.push(char::from(byte));
        } else {
            name.push_str(&format!("%{byte:02X}"));
        }
    }
    // "." and ".." are directories, and "url-" is the anonymous namespace.
    if name.chars().all(|c| c == '.') || name.starts_with("url-") {
        name = format!("%{name}");
    }
    name
}

/// Open the cache at `dir`, creating it for `target` if it does not exist.
///
/// Errors with `RemoteError` when the directory holds the cache of a
/// different server tree (the remote's URL was changed): mixing two
/// histories in one cache would corrupt the commit mapping.
pub(crate) fn open_or_create(dir: &Path, target: &Target) -> Result<Cache> {
    // `Repository::open` walks up the directory tree, so only call it when
    // the cache itself exists (the Git repository may live in a worktree).
    if !dir.join(".wt").is_dir() {
        return create(dir, target);
    }
    let repo = Repository::open(dir)?;
    let root = repo.root_tree()?;
    let remote_name = if root.remote_name.is_empty() {
        root.name.as_str()
    } else {
        root.remote_name.as_str()
    };
    let same = repo
        .config()
        .remote
        .get(ORIGIN)
        .is_some_and(|section| section.url == target.server && section.tenant == target.tenant)
        && remote_name == target.tree;
    if !same {
        return Err(GitCompatError::RemoteError(format!(
            "{} is the cache of a different W0rkTree tree than {}/{}; \
             if the remote URL was changed on purpose, delete that directory and fetch again",
            dir.display(),
            target.server,
            target.spec()
        )));
    }
    Ok(Cache {
        tree: root.name,
        repo,
    })
}

fn create(dir: &Path, target: &Target) -> Result<Cache> {
    let mut repo = Repository::init(
        dir,
        InitOptions {
            name: Some(target.tree.clone()),
            default_branch: None,
        },
    )?;
    let mut config = repo.config().clone();
    config.remote.insert(
        ORIGIN.to_owned(),
        RemoteSection {
            url: target.server.clone(),
            tenant: target.tenant.clone(),
        },
    );
    repo.set_config(config)?;
    let tree = repo.root_tree()?.name;
    repo.set_tree_remote_name(&tree, &target.tree)?;
    Ok(Cache { repo, tree })
}

/// Annotated tag objects this Git repository pushed, by tag name.
///
/// A W0rkTree tag keeps a tagger and a message but not Git's tag object
/// (time zone, signature). Remembering the pushed object lets later
/// fetches advertise the very object the user already has instead of a
/// rebuilt one that Git would report as a clobbered tag.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TagObjects {
    tags: BTreeMap<String, TagObject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TagObject {
    /// SHA of the Git tag object.
    object: String,
    /// Snapshot the tag pointed at when it was pushed.
    target: String,
}

#[derive(Serialize, Deserialize)]
struct TagObjectsFile {
    version: u32,
    tags: BTreeMap<String, TagObject>,
}

impl TagObjects {
    fn path(wt_dir: &Path) -> PathBuf {
        wt_dir.join(TAG_OBJECTS_FILE)
    }

    /// Load the file; missing means empty. A malformed or newer file is a
    /// `MapError`.
    pub fn load(wt_dir: &Path) -> Result<Self> {
        let path = Self::path(wt_dir);
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let file: TagObjectsFile = serde_json::from_slice(&data)
            .map_err(|e| GitCompatError::MapError(format!("{}: {e}", path.display())))?;
        if file.version != TAG_OBJECTS_VERSION {
            return Err(GitCompatError::MapError(format!(
                "{}: unsupported version {}",
                path.display(),
                file.version
            )));
        }
        Ok(Self { tags: file.tags })
    }

    /// Persist atomically (temp file + rename).
    pub fn save(&self, wt_dir: &Path) -> Result<()> {
        let path = Self::path(wt_dir);
        let file = TagObjectsFile {
            version: TAG_OBJECTS_VERSION,
            tags: self.tags.clone(),
        };
        let mut data = serde_json::to_vec_pretty(&file)
            .map_err(|e| GitCompatError::MapError(e.to_string()))?;
        data.push(b'\n');
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &data)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// The tag object pushed as `name`, if the tag still points at `target`.
    pub fn object_for(&self, name: &str, target: &str) -> Option<&str> {
        self.tags
            .get(name)
            .filter(|tag| tag.target == target)
            .map(|tag| tag.object.as_str())
    }

    /// Remember that `object` was pushed as tag `name` of snapshot `target`.
    pub fn record(&mut self, name: &str, object: &str, target: &str) {
        self.tags.insert(
            name.to_owned(),
            TagObject {
                object: object.to_owned(),
                target: target.to_owned(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(tree: &str) -> Target {
        Target {
            server: "http://localhost:7420".into(),
            tenant: "acme".into(),
            tree: tree.into(),
        }
    }

    #[test]
    fn remote_names_become_safe_directory_names() {
        let url = "https://wt.example.com/acme/web";
        assert_eq!(dir_name("origin", url), "origin");
        assert_eq!(dir_name("up.stream_2-x", url), "up.stream_2-x");
        assert_eq!(dir_name("team/fork", url), "team%2Ffork");
        assert_eq!(dir_name("..", url), "%..");
        assert_eq!(dir_name("url-x", url), "%url-x");

        let anonymous = dir_name(url, url);
        assert!(anonymous.starts_with("url-") && anonymous.len() == 20);
        assert_eq!(anonymous, dir_name(&format!("wt::{url}"), url));
        assert_eq!(anonymous, dir_name("", url));
        assert_ne!(anonymous, dir_name("", "https://wt.example.com/acme/api"));
    }

    #[test]
    fn creates_then_reopens_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wt").join("origin");
        let created = open_or_create(&path, &target("web")).unwrap();
        assert!(path.join(".wt").is_dir());
        let section = &created.repo.config().remote[ORIGIN];
        assert_eq!(section.url, "http://localhost:7420");
        assert_eq!(section.tenant, "acme");
        assert_eq!(created.repo.tree(&created.tree).unwrap().remote_name, "web");
        drop(created);

        let reopened = open_or_create(&path, &target("web")).unwrap();
        assert_eq!(reopened.tree, "web");
    }

    #[test]
    fn refuses_the_cache_of_another_tree() {
        let dir = tempfile::tempdir().unwrap();
        open_or_create(dir.path(), &target("web")).unwrap();
        let err = open_or_create(dir.path(), &target("api")).err().unwrap();
        assert!(
            matches!(&err, GitCompatError::RemoteError(m) if m.contains("different W0rkTree tree")),
            "{err}"
        );
        let mut other = target("web");
        other.server = "http://localhost:7421".into();
        assert!(open_or_create(dir.path(), &other).is_err());
    }

    #[test]
    fn tag_objects_round_trip_and_follow_the_target() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(TagObjects::load(dir.path()).unwrap(), TagObjects::default());

        let mut tags = TagObjects::default();
        tags.record("v1", "aaaa", "snap-1");
        tags.save(dir.path()).unwrap();
        let loaded = TagObjects::load(dir.path()).unwrap();
        assert_eq!(loaded, tags);
        assert_eq!(loaded.object_for("v1", "snap-1"), Some("aaaa"));
        // The tag was re-created elsewhere on another snapshot.
        assert_eq!(loaded.object_for("v1", "snap-2"), None);
        assert_eq!(loaded.object_for("v2", "snap-1"), None);

        fs::write(
            dir.path().join(TAG_OBJECTS_FILE),
            "{\"version\":9,\"tags\":{}}",
        )
        .unwrap();
        assert!(matches!(
            TagObjects::load(dir.path()),
            Err(GitCompatError::MapError(_))
        ));
    }
}
