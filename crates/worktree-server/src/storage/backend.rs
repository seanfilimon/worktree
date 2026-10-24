use worktree_protocol::core::hash::ContentHash;

use crate::error::ServerError;

/// Trait defining the interface for content-addressable object storage.
///
/// Implementations of `StorageBackend` are responsible for persisting and
/// retrieving opaque byte blobs keyed by their [`ContentHash`]. The server
/// uses this abstraction so that different backends (disk, in-memory, remote
/// object stores, etc.) can be swapped transparently.
pub trait StorageBackend: Send + Sync {
    /// Store `data` under the given content-addressable `hash`.
    ///
    /// If an object with the same hash already exists, the implementation
    /// may skip the write (content-addressable deduplication) or overwrite
    /// with identical data — either behaviour is correct.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Storage`] if the write fails (e.g. disk full,
    /// permission denied, network error).
    fn store(&self, hash: &ContentHash, data: &[u8]) -> Result<(), ServerError>;

    /// Retrieve the data previously stored under `hash`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Storage`] if the object does not exist or if
    /// the read fails.
    fn retrieve(&self, hash: &ContentHash) -> Result<Vec<u8>, ServerError>;

    /// Check whether an object with the given `hash` exists in the store.
    ///
    /// This should be a fast, non-blocking check when possible (e.g. a
    /// filesystem `exists()` call rather than reading the full object).
    fn exists(&self, hash: &ContentHash) -> bool;

    /// Delete the object stored under `hash`, if it exists.
    ///
    /// Returns `Ok(())` even if the object was not present (idempotent delete).
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Storage`] if the deletion fails for a reason
    /// other than the object not existing.
    fn delete(&self, hash: &ContentHash) -> Result<(), ServerError> {
        // Default implementation — backends can override for efficiency.
        let _ = hash;
        Ok(())
    }

    /// Return the total number of objects currently stored, if the backend
    /// supports cheap enumeration.
    ///
    /// Backends that cannot efficiently count objects may return `None`.
    fn object_count(&self) -> Option<usize> {
        None
    }
}
