//! Declarative access configuration on the server (ENGINEERING_PLAN §13.7):
//! plan, apply and pull the canonical access document compiled by
//! [`declarative`] from `.wt/access/*.toml` or
//! `<tree>/.wt-tree/access/policies.toml`.
//!
//! [`plan`] and [`apply`] call `SyncAccessConfig` (config type
//! `ACCESS_CONFIG_TYPE_POLICIES`, content = canonical JSON, `tree_id` for
//! tree documents, `dry_run` for plans); [`pull`] calls `GetAccessConfig`.
//! The `*_with` variants take an existing [`SyncClient`].

use chrono::{DateTime, Utc};
use worktree_protocol::proto::sync as wire;

pub use worktree_protocol::access::attributes; // v0.8 ABAC (§16.5)
pub use worktree_protocol::access::declarative;
use worktree_protocol::access::declarative::Document;

use super::client::SyncClient;
use super::convert::{tenant_id, tree_id};
use super::credentials::CredentialStore;
use crate::error::{Result, SdkError};

/// Which document: a tenant's, or one tree's (by server tree name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessTarget {
    /// Tenant slug.
    pub tenant: String,
    /// Server name of the tree for tree-level documents.
    pub tree: Option<String>,
}

impl AccessTarget {
    pub fn tenant(slug: &str) -> Self {
        Self {
            tenant: slug.to_string(),
            tree: None,
        }
    }

    pub fn tree(tenant: &str, tree: &str) -> Self {
        Self {
            tenant: tenant.to_string(),
            tree: Some(tree.to_string()),
        }
    }
}

impl std::fmt::Display for AccessTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.tree {
            Some(tree) => write!(f, "tree {}/{tree}", self.tenant),
            None => write!(f, "tenant {}", self.tenant),
        }
    }
}

/// The server's answer to a plan or apply.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AccessPlan {
    /// The document is valid and (for apply) was applied.
    pub accepted: bool,
    /// Why the server refused the document (validation, escalation...).
    pub errors: Vec<String>,
    /// Human-readable plan, e.g. `policy protect-main: update (2 statements)`.
    pub changes: Vec<String>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// The last applied document, as returned by [`pull`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulledDocument {
    pub document: Document,
    /// The raw canonical JSON as stored.
    pub json: String,
    pub updated_by: String,
    pub updated_at: Option<DateTime<Utc>>,
}

async fn connect(server: &str) -> Result<SyncClient> {
    SyncClient::connect_with_store(&CredentialStore::default_location(), server).await
}

/// Validate `document_json` on the server and report what [`apply`] would
/// change, without changing anything.
pub async fn plan(server: &str, target: &AccessTarget, document_json: &str) -> Result<AccessPlan> {
    sync_with(&mut connect(server).await?, target, document_json, true).await
}

/// Apply `document_json`: the config-managed roles and policies of the
/// target become exactly the document's.
pub async fn apply(server: &str, target: &AccessTarget, document_json: &str) -> Result<AccessPlan> {
    sync_with(&mut connect(server).await?, target, document_json, false).await
}

/// The last applied document of the target (`None` if none was applied).
pub async fn pull(server: &str, target: &AccessTarget) -> Result<Option<PulledDocument>> {
    pull_with(&mut connect(server).await?, target).await
}

/// Resolve the tree of a tree-level target to its server id.
async fn target_tree(
    client: &mut SyncClient,
    target: &AccessTarget,
) -> Result<Option<wire::TreeId>> {
    match &target.tree {
        None => Ok(None),
        Some(name) => {
            let tree = client.resolve_tree(&target.tenant, name, false).await?;
            Ok(Some(tree_id(&tree.id)))
        }
    }
}

/// [`plan`] / [`apply`] over an existing connection.
pub async fn sync_with(
    client: &mut SyncClient,
    target: &AccessTarget,
    document_json: &str,
    dry_run: bool,
) -> Result<AccessPlan> {
    if !client.is_authenticated() {
        return Err(SdkError::NotLoggedIn(client.server().to_string()));
    }
    let content = document_json.as_bytes().to_vec();
    let req = wire::AccessConfigSyncRequest {
        tenant_id: Some(tenant_id(&target.tenant)),
        config_type: wire::AccessConfigType::Policies as i32,
        config_hash: Some(wire::ContentHash {
            value: blake3::hash(&content).as_bytes().to_vec(),
        }),
        content,
        account_id: None,
        tree_id: target_tree(client, target).await?,
        dry_run,
    };
    let resp = client.sync_access_config(req).await?;
    Ok(AccessPlan {
        accepted: resp.accepted,
        errors: resp.validation_errors,
        changes: resp.changes,
        dry_run,
    })
}

/// [`pull`] over an existing connection. Tree-level policy names come back
/// as written in the tree's file (without the server's `<tree>/` prefix).
pub async fn pull_with(
    client: &mut SyncClient,
    target: &AccessTarget,
) -> Result<Option<PulledDocument>> {
    if !client.is_authenticated() {
        return Err(SdkError::NotLoggedIn(client.server().to_string()));
    }
    let req = wire::GetAccessConfigRequest {
        tenant_id: Some(tenant_id(&target.tenant)),
        tree_id: target_tree(client, target).await?,
        config_type: wire::AccessConfigType::Policies as i32,
    };
    let resp = client.get_access_config(req).await?;
    if !resp.found {
        return Ok(None);
    }
    let mut document = Document::from_json(&resp.content).map_err(SdkError::Remote)?;
    if let Some(tree) = &target.tree {
        declarative::strip_tree_prefix(&mut document.policies, tree);
    }
    Ok(Some(PulledDocument {
        document,
        json: String::from_utf8_lossy(&resp.content).into_owned(),
        updated_by: resp.updated_by,
        updated_at: resp
            .updated_at
            .and_then(|t| DateTime::from_timestamp(t.seconds, t.nanos.max(0) as u32)),
    }))
}
