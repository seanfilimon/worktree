//! Conversions between SDK types and `worktree.sync.v1` wire messages.

use chrono::{DateTime, Utc};
use worktree_protocol::proto::sync as wire;

use super::StagedInfo;
use crate::error::{Result, SdkError};
use crate::model::TagInfo;
use crate::store::ObjectHash;

pub(crate) fn wire_hash(hash: &ObjectHash) -> wire::ContentHash {
    wire::ContentHash {
        value: hash.as_bytes().to_vec(),
    }
}

/// Decode a wire hash; `Remote` if absent or not 32 bytes.
pub(crate) fn hash_from_wire(hash: Option<&wire::ContentHash>) -> Result<ObjectHash> {
    let hash =
        hash.ok_or_else(|| SdkError::Remote("server sent an object without a hash".into()))?;
    let bytes: [u8; 32] = hash.value.as_slice().try_into().map_err(|_| {
        SdkError::Remote(format!(
            "server sent a {}-byte hash (expected 32)",
            hash.value.len()
        ))
    })?;
    Ok(ObjectHash::from_bytes(bytes))
}

pub(crate) fn snapshot_id(id: &str) -> wire::SnapshotId {
    wire::SnapshotId {
        value: id.to_string(),
    }
}

pub(crate) fn tree_id(id: &str) -> wire::TreeId {
    wire::TreeId {
        value: id.to_string(),
    }
}

pub(crate) fn tenant_id(id: &str) -> wire::TenantId {
    wire::TenantId {
        value: id.to_string(),
    }
}

/// Branches have no server-side id distinct from their name, so
/// `BranchId.value` carries the branch name.
pub(crate) fn branch_id(name: &str) -> wire::BranchId {
    wire::BranchId {
        value: name.to_string(),
    }
}

/// `Some(value)` for a present, non-empty snapshot id.
pub(crate) fn opt_snapshot_id(id: Option<&wire::SnapshotId>) -> Option<String> {
    id.map(|s| s.value.clone()).filter(|v| !v.is_empty())
}

// Tag records build their timestamp in `signing::tag_record` (§15.8).
#[cfg(test)]
pub(crate) fn timestamp(at: DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: at.timestamp(),
        nanos: at.timestamp_subsec_nanos() as i32,
    }
}

/// Missing or out-of-range timestamps decode as the Unix epoch.
pub(crate) fn datetime(ts: Option<&prost_types::Timestamp>) -> DateTime<Utc> {
    ts.and_then(|t| DateTime::from_timestamp(t.seconds, u32::try_from(t.nanos).unwrap_or(0)))
        .unwrap_or(DateTime::UNIX_EPOCH)
}

/// The wire record of a tag, signature included (§15.8). The signed
/// payload is this record with the signature fields cleared
/// (`signing::tag_signed_payload`).
pub(crate) fn tag_to_record(tag: &TagInfo) -> wire::TagRecord {
    let mut rec = crate::signing::tag_record(tag);
    if let Some(sig) = &tag.signature {
        rec.signature = sig.signature.clone();
        rec.signing_key_id = sig.key_id.clone();
    }
    rec
}

pub(crate) fn tag_from_record(rec: &wire::TagRecord) -> Result<TagInfo> {
    let target = opt_snapshot_id(rec.target.as_ref())
        .ok_or_else(|| SdkError::Remote(format!("server tag '{}' has no target", rec.name)))?;
    Ok(TagInfo {
        name: rec.name.clone(),
        target,
        message: rec.message.clone(),
        tagger: rec.tagger.clone(),
        created_at: datetime(rec.created_at.as_ref()),
        signature: (!rec.signature.is_empty() || !rec.signing_key_id.is_empty()).then(|| {
            crate::model::SnapshotSignature {
                key_id: rec.signing_key_id.clone(),
                signature: rec.signature.clone(),
            }
        }),
    })
}

