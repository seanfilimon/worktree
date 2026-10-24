//! Opening a connection for one local tree + configured remote.

use super::client::{RemoteTree, SyncClient};
use super::credentials::CredentialStore;
use super::server_url::normalize_server_url;
use crate::error::{Result, SdkError};
use crate::repo::Repository;

/// What the caller is about to do; decides login and tree-creation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// Anonymous allowed (public trees); the tree must already exist.
    Read,
    /// Requires a saved credential; creates the server tree if missing.
    Write,
}

/// A connected client plus the resolved server tree.
pub(crate) struct RemoteSession {
    pub client: SyncClient,
    pub tree: RemoteTree,
    /// Username of the saved credential, if any.
    pub username: Option<String>,
}

/// `(normalized server URL, tenant slug)` of `[remote.<name>]`.
pub(crate) fn remote_target(repo: &Repository, remote: &str) -> Result<(String, String)> {
    let section = repo
        .config()
        .remote
        .get(remote)
        .ok_or_else(|| SdkError::RemoteNotConfigured(remote.to_string()))?;
    if section.tenant.trim().is_empty() {
        return Err(SdkError::InvalidConfig(format!(
            "remote.{remote}.tenant is empty"
        )));
    }
    Ok((normalize_server_url(&section.url)?, section.tenant.clone()))
}

/// Connect to `remote` and resolve the server tree backing local `tree`.
///
/// Once resolved, the local tree id is linked to the server's tree id
/// (`Repository::set_tree_id`) so snapshot records agree on both sides.
pub(crate) async fn open(
    repo: &Repository,
    tree: &str,
    remote: &str,
    access: Access,
) -> Result<RemoteSession> {
    let (server, tenant) = remote_target(repo, remote)?;
    let info = repo.tree(tree)?;
    let cred = CredentialStore::default_location().get(&server)?;
    if access == Access::Write && cred.is_none() {
        return Err(SdkError::NotLoggedIn(server));
    }
    let mut client = SyncClient::connect(&server, cred.as_ref().map(|c| c.token.as_str())).await?;
    let remote_name = if info.remote_name.is_empty() {
        info.name.as_str()
    } else {
        info.remote_name.as_str()
    };
    let resolved = client
        .resolve_tree(&tenant, remote_name, access == Access::Write)
        .await?;
    if info.id != resolved.id {
        repo.set_tree_id(&info.name, &resolved.id)?;
    }
    Ok(RemoteSession {
        client,
        tree: resolved,
        username: cred.map(|c| c.username),
    })
}
