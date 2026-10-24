//! Wire format decoding.
//!
//! Provides [`decode()`] for deserializing a typed value from a wire-format
//! message (header + payload), and [`decode_payload()`] for deserializing
//! from raw payload bytes without a header.

use serde::de::DeserializeOwned;

use crate::wire::format::{WireError, WireFormat, HEADER_SIZE};

/// Decode a typed value from a complete wire-format message (header + payload).
///
/// This function:
/// 1. Parses the [`WireFormat`] header from the first [`HEADER_SIZE`] bytes.
/// 2. Validates that enough payload bytes are present.
/// 3. Deserializes the payload using `bincode`.
///
/// Returns the decoded value and the parsed header.
///
/// # Errors
///
/// Returns a [`WireError`] if the header is invalid, the payload is truncated,
/// or deserialization fails.
pub fn decode<T: DeserializeOwned>(data: &[u8]) -> Result<(WireFormat, T), WireError> {
    let header = WireFormat::decode_header(data)?;
    let payload = header.payload(data)?;
    let value: T =
        bincode::deserialize(payload).map_err(|e| WireError::DecodeError(e.to_string()))?;
    Ok((header, value))
}

/// Decode a typed value from raw payload bytes (no header).
///
/// This is useful when the header has already been parsed and you just need
/// to deserialize the payload content.
///
/// # Errors
///
/// Returns a [`WireError::DecodeError`] if deserialization fails.
pub fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T, WireError> {
    bincode::deserialize(payload).map_err(|e| WireError::DecodeError(e.to_string()))
}

/// Decode a typed value from a length-prefixed wire message.
///
/// The format is:
/// 1. 4 bytes (little-endian `u32`) — total message length (header + payload).
/// 2. The full wire message (header + payload).
///
/// This is the inverse of [`crate::wire::encode::encode_length_prefixed()`].
///
/// # Errors
///
/// Returns a [`WireError`] if the length prefix is missing, the message is
/// truncated, or deserialization fails.
pub fn decode_length_prefixed<T: DeserializeOwned>(data: &[u8]) -> Result<(WireFormat, T), WireError> {
    if data.len() < 4 {
        return Err(WireError::TruncatedHeader(data.len()));
    }

    let msg_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

    if data.len() < 4 + msg_len {
        return Err(WireError::TruncatedPayload {
            expected: msg_len as u32,
            actual: data.len() - 4,
        });
    }

    decode(&data[4..4 + msg_len])
}

/// Split a byte buffer into individual length-prefixed messages.
///
/// Returns a vector of byte slices, each containing a complete wire message
/// (without the length prefix). This is useful for processing a stream of
/// concatenated length-prefixed messages.
///
/// # Errors
///
/// Returns a [`WireError`] if any message is truncated.
pub fn split_messages(data: &[u8]) -> Result<Vec<&[u8]>, WireError> {
    let mut messages = Vec::new();
    let mut offset = 0;

    while offset < data.len() {
        if offset + 4 > data.len() {
            return Err(WireError::TruncatedHeader(data.len() - offset));
        }

        let msg_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;

        let start = offset + 4;
        let end = start + msg_len;

        if end > data.len() {
            return Err(WireError::TruncatedPayload {
                expected: msg_len as u32,
                actual: data.len() - start,
            });
        }

        messages.push(&data[start..end]);
        offset = end;
    }

    Ok(messages)
}

/// Peek at the wire format header without decoding the payload.
///
/// This is useful for inspecting the version, payload length, and flags
/// before committing to a full decode.
///
/// # Errors
///
/// Returns a [`WireError`] if the data is too short or the header is invalid.
pub fn peek_header(data: &[u8]) -> Result<WireFormat, WireError> {
    WireFormat::decode_header(data)
}

