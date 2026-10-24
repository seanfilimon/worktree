//! Wire-level encoding.
//!
//! Provides functions to serialize a value into the Worktree binary wire format,
//! prepending the [`WireFormat`] header so that the receiver can decode the
//! message without any out-of-band length information.

use serde::Serialize;

use crate::wire::format::{WireFlags, WireFormat, WireError};

/// Encode a serializable value into a wire-format message.
///
/// The returned `Vec<u8>` contains:
/// 1. A 13-byte [`WireFormat`] header (magic, version, payload length, flags).
/// 2. The bincode-serialized payload bytes.
///
/// # Errors
///
/// Returns [`WireError::EncodeError`] if bincode serialization fails.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, WireError> {
    encode_with_flags(value, WireFlags::NONE)
}

/// Encode a serializable value into a wire-format message with the given flags.
///
/// The returned `Vec<u8>` contains:
/// 1. A 13-byte [`WireFormat`] header with the specified flags.
/// 2. The bincode-serialized payload bytes.
///
/// # Errors
///
/// Returns [`WireError::EncodeError`] if bincode serialization fails.
pub fn encode_with_flags<T: Serialize>(value: &T, flags: WireFlags) -> Result<Vec<u8>, WireError> {
    let payload = bincode::serialize(value).map_err(|e| WireError::EncodeError(e.to_string()))?;

    let header = WireFormat::with_flags(payload.len() as u32, flags);
    let header_bytes = header.encode_header();

    let mut message = Vec::with_capacity(header_bytes.len() + payload.len());
    message.extend_from_slice(&header_bytes);
    message.extend_from_slice(&payload);

    Ok(message)
}

/// Encode a serializable value with an additional 4-byte little-endian length
/// prefix *before* the wire-format header.
///
/// The full layout is:
/// ```text
/// ┌──────────────────┬───────────────────────┬─────────┐
/// │ total_len (4 LE) │ wire header (13 bytes) │ payload │
/// └──────────────────┴───────────────────────┴─────────┘
/// ```
///
/// `total_len` is the number of bytes that follow the 4-byte prefix
/// (i.e. header + payload, **not** including the prefix itself).
///
/// This is useful for framing messages over a stream transport (TCP, pipes)
/// where the reader needs to know how many bytes to read before it can
/// parse the header.
///
/// # Errors
///
/// Returns [`WireError::EncodeError`] if bincode serialization fails, or
/// [`WireError::PayloadTooLarge`] if the total message (header + payload)
/// exceeds `u32::MAX` bytes.
pub fn encode_length_prefixed<T: Serialize>(value: &T) -> Result<Vec<u8>, WireError> {
    encode_length_prefixed_with_flags(value, WireFlags::NONE)
}

/// Encode a serializable value with a 4-byte length prefix and the given flags.
///
/// See [`encode_length_prefixed`] for the wire layout.
///
/// # Errors
///
/// Returns [`WireError::EncodeError`] if bincode serialization fails, or
/// [`WireError::PayloadTooLarge`] if the total message exceeds `u32::MAX`.
pub fn encode_length_prefixed_with_flags<T: Serialize>(
    value: &T,
    flags: WireFlags,
) -> Result<Vec<u8>, WireError> {
    let inner = encode_with_flags(value, flags)?;
    let inner_len = inner.len();

    if inner_len > u32::MAX as usize {
        return Err(WireError::PayloadTooLarge {
            size: inner_len as u64,
            max: u32::MAX as u64,
        });
    }

    let len_prefix = (inner_len as u32).to_le_bytes();

    let mut framed = Vec::with_capacity(4 + inner_len);
    framed.extend_from_slice(&len_prefix);
    framed.extend_from_slice(&inner);

    Ok(framed)
}

/// Encode raw bytes (already serialized) into a wire-format message.
///
/// This is useful when you have pre-serialized payload bytes and just want
/// to wrap them with the wire header.
pub fn encode_raw(payload: &[u8]) -> Result<Vec<u8>, WireError> {
    encode_raw_with_flags(payload, WireFlags::NONE)
}

/// Encode raw bytes with the given flags.
pub fn encode_raw_with_flags(payload: &[u8], flags: WireFlags) -> Result<Vec<u8>, WireError> {
    if payload.len() > u32::MAX as usize {
        return Err(WireError::PayloadTooLarge {
            size: payload.len() as u64,
            max: u32::MAX as u64,
        });
    }

    let header = WireFormat::with_flags(payload.len() as u32, flags);
    let header_bytes = header.encode_header();

    let mut message = Vec::with_capacity(header_bytes.len() + payload.len());
    message.extend_from_slice(&header_bytes);
    message.extend_from_slice(payload);

    Ok(message)
}

