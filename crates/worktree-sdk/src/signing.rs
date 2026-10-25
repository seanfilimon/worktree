//! Ed25519 snapshot signing.
//!
//! The signed payload is the protobuf encoding of the snapshot's
//! `SnapshotRecord` with `signature`, `signing_key_id`, `tree_id` and
//! `branch_name` cleared (the server files records under its own tree id,
//! and the branch is informational). See docs/ENGINEERING_PLAN.md §13.6.
//!
//! v0.5 stub: the owning workstream implements key storage
//! (`~/.config/worktree/keys/`), `[signing]` configuration and signing.

use crate::error::Result;
use crate::model::Snapshot;
use crate::repo::Repository;

/// Sign `snapshot` in place when signing is configured for the repository
/// (sets `snapshot.signature`); a no-op otherwise.
pub fn sign_snapshot(_repo: &Repository, _snapshot: &mut Snapshot) -> Result<()> {
    Ok(())
}

// ---- BEGIN merge-diff workstream block (ENGINEERING_PLAN §13.10) ----
// Stub used by `wt log --show-signature` until the `signing` workstream
// implements local verification; keep the signature exactly as below.

/// Result of verifying a snapshot's signature locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    /// The snapshot carries no signature.
    Unsigned,
    /// The signature verifies against a known key.
    Verified { key_id: String },
    /// The signature does not verify (reason).
    Invalid(String),
    /// Signed with a key that is not known locally (key id).
    UnknownKey(String),
}

/// Verify `snapshot`'s signature against the locally known keys.
/// Stub: signed snapshots are reported as `UnknownKey` (unverified).
pub fn verify_local(_repo: &Repository, snapshot: &Snapshot) -> Result<SignatureStatus> {
    Ok(match &snapshot.signature {
        None => SignatureStatus::Unsigned,
        Some(sig) => SignatureStatus::UnknownKey(sig.key_id.clone()),
    })
}
// ---- END merge-diff workstream block ----

/// The canonical bytes a snapshot signature covers.
pub fn signed_payload(snapshot: &Snapshot) -> Vec<u8> {
    use prost::Message;
    let mut rec = snapshot.to_record();
    rec.signature.clear();
    rec.signing_key_id.clear();
    rec.tree_id = None;
    rec.branch_name.clear();
    rec.encode_to_vec()
}
