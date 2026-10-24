#[cfg(test)]
mod tests {
    use worktree_protocol::core::hash::*;

    #[test]
    fn test_content_hash_roundtrip() {
        let data = b"hello worktree";
        let hash = hash_bytes(data);
        let hex = hash.to_string();
        assert!(!hex.is_empty());
        assert_eq!(hex.len(), 64); // BLAKE3 produces 32 bytes = 64 hex chars
    }
}
