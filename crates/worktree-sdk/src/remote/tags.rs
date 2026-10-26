//! v0.7 trust (ENGINEERING_PLAN §15.8): push one tag and read server tags.
//!
//! `push` sends every missing tag but only logs tag failures (the branch
//! push already succeeded); [`push_tag`] pushes one tag and reports the
//! server's refusal (for example a tag protection with `require_signed`).

use super::session::{self, Access};
use crate::error::{Result, SdkError};
use crate::model::TagInfo;
use crate::repo::Repository;

/// Push the local tag `name` of `tree` to `remote`; `Remote` with the
/// server's reason when it is refused, `TagNotFound` when it does not
/// exist locally.
pub async fn push_tag(repo: &Repository, tree: &str, remote: &str, name: &str) -> Result<TagInfo> {
    let tree_name = repo.tree(tree)?.name;
    let tag = repo
        .find_tag(&tree_name, name)?
        .ok_or_else(|| SdkError::TagNotFound(name.to_string()))?;
    let mut session = session::open(repo, &tree_name, remote, Access::Write).await?;
    session.client.push_tag(&session.tree.id, &tag).await?;
    Ok(tag)
}

/// The tags of `tree` on `remote` (signatures included).
pub async fn remote_tags(repo: &Repository, tree: &str, remote: &str) -> Result<Vec<TagInfo>> {
    let tree_name = repo.tree(tree)?.name;
    let mut session = session::open(repo, &tree_name, remote, Access::Read).await?;
    session.client.list_tags(&session.tree.id).await
}