/// Encode raw bytes into a length-prefixed wire-format message.
pub fn encode_raw_length_prefixed(payload: &[u8]) -> Result<Vec<u8>, WireError> {
    let inner = encode_raw(payload)?;
    let inner_len = inner.len();

    if inner_len > u32::MAX as usize {
        return Err(WireError::PayloadTooLarge {
            size: inner_len as u64,
            max: u32::MAX as u64,
        });
    }

    let len_prefix = (inner_len as u32).to_le_bytes();

    let mut framed = Vec::with_capacity(4 + inner_len);
    framed.extend_from_slice(&len_prefix);
    framed.extend_from_slice(&inner);

    Ok(framed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::format::{HEADER_SIZE, MAGIC, CURRENT_VERSION};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestMessage {
        name: String,
        value: u64,
    }

    fn sample_message() -> TestMessage {
        TestMessage {
            name: "hello".to_string(),
            value: 42,
        }
    }

    // ── encode() ────────────────────────────────────────────────────────

    #[test]
    fn test_encode_produces_valid_header() {
        let msg = sample_message();
        let encoded = encode(&msg).expect("encode");

        assert!(encoded.len() >= HEADER_SIZE);
        // Magic bytes
        assert_eq!(&encoded[0..4], &MAGIC);
        // Version
        let version = u32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]);
        assert_eq!(version, CURRENT_VERSION);
        // Flags should be NONE
        assert_eq!(encoded[12], 0);
    }

    #[test]
    fn test_encode_payload_length_matches() {
        let msg = sample_message();
        let encoded = encode(&msg).expect("encode");

        let payload_len =
            u32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]) as usize;
        assert_eq!(encoded.len(), HEADER_SIZE + payload_len);
    }

    #[test]
    fn test_encode_payload_is_valid_bincode() {
        let msg = sample_message();
        let encoded = encode(&msg).expect("encode");

        let payload = &encoded[HEADER_SIZE..];
        let decoded: TestMessage = bincode::deserialize(payload).expect("deserialize payload");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_encode_with_flags() {
        let msg = sample_message();
        let flags = WireFlags::NONE.with_compressed().with_checksummed();
        let encoded = encode_with_flags(&msg, flags).expect("encode");

        assert_eq!(encoded[12], flags.as_byte());
    }

    #[test]
    fn test_encode_empty_struct() {
        #[derive(Serialize)]
        struct Empty;

        let encoded = encode(&Empty).expect("encode");
        assert!(encoded.len() >= HEADER_SIZE);
        let payload_len =
            u32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]) as usize;
        assert_eq!(encoded.len(), HEADER_SIZE + payload_len);
    }

    #[test]
    fn test_encode_large_payload() {
        let big = vec![42u8; 100_000];
        let encoded = encode(&big).expect("encode");

        let payload_len =
            u32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]) as usize;
        assert_eq!(encoded.len(), HEADER_SIZE + payload_len);

        let decoded: Vec<u8> = bincode::deserialize(&encoded[HEADER_SIZE..]).expect("deserialize");
        assert_eq!(decoded, big);
    }

    // ── encode_length_prefixed() ────────────────────────────────────────

    #[test]
    fn test_encode_length_prefixed_has_prefix() {
        let msg = sample_message();
        let framed = encode_length_prefixed(&msg).expect("encode");

        assert!(framed.len() >= 4 + HEADER_SIZE);

        // The first 4 bytes are the LE u32 length of the rest.
        let prefix_len =
            u32::from_le_bytes([framed[0], framed[1], framed[2], framed[3]]) as usize;
        assert_eq!(prefix_len, framed.len() - 4);
    }

    #[test]
    fn test_encode_length_prefixed_inner_is_valid() {
        let msg = sample_message();
        let framed = encode_length_prefixed(&msg).expect("encode");

        // Skip the 4-byte prefix; the rest should be a valid wire message.
        let inner = &framed[4..];
        assert_eq!(&inner[0..4], &MAGIC);

        let payload_len =
            u32::from_le_bytes([inner[8], inner[9], inner[10], inner[11]]) as usize;
        let payload = &inner[HEADER_SIZE..HEADER_SIZE + payload_len];
        let decoded: TestMessage = bincode::deserialize(payload).expect("deserialize");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_encode_length_prefixed_with_flags() {
        let msg = sample_message();
        let flags = WireFlags::NONE.with_encrypted();
        let framed = encode_length_prefixed_with_flags(&msg, flags).expect("encode");

        // Check flags in the inner message
        let inner = &framed[4..];
        assert_eq!(inner[12], flags.as_byte());
    }

    // ── encode_raw() ────────────────────────────────────────────────────

    #[test]
    fn test_encode_raw_wraps_payload() {
        let payload = b"raw payload bytes";
        let encoded = encode_raw(payload).expect("encode_raw");

        assert_eq!(&encoded[0..4], &MAGIC);

        let payload_len =
            u32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]) as usize;
        assert_eq!(payload_len, payload.len());
        assert_eq!(&encoded[HEADER_SIZE..], payload);
    }

    #[test]
    fn test_encode_raw_with_flags() {
        let payload = b"flagged";
        let flags = WireFlags::NONE.with_compressed();
        let encoded = encode_raw_with_flags(payload, flags).expect("encode_raw_with_flags");

        assert_eq!(encoded[12], flags.as_byte());
        assert_eq!(&encoded[HEADER_SIZE..], &payload[..]);
    }

    #[test]
    fn test_encode_raw_empty_payload() {
        let encoded = encode_raw(b"").expect("encode_raw");

        let payload_len =
            u32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]);
        assert_eq!(payload_len, 0);
        assert_eq!(encoded.len(), HEADER_SIZE);
    }

    // ── encode_raw_length_prefixed() ────────────────────────────────────

    #[test]
    fn test_encode_raw_length_prefixed() {
        let payload = b"stream framed";
        let framed = encode_raw_length_prefixed(payload).expect("encode");

        let prefix_len =
            u32::from_le_bytes([framed[0], framed[1], framed[2], framed[3]]) as usize;
        assert_eq!(prefix_len, framed.len() - 4);

        let inner = &framed[4..];
        assert_eq!(&inner[0..4], &MAGIC);
        assert_eq!(&inner[HEADER_SIZE..], &payload[..]);
    }

    // ── Roundtrip with decode ───────────────────────────────────────────

    #[test]
    fn test_encode_decode_header_roundtrip() {
        let msg = sample_message();
        let encoded = encode(&msg).expect("encode");

        let header = WireFormat::decode_header(&encoded).expect("decode header");
        assert_eq!(header.version, CURRENT_VERSION);
        assert_eq!(header.flags, WireFlags::NONE);

        let payload = header.payload(&encoded).expect("extract payload");
        let decoded: TestMessage = bincode::deserialize(payload).expect("deserialize");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_encode_various_types() {
        // String
        let s = "hello world".to_string();
        let encoded = encode(&s).expect("encode string");
        let payload = &encoded[HEADER_SIZE..];
        let decoded: String = bincode::deserialize(payload).expect("deserialize string");
        assert_eq!(decoded, s);

        // Vec<u32>
        let v: Vec<u32> = vec![1, 2, 3, 4, 5];
        let encoded = encode(&v).expect("encode vec");
        let payload = &encoded[HEADER_SIZE..];
        let decoded: Vec<u32> = bincode::deserialize(payload).expect("deserialize vec");
        assert_eq!(decoded, v);

        // Tuple
        let t = (42u64, true, "data".to_string());
        let encoded = encode(&t).expect("encode tuple");
        let payload = &encoded[HEADER_SIZE..];
        let decoded: (u64, bool, String) =
            bincode::deserialize(payload).expect("deserialize tuple");
        assert_eq!(decoded, t);
    }

    #[test]
    fn test_encode_deterministic() {
        let msg = sample_message();
        let a = encode(&msg).expect("encode a");
        let b = encode(&msg).expect("encode b");
        assert_eq!(a, b);
    }

    #[test]
    fn test_encode_different_messages_differ() {
        let a = encode(&TestMessage {
            name: "alpha".into(),
            value: 1,
        })
        .expect("encode a");
        let b = encode(&TestMessage {
            name: "beta".into(),
            value: 2,
        })
        .expect("encode b");
        assert_ne!(a, b);
    }

    #[test]
    fn test_total_size_consistency() {
        let msg = sample_message();

        // encode
        let encoded = encode(&msg).expect("encode");
        let header = WireFormat::decode_header(&encoded).expect("decode");
        assert_eq!(header.total_size(), encoded.len());

        // encode_length_prefixed
        let framed = encode_length_prefixed(&msg).expect("encode lp");
        let inner = &framed[4..];
        let header = WireFormat::decode_header(inner).expect("decode");
        assert_eq!(header.total_size(), inner.len());
    }

    #[test]
    fn test_flags_preserved_through_encode_decode() {
        let msg = sample_message();
        let flags = WireFlags::NONE
            .with_compressed()
            .with_checksummed()
            .with_encrypted();

        let encoded = encode_with_flags(&msg, flags).expect("encode");
        let header = WireFormat::decode_header(&encoded).expect("decode");

        assert!(header.flags.is_compressed());
        assert!(header.flags.is_checksummed());
        assert!(header.flags.is_encrypted());
        assert_eq!(header.flags, flags);
    }
}
