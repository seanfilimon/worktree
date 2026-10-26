//! Opening a connection for one local tree + configured remote.
//!
//! device-auth (ENGINEERING_PLAN §16.6): access tokens expire after an
//! hour. [`saved_credential`] renews one that is about to expire before
//! connecting, and [`open`] renews and reconnects once when the server
//! rejects the token anyway, so a saved login keeps working for the 30
//! days of its refresh token.

use super::auth;
use super::client::{RemoteTree, SyncClient};
use super::credentials::CredentialStore;
use super::server_url::normalize_server_url;
use super::Credential;
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

/// The credential saved in `store` for `server`, renewed first when its
/// access token is about to expire.
pub(crate) async fn saved_credential(
    store: &CredentialStore,
    server: &str,
) -> Result<Option<Credential>> {
    auth::fresh_credential(store, server).await
}

/// After the server rejected the saved token of `server` (`failed`, if
/// known): renew it with the refresh token and connect again. `None` when
/// it cannot be renewed (the caller reports the original error).
pub(crate) async fn reconnect_after_refresh(
    store: &CredentialStore,
    server: &str,
    failed: Option<&str>,
) -> Result<Option<SyncClient>> {
    match auth::refresh(store, server, failed).await? {
        Some(cred) if failed != Some(cred.token.as_str()) => {
            Ok(Some(SyncClient::connect(server, Some(&cred.token)).await?))
        }
        _ => Ok(None),
    }
}

/// Connect to `server` with the saved credential and resolve
/// `<tenant>/<name>`, renewing an expired access token once.
pub(crate) async fn connect_and_resolve(
    server: &str,
    tenant: &str,
    name: &str,
    create_if_missing: bool,
    require_login: bool,
) -> Result<(SyncClient, RemoteTree, Option<Credential>)> {
    let store = CredentialStore::default_location();
    let mut cred = saved_credential(&store, server).await?;
    if require_login && cred.is_none() {
        return Err(SdkError::NotLoggedIn(server.to_string()));
    }
    let mut client = SyncClient::connect(server, cred.as_ref().map(|c| c.token.as_str())).await?;
    let resolved = match client.resolve_tree(tenant, name, create_if_missing).await {
        Err(SdkError::NotLoggedIn(s)) if cred.is_some() => {
            let failed = cred.as_ref().map(|c| c.token.clone());
            match reconnect_after_refresh(&store, server, failed.as_deref()).await? {
                Some(fresh) => {
                    client = fresh;
                    cred = store.get(server)?;
                    client.resolve_tree(tenant, name, create_if_missing).await?
                }
                None => return Err(SdkError::NotLoggedIn(s)),
            }
        }
        other => other?,
    };
    Ok((client, resolved, cred))
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
    let remote_name = if info.remote_name.is_empty() {
        info.name.as_str()
    } else {
        info.remote_name.as_str()
    };
    let write = access == Access::Write;
    let (client, resolved, cred) =
        connect_and_resolve(&server, &tenant, remote_name, write, write).await?;
    if info.id != resolved.id {
        repo.set_tree_id(&info.name, &resolved.id)?;
    }
    Ok(RemoteSession {
        client,
        tree: resolved,
        username: cred.map(|c| c.username),
    })
}
