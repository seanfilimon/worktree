//! history-v2 (ENGINEERING_PLAN §16.10): `wt push --force` with lease
//! semantics, the protection bypass, push idempotency and remote tag
//! deletion.
//!
//! - A force push rewinds a server branch whose tip the local branch does
//!   not contain. It is refused locally unless the server tip is the last
//!   known remote-tracking tip (`refs/remotes/<remote>/<branch>`, updated by
//!   fetch and push): you never overwrite snapshots you have not seen. The
//!   request carries that tip as `old_tip`, and the server only moves the
//!   branch while it is still there.
//! - Every push carries a fresh `operation_id`; a push whose connection
//!   fails is retried once with the same id, and the server answers the
//!   retry with the first result instead of pushing twice.
//! - `bypass_protection` sends `x-wt-bypass-protection: true`; owners
//!   (`branch:bypass_protection`) then push past branch protection. The
//!   server audits every bypass.

use std::time::Duration;

use worktree_protocol::proto::sync as wire;

use super::session::{self, Access, RemoteSession};
use super::PushReport;
use crate::error::{Result, SdkError};
use crate::repo::Repository;

/// Options of [`push_with_options`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushOptions {
    /// Rewind the server branch when it has snapshots the local branch
    /// lacks (`wt push --force`); refused unless the server tip is the last
    /// known remote tip (lease).
    pub force: bool,
    /// Ask the server to bypass branch protection (`--bypass-protection`;
    /// needs `branch:bypass_protection`).
    pub bypass_protection: bool,
}

/// [`super::push`] with [`PushOptions`].
pub async fn push_with_options(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
    opts: &PushOptions,
) -> Result<PushReport> {
    Ok(super::push::push_with(repo, tree, remote, branch, opts)
        .await?
        .0)
}

/// Delete tag `name` of `tree` on `remote` (`wt tag delete --remote`); the
/// local tag is not touched. `Remote` with the server's reason when refused.
pub async fn delete_remote_tag(
    repo: &Repository,
    tree: &str,
    remote: &str,
    name: &str,
) -> Result<()> {
    let tree_name = repo.tree(tree)?.name;
    let mut session = session::open(repo, &tree_name, remote, Access::Write).await?;
    session.client.delete_tag(&session.tree.id, name).await
}

/// The lease of a force push: the server tip must be the last known tip of
/// `<remote>/<branch>`.
pub(crate) fn check_lease(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: &str,
    server_tip: &str,
) -> Result<()> {
    let known = repo.remote_tip(tree, remote, branch)?;
    if known.as_deref() == Some(server_tip) {
        return Ok(());
    }
    Err(SdkError::PushRejected(lease_message(
        remote,
        branch,
        known.as_deref(),
        server_tip,
    )))
}

fn lease_message(remote: &str, branch: &str, known: Option<&str>, server_tip: &str) -> String {
    format!(
        "refusing to force-push {remote}/{branch}: the server tip {} is not the last tip you fetched ({}); \
         run `wt fetch` and review the remote history before forcing",
        short(server_tip),
        known.map(short).unwrap_or("none")
    )
}

/// Send `req` with a fresh operation id; retry once with the same id when
/// the server could not be reached.
pub(crate) async fn send_push(
    session: &mut RemoteSession,
    mut req: wire::PushRequest,
    opts: &PushOptions,
) -> Result<wire::PushResponse> {
    if req.operation_id.is_empty() {
        req.operation_id = uuid::Uuid::new_v4().to_string();
    }
    match session
        .client
        .push_with_bypass(req.clone(), opts.bypass_protection)
        .await
    {
        Err(e) if retryable(&e) => {
            tracing::warn!(error = %e, "push failed; retrying once with the same operation id");
            tokio::time::sleep(Duration::from_millis(500)).await;
            session
                .client
                .push_with_bypass(req, opts.bypass_protection)
                .await
        }
        other => other,
    }
}

fn retryable(err: &SdkError) -> bool {
    let SdkError::Remote(m) = err else {
        return false;
    };
    let m = m.to_ascii_lowercase();
    m.contains("transport error") || m.contains("connection reset") || m.contains("broken pipe")
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_message_explains() {
        let m = lease_message("origin", "main", Some("0123456789"), "abcdef0123");
        assert!(
            m.contains("origin/main") && m.contains("abcdef01") && m.contains("01234567"),
            "{m}"
        );
        assert!(m.contains("wt fetch"), "{m}");
        assert!(lease_message("o", "b", None, "x").contains("none"));
    }

    #[test]
    fn only_transport_failures_are_retried() {
        assert!(retryable(&SdkError::Remote("transport error".into())));
        assert!(!retryable(&SdkError::Remote("permission denied".into())));
        assert!(!retryable(&SdkError::Other("transport error".into())));
    }
}
