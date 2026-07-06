//! Content-addressable object store.
//!
//! Layout per Storage.md:
//!
//! ```text
//! <store>/objects/{blobs,trees,snapshots,manifests,chunks}/<2-hex>/<hash>
//! ```

mod object_store;

pub use object_store::ObjectStore;

/// Kinds of objects the CAS stores, each in its own fan-out namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Blob,
    Tree,
    Snapshot,
    Manifest,
    Chunk,
}

impl ObjectKind {
    /// All kinds, for iteration (stats, verification).
    pub const ALL: [ObjectKind; 5] = [
        ObjectKind::Blob,
        ObjectKind::Tree,
        ObjectKind::Snapshot,
        ObjectKind::Manifest,
        ObjectKind::Chunk,
    ];

    /// Directory name under `objects/` for this kind.
    pub fn dir_name(&self) -> &'static str {
        match self {
            ObjectKind::Blob => "blobs",
            ObjectKind::Tree => "trees",
            ObjectKind::Snapshot => "snapshots",
            ObjectKind::Manifest => "manifests",
            ObjectKind::Chunk => "chunks",
        }
    }

    /// Frame type byte for the on-disk object frame (Storage.md).
    pub fn type_byte(&self) -> u8 {
        match self {
            ObjectKind::Blob => 0x01,
            ObjectKind::Tree => 0x02,
            ObjectKind::Snapshot => 0x03,
            ObjectKind::Manifest => 0x04,
            ObjectKind::Chunk => 0x05,
        }
    }

    /// Inverse of [`ObjectKind::type_byte`].
    pub fn from_type_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0x01 => ObjectKind::Blob,
            0x02 => ObjectKind::Tree,
            0x03 => ObjectKind::Snapshot,
            0x04 => ObjectKind::Manifest,
            0x05 => ObjectKind::Chunk,
            _ => return None,
        })
    }
}
