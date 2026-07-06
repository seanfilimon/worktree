//! Object frame encode/decode.
//!
//! On-disk frame per Storage.md §Object Serialization:
//!
//! ```text
//! [type: u8][uncompressed size: varint][zstd body][BLAKE3 of payload: 32B]
//! ```
//!
//! The trailer hashes the **uncompressed payload**; it doubles as the
//! object's content address, so decoding verifies both frame integrity and
//! (when the caller passes the expected hash) addressing.

use crate::cas::ObjectKind;
use crate::error::{Result, StoreError};

/// zstd compression level (Storage.md: "default compression: zstd level 3").
const ZSTD_LEVEL: i32 = 3;

/// Encode a payload into an object frame.
pub fn encode(kind: ObjectKind, payload: &[u8]) -> Result<Vec<u8>> {
    let compressed = zstd::encode_all(payload, ZSTD_LEVEL)
        .map_err(|e| StoreError::Corrupt(format!("zstd encode failed: {e}")))?;

    let mut frame = Vec::with_capacity(compressed.len() + 40);
    frame.push(kind.type_byte());
    write_varint(&mut frame, payload.len() as u64);
    frame.extend_from_slice(&compressed);
    frame.extend_from_slice(blake3::hash(payload).as_bytes());
    Ok(frame)
}

/// Decode an object frame, verifying size and hash trailer.
///
/// When `expected_hash` is given, additionally verifies the payload hashes
/// to that address (catches objects stored under the wrong name).
pub fn decode(frame: &[u8], expected_hash: Option<&[u8; 32]>) -> Result<(ObjectKind, Vec<u8>)> {
    if frame.len() < 1 + 1 + 32 {
        return Err(StoreError::Corrupt("frame shorter than header".into()));
    }
    let kind = ObjectKind::from_type_byte(frame[0]).ok_or_else(|| {
        StoreError::Corrupt(format!("unknown object type byte {:#04x}", frame[0]))
    })?;

    let (size, varint_len) = read_varint(&frame[1..])
        .ok_or_else(|| StoreError::Corrupt("invalid size varint".into()))?;

    let body_start = 1 + varint_len;
    let trailer_start = frame
        .len()
        .checked_sub(32)
        .filter(|&t| t >= body_start)
        .ok_or_else(|| StoreError::Corrupt("frame missing hash trailer".into()))?;

    let payload = zstd::decode_all(&frame[body_start..trailer_start])
        .map_err(|e| StoreError::Corrupt(format!("zstd decode failed: {e}")))?;

    if payload.len() as u64 != size {
        return Err(StoreError::Corrupt(format!(
            "size mismatch: header says {size}, payload is {}",
            payload.len()
        )));
    }

    let actual = blake3::hash(&payload);
    if actual.as_bytes() != &frame[trailer_start..] {
        return Err(StoreError::Corrupt("payload hash != frame trailer".into()));
    }
    if let Some(expected) = expected_hash {
        if actual.as_bytes() != expected {
            return Err(StoreError::Corrupt(
                "payload hash != expected object address".into(),
            ));
        }
    }

    Ok((kind, payload))
}

/// LEB128 unsigned varint.
fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Returns `(value, bytes_consumed)`, or `None` on truncation/overflow.
fn read_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    for (i, &byte) in bytes.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_kinds() {
        for kind in [
            ObjectKind::Blob,
            ObjectKind::Tree,
            ObjectKind::Snapshot,
            ObjectKind::Manifest,
            ObjectKind::Chunk,
        ] {
            let payload = b"hello frame".as_slice();
            let frame = encode(kind, payload).unwrap();
            let (got_kind, got_payload) = decode(&frame, None).unwrap();
            assert_eq!(got_kind, kind);
            assert_eq!(got_payload, payload);
        }
    }

    #[test]
    fn roundtrip_empty_and_large() {
        let empty = encode(ObjectKind::Blob, b"").unwrap();
        assert_eq!(decode(&empty, None).unwrap().1, b"");

        let large = vec![42u8; 1_000_000];
        let frame = encode(ObjectKind::Chunk, &large).unwrap();
        // Compressible payload should shrink dramatically.
        assert!(frame.len() < large.len() / 10);
        assert_eq!(decode(&frame, None).unwrap().1, large);
    }

    #[test]
    fn detects_bitflip_in_body() {
        let mut frame = encode(ObjectKind::Blob, b"important data").unwrap();
        let mid = frame.len() / 2;
        frame[mid] ^= 0xff;
        assert!(decode(&frame, None).is_err());
    }

    #[test]
    fn detects_wrong_address() {
        let frame = encode(ObjectKind::Blob, b"content a").unwrap();
        let wrong = *blake3::hash(b"content b").as_bytes();
        let err = decode(&frame, Some(&wrong)).unwrap_err();
        assert!(matches!(err, StoreError::Corrupt(_)));
    }

    #[test]
    fn varint_roundtrip() {
        for value in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            let (got, len) = read_varint(&buf).unwrap();
            assert_eq!(got, value);
            assert_eq!(len, buf.len());
        }
    }
}