/// Peek at the header from a length-prefixed message.
///
/// # Errors
///
/// Returns a [`WireError`] if the data is too short or the header is invalid.
pub fn peek_header_length_prefixed(data: &[u8]) -> Result<WireFormat, WireError> {
    if data.len() < 4 {
        return Err(WireError::TruncatedHeader(data.len()));
    }

    let msg_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if data.len() < 4 + HEADER_SIZE.min(msg_len) {
        return Err(WireError::TruncatedHeader(data.len() - 4));
    }

    WireFormat::decode_header(&data[4..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::encode::{encode, encode_length_prefixed};
    use crate::wire::format::{WireFlags, WireFormat, CURRENT_VERSION, HEADER_SIZE, MAGIC};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestMessage {
        id: u64,
        name: String,
        data: Vec<u8>,
    }

    fn sample_message() -> TestMessage {
        TestMessage {
            id: 42,
            name: "hello world".to_string(),
            data: vec![1, 2, 3, 4, 5],
        }
    }

    #[test]
    fn test_decode_roundtrip() {
        let msg = sample_message();
        let encoded = encode(&msg).expect("encode");
        let (header, decoded): (_, TestMessage) = decode(&encoded).expect("decode");

        assert_eq!(header.version, CURRENT_VERSION);
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_decode_truncated_header() {
        let result = decode::<TestMessage>(&[0u8; 5]);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::TruncatedHeader(len) => assert_eq!(len, 5),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_decode_invalid_magic() {
        let mut data = WireFormat::new(0).encode_header().to_vec();
        data[0] = 0xFF;
        let result = decode::<TestMessage>(&data);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::InvalidMagic(_) => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_decode_truncated_payload() {
        let msg = sample_message();
        let encoded = encode(&msg).expect("encode");
        // Truncate the payload
        let truncated = &encoded[..HEADER_SIZE + 2];
        let result = decode::<TestMessage>(truncated);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::TruncatedPayload { .. } => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_decode_corrupt_payload() {
        let wf = WireFormat::new(10);
        let header = wf.encode_header();
        let mut data = Vec::new();
        data.extend_from_slice(&header);
        data.extend_from_slice(&[0xFF; 10]); // garbage payload

        let result = decode::<TestMessage>(&data);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::DecodeError(_) => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_decode_payload_roundtrip() {
        let msg = sample_message();
        let payload = bincode::serialize(&msg).expect("serialize");
        let decoded: TestMessage = decode_payload(&payload).expect("decode_payload");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_decode_payload_corrupt() {
        let result = decode_payload::<TestMessage>(&[0xFF, 0x00, 0x01]);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::DecodeError(_) => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_decode_length_prefixed_roundtrip() {
        let msg = sample_message();
        let encoded = encode_length_prefixed(&msg).expect("encode");
        let (header, decoded): (_, TestMessage) =
            decode_length_prefixed(&encoded).expect("decode");

        assert_eq!(header.version, CURRENT_VERSION);
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_decode_length_prefixed_too_short_for_prefix() {
        let result = decode_length_prefixed::<TestMessage>(&[0u8; 2]);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::TruncatedHeader(len) => assert_eq!(len, 2),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_decode_length_prefixed_truncated_message() {
        let msg = sample_message();
        let encoded = encode_length_prefixed(&msg).expect("encode");
        // Truncate after the length prefix + a few bytes
        let truncated = &encoded[..8];
        let result = decode_length_prefixed::<TestMessage>(truncated);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::TruncatedPayload { .. } => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_split_messages_single() {
        let msg = sample_message();
        let encoded = encode_length_prefixed(&msg).expect("encode");
        let messages = split_messages(&encoded).expect("split");
        assert_eq!(messages.len(), 1);

        let (_, decoded): (_, TestMessage) = decode(messages[0]).expect("decode");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_split_messages_multiple() {
        let msg1 = TestMessage {
            id: 1,
            name: "first".to_string(),
            data: vec![1],
        };
        let msg2 = TestMessage {
            id: 2,
            name: "second".to_string(),
            data: vec![2, 3],
        };
        let msg3 = TestMessage {
            id: 3,
            name: "third".to_string(),
            data: vec![4, 5, 6],
        };

        let mut buf = Vec::new();
        buf.extend_from_slice(&encode_length_prefixed(&msg1).expect("encode1"));
        buf.extend_from_slice(&encode_length_prefixed(&msg2).expect("encode2"));
        buf.extend_from_slice(&encode_length_prefixed(&msg3).expect("encode3"));

        let messages = split_messages(&buf).expect("split");
        assert_eq!(messages.len(), 3);

        let (_, d1): (_, TestMessage) = decode(messages[0]).expect("decode1");
        let (_, d2): (_, TestMessage) = decode(messages[1]).expect("decode2");
        let (_, d3): (_, TestMessage) = decode(messages[2]).expect("decode3");

        assert_eq!(d1, msg1);
        assert_eq!(d2, msg2);
        assert_eq!(d3, msg3);
    }

    #[test]
    fn test_split_messages_empty() {
        let messages = split_messages(&[]).expect("split");
        assert!(messages.is_empty());
    }

    #[test]
    fn test_split_messages_truncated_prefix() {
        let result = split_messages(&[0u8; 3]);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::TruncatedHeader(_) => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_split_messages_truncated_message() {
        // Length prefix says 100 bytes but only 5 bytes follow
        let mut data = Vec::new();
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 5]);

        let result = split_messages(&data);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::TruncatedPayload { expected, actual } => {
                assert_eq!(expected, 100);
                assert_eq!(actual, 5);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_peek_header() {
        let msg = sample_message();
        let encoded = encode(&msg).expect("encode");
        let header = peek_header(&encoded).expect("peek");

        assert_eq!(header.version, CURRENT_VERSION);
        assert!(header.flags == WireFlags::NONE);
    }

    #[test]
    fn test_peek_header_truncated() {
        let result = peek_header(&[0u8; 5]);
        assert!(result.is_err());
    }

    #[test]
    fn test_peek_header_length_prefixed() {
        let msg = sample_message();
        let encoded = encode_length_prefixed(&msg).expect("encode");
        let header = peek_header_length_prefixed(&encoded).expect("peek");

        assert_eq!(header.version, CURRENT_VERSION);
    }

    #[test]
    fn test_peek_header_length_prefixed_too_short() {
        let result = peek_header_length_prefixed(&[0u8; 2]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_with_flags() {
        // Manually construct a message with flags set
        let msg = sample_message();
        let payload = bincode::serialize(&msg).expect("serialize");
        let flags = WireFlags::NONE.with_checksummed();
        let wf = WireFormat::with_flags(payload.len() as u32, flags);
        let header = wf.encode_header();

        let mut data = Vec::new();
        data.extend_from_slice(&header);
        data.extend_from_slice(&payload);

        let (decoded_header, decoded_msg): (_, TestMessage) = decode(&data).expect("decode");
        assert_eq!(decoded_header.flags, flags);
        assert!(decoded_header.flags.is_checksummed());
        assert_eq!(decoded_msg, msg);
    }

    #[test]
    fn test_decode_empty_payload() {
        let wf = WireFormat::new(0);
        let header = wf.encode_header();
        // Try to decode an empty payload into a unit type
        let result = decode::<()>(&header);
        // bincode for () is 0 bytes, so this should succeed
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_simple_types() {
        // Encode and decode a simple u64
        let value: u64 = 0xDEAD_BEEF_CAFE_BABE;
        let encoded = encode(&value).expect("encode");
        let (_, decoded): (_, u64) = decode(&encoded).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_decode_string() {
        let value = "Hello, Worktree Protocol!".to_string();
        let encoded = encode(&value).expect("encode");
        let (_, decoded): (_, String) = decode(&encoded).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_decode_vec() {
        let value: Vec<u32> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let encoded = encode(&value).expect("encode");
        let (_, decoded): (_, Vec<u32>) = decode(&encoded).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_decode_nested_struct() {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        struct Inner {
            x: i32,
            y: i32,
        }

        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        struct Outer {
            label: String,
            inner: Inner,
            values: Vec<Inner>,
        }

        let value = Outer {
            label: "test".to_string(),
            inner: Inner { x: 10, y: 20 },
            values: vec![Inner { x: 1, y: 2 }, Inner { x: 3, y: 4 }],
        };

        let encoded = encode(&value).expect("encode");
        let (_, decoded): (_, Outer) = decode(&encoded).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_decode_length_prefixed_preserves_header_info() {
        let msg = sample_message();
        let encoded = encode_length_prefixed(&msg).expect("encode");

        // Verify the length prefix
        let msg_len = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
        assert_eq!(msg_len as usize, encoded.len() - 4);

        // Verify the magic bytes are after the length prefix
        assert_eq!(&encoded[4..8], &MAGIC);

        let (header, decoded): (_, TestMessage) =
            decode_length_prefixed(&encoded).expect("decode");
        assert_eq!(header.version, CURRENT_VERSION);
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_decode_extra_data_after_message_is_ignored() {
        let msg = sample_message();
        let mut encoded = encode(&msg).expect("encode");
        // Append garbage
        encoded.extend_from_slice(b"this is garbage data after the message");

        let (_, decoded): (_, TestMessage) = decode(&encoded).expect("decode");
        assert_eq!(decoded, msg);
    }
}