pub(crate) fn staged_from_wire(rec: &wire::StagedInfo) -> StagedInfo {
    StagedInfo {
        snapshot_id: rec
            .snapshot_id
            .as_ref()
            .map(|s| s.value.clone())
            .unwrap_or_default(),
        author: rec.author.clone(),
        branch: rec.branch_name.clone(),
        files_changed: rec.files_changed.clone(),
        files_added: rec.files_added,
        files_modified: rec.files_modified,
        files_deleted: rec.files_deleted,
        message: rec.message.clone().filter(|m| !m.is_empty()),
        staged_at: datetime(rec.staged_at.as_ref()),
        // staged-v2 (§16.8)
        label: rec.label.clone(),
        manifest_hash: hash_from_wire(rec.manifest_hash.as_ref())
            .ok()
            .map(|h| h.to_hex()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_round_trip_and_validation() {
        let h = crate::store::hash_bytes(b"hello");
        assert_eq!(hash_from_wire(Some(&wire_hash(&h))).unwrap(), h);
        assert!(matches!(hash_from_wire(None), Err(SdkError::Remote(_))));
        let short = wire::ContentHash {
            value: vec![1, 2, 3],
        };
        assert!(
            matches!(hash_from_wire(Some(&short)), Err(SdkError::Remote(m)) if m.contains("3-byte"))
        );
    }

    #[test]
    fn timestamps_round_trip_and_default_to_epoch() {
        let at = DateTime::from_timestamp(1_700_000_000, 123_456_789).unwrap();
        assert_eq!(datetime(Some(&timestamp(at))), at);
        assert_eq!(datetime(None), DateTime::UNIX_EPOCH);
        let bad = prost_types::Timestamp {
            seconds: 0,
            nanos: -5,
        };
        assert_eq!(datetime(Some(&bad)), DateTime::UNIX_EPOCH);
    }

    #[test]
    fn tag_round_trip() {
        let tag = TagInfo {
            name: "v1.0".into(),
            target: "0b9f0b4e-7d41-4a51-a8a6-1e4b8c7b6f10".into(),
            message: Some("first release".into()),
            tagger: "alice".into(),
            created_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            signature: None,
        };
        assert_eq!(tag_from_record(&tag_to_record(&tag)).unwrap(), tag);
        let mut rec = tag_to_record(&tag);
        rec.target = None;
        assert!(tag_from_record(&rec).is_err());

        // Signed tags carry their signature on the wire.
        let mut signed = tag.clone();
        signed.signature = Some(crate::model::SnapshotSignature {
            key_id: "key-1".into(),
            signature: vec![7; 64],
        });
        let rec = tag_to_record(&signed);
        assert_eq!(rec.signing_key_id, "key-1");
        assert_eq!(rec.signature.len(), 64);
        assert_eq!(tag_from_record(&rec).unwrap(), signed);
    }

    #[test]
    fn empty_snapshot_ids_are_none() {
        assert_eq!(opt_snapshot_id(None), None);
        assert_eq!(opt_snapshot_id(Some(&snapshot_id(""))), None);
        assert_eq!(
            opt_snapshot_id(Some(&snapshot_id("abc"))),
            Some("abc".into())
        );
    }

    #[test]
    fn staged_conversion() {
        let rec = wire::StagedInfo {
            snapshot_id: Some(snapshot_id("s1")),
            author: "bob".into(),
            branch_name: "main".into(),
            files_changed: vec!["a.txt".into()],
            files_added: 1,
            files_modified: 0,
            files_deleted: 0,
            message: Some(String::new()),
            staged_at: Some(prost_types::Timestamp {
                seconds: 10,
                nanos: 0,
            }),
            manifest_hash: None,
            ..Default::default()
        };
        let info = staged_from_wire(&rec);
        assert_eq!(info.snapshot_id, "s1");
        assert_eq!(info.branch, "main");
        assert_eq!(info.message, None);
        assert_eq!(info.staged_at.timestamp(), 10);
        assert_eq!(
            (info.label.as_str(), info.manifest_hash.as_deref()),
            ("", None)
        );
        let labeled = staged_from_wire(&wire::StagedInfo {
            label: "exp".into(),
            manifest_hash: Some(wire_hash(&crate::store::hash_bytes(b"m"))),
            ..rec
        });
        assert_eq!(labeled.label, "exp");
        assert_eq!(
            labeled.manifest_hash,
            Some(crate::store::hash_bytes(b"m").to_hex())
        );
        assert_eq!(branch_id("dev").value, "dev");
        assert_eq!(tree_id("t").value, "t");
        assert_eq!(tenant_id("x").value, "x");
    }
}
