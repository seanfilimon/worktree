//! `clone`: create a new local worktree from a server tree.

use std::path::Path;

use super::client::{RemoteTree, SyncClient};
use super::credentials::CredentialStore;
use super::fetch::fetch_with;
use super::server_url::normalize_server_url;
use super::session::RemoteSession;
use crate::config::RemoteSection;
use crate::error::{Result, SdkError};
use crate::model::InitOptions;
use crate::repo::Repository;

/// Name of the remote a clone is configured with.
pub const ORIGIN: &str = "origin";

/// State of the clone destination before we touched it, for cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Destination {
    /// Did not exist; created by the clone.
    Missing,
    /// Existed as an empty directory.
    Empty,
}

/// `InvalidArgument` unless `dest` is absent or an empty directory.
pub(crate) fn check_destination(dest: &Path) -> Result<Destination> {
    match std::fs::metadata(dest) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Destination::Missing),
        Err(e) => Err(e.into()),
        Ok(meta) if !meta.is_dir() => Err(SdkError::InvalidArgument(format!(
            "clone destination {} exists and is not a directory",
            dest.display()
        ))),
        Ok(_) => {
            if std::fs::read_dir(dest)?.next().is_some() {
                Err(SdkError::InvalidArgument(format!(
                    "clone destination {} is not empty",
                    dest.display()
                )))
            } else {
                Ok(Destination::Empty)
            }
        }
    }
}

/// Undo a failed clone: remove what it created, never anything else.
pub(crate) fn cleanup(dest: &Path, state: Destination) {
    let result = match state {
        Destination::Missing => std::fs::remove_dir_all(dest),
        Destination::Empty => std::fs::read_dir(dest).and_then(|entries| {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                // file_type() does not follow symlinks: a link is removed, not its target.
                if entry.file_type()?.is_dir() {
                    std::fs::remove_dir_all(&path)?;
                } else {
                    std::fs::remove_file(&path)?;
                }
            }
            Ok(())
        }),
    };
    if let Err(e) = result {
        tracing::warn!(dest = %dest.display(), error = %e, "could not clean up after failed clone");
    }
}

pub(crate) async fn clone(
    server: &str,
    tenant: &str,
    tree_name: &str,
    dest: &Path,
) -> Result<Repository> {
    let server = normalize_server_url(server)?;
    if tenant.trim().is_empty() || tree_name.trim().is_empty() {
        return Err(SdkError::InvalidArgument(
            "clone needs <tenant>/<tree>".into(),
        ));
    }
    let state = check_destination(dest)?;

    let cred = CredentialStore::default_location().get(&server)?;
    let mut client = SyncClient::connect(&server, cred.as_ref().map(|c| c.token.as_str())).await?;
    // Resolve before creating anything, so a typo leaves no directory behind.
    let tree = client.resolve_tree(tenant, tree_name, false).await?;
    let session = RemoteSession {
        client,
        tree,
        username: cred.map(|c| c.username),
    };

    match clone_into(session, &server, tenant, dest).await {
        Ok(repo) => Ok(repo),
        Err(e) => {
            cleanup(dest, state);
            Err(e)
        }
    }
}

async fn clone_into(
    mut session: RemoteSession,
    server: &str,
    tenant: &str,
    dest: &Path,
) -> Result<Repository> {
    let RemoteTree {
        id,
        name,
        default_branch,
        ..
    } = session.tree.clone();
    let default_branch = if default_branch.is_empty() {
        "main".to_string()
    } else {
        default_branch
    };

    let mut repo = Repository::init(
        dest,
        InitOptions {
            name: Some(name.clone()),
            default_branch: Some(default_branch.clone()),
        },
    )?;
    let mut config = repo.config().clone();
    config.remote.insert(
        ORIGIN.to_string(),
        RemoteSection {
            url: server.to_string(),
            tenant: tenant.to_string(),
        },
    );
    repo.set_config(config)?;

    let root = repo.root_tree()?.name;
    repo.set_tree_id(&root, &id)?;
    repo.set_tree_remote_name(&root, &name)?;

    let (_, branches) = fetch_with(&repo, &root, ORIGIN, &mut session).await?;
    for branch in &branches {
        repo.set_branch_tip(&root, &branch.name, branch.tip.as_deref())?;
    }
    let head = repo.current_branch(&root)?;
    if let Some(tip) = repo.branch_tip(&root, &head)? {
        repo.checkout(&root, &tip, true)?;
    }
    Ok(repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_states() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("new");
        assert_eq!(check_destination(&missing).unwrap(), Destination::Missing);

        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert_eq!(check_destination(&empty).unwrap(), Destination::Empty);

        let full = dir.path().join("full");
        std::fs::create_dir(&full).unwrap();
        std::fs::write(full.join("f"), "x").unwrap();
        assert!(
            matches!(check_destination(&full), Err(SdkError::InvalidArgument(m)) if m.contains("not empty"))
        );

        let file = dir.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(
            matches!(check_destination(&file), Err(SdkError::InvalidArgument(m)) if m.contains("not a directory"))
        );
    }

    #[test]
    fn cleanup_removes_only_what_clone_created() {
        let dir = tempfile::tempdir().unwrap();

        let created = dir.path().join("created");
        std::fs::create_dir_all(created.join(".wt/objects")).unwrap();
        std::fs::write(created.join("a.txt"), "x").unwrap();
        cleanup(&created, Destination::Missing);
        assert!(!created.exists());

        let existing = dir.path().join("existing");
        std::fs::create_dir_all(existing.join(".wt/objects")).unwrap();
        std::fs::write(existing.join("a.txt"), "x").unwrap();
        cleanup(&existing, Destination::Empty);
        assert!(existing.is_dir(), "a pre-existing directory is kept");
        assert_eq!(std::fs::read_dir(&existing).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn rejects_bad_arguments_before_connecting() {
        let dir = tempfile::tempdir().unwrap();
        let err = clone("http://127.0.0.1:1", "", "tree", &dir.path().join("x"))
            .await
            .expect_err("clone must fail");
        assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
        let err = clone("ftp://nope", "t", "tree", &dir.path().join("x"))
            .await
            .expect_err("clone must fail");
        assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
        std::fs::write(dir.path().join("f"), "x").unwrap();
        let err = clone("http://127.0.0.1:1", "t", "tree", dir.path())
            .await
            .expect_err("clone must fail");
        assert!(matches!(err, SdkError::InvalidArgument(m) if m.contains("not empty")));
    }
}
