//! Wire format versioning and metadata.
//!
//! Defines the [`WireFormat`] header that prefixes every binary message on the
//! wire, along with the current protocol version constant and wire-level errors.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// The current wire protocol version.
///
/// Bump this when making backwards-incompatible changes to the binary encoding.
pub const CURRENT_VERSION: u32 = 1;

/// Magic bytes that identify a Worktree wire message.
/// ASCII for "WKTR" (0x57 0x4B 0x54 0x52).
pub const MAGIC: [u8; 4] = [0x57, 0x4B, 0x54, 0x52];

/// The size of the wire format header in bytes.
/// 4 (magic) + 4 (version) + 4 (payload length) + 1 (flags) = 13 bytes.
pub const HEADER_SIZE: usize = 13;

// ---------------------------------------------------------------------------
// WireError
// ---------------------------------------------------------------------------

/// Errors that can occur during wire-level encoding or decoding.
#[derive(Debug, Clone, Error)]
pub enum WireError {
    /// The magic bytes in the header did not match the expected value.
    #[error("invalid magic bytes: expected {:?}, got {0:?}", MAGIC)]
    InvalidMagic([u8; 4]),

    /// The wire protocol version is not supported.
    #[error("unsupported wire version: {version} (current: {CURRENT_VERSION})")]
    UnsupportedVersion {
        /// The version found in the header.
        version: u32,
    },

    /// The payload length exceeded the maximum allowed size.
    #[error("payload too large: {size} bytes (max: {max})")]
    PayloadTooLarge {
        /// The actual payload size.
        size: u64,
        /// The maximum allowed size.
        max: u64,
    },

    /// The input data was too short to contain a valid header.
    #[error("truncated header: expected at least {HEADER_SIZE} bytes, got {0}")]
    TruncatedHeader(usize),

    /// The input data was too short to contain the full payload.
    #[error("truncated payload: expected {expected} bytes, got {actual}")]
    TruncatedPayload {
        /// The expected number of payload bytes.
        expected: u32,
        /// The actual number of bytes available.
        actual: usize,
    },

    /// A serialization error occurred while encoding.
    #[error("encode error: {0}")]
    EncodeError(String),

    /// A deserialization error occurred while decoding.
    #[error("decode error: {0}")]
    DecodeError(String),

    /// A checksum mismatch was detected.
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// The expected checksum (hex string).
        expected: String,
        /// The actual computed checksum (hex string).
        actual: String,
    },
}

// ---------------------------------------------------------------------------
// WireFlags
// ---------------------------------------------------------------------------

/// Bit flags for wire message options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WireFlags(u8);

impl WireFlags {
    /// No flags set.
    pub const NONE: WireFlags = WireFlags(0);

    /// The payload is compressed (e.g. with zstd or lz4).
    pub const COMPRESSED: WireFlags = WireFlags(1 << 0);

    /// The message includes a trailing checksum.
    pub const CHECKSUMMED: WireFlags = WireFlags(1 << 1);

    /// The payload is encrypted.
    pub const ENCRYPTED: WireFlags = WireFlags(1 << 2);

    /// Create flags from a raw byte.
    pub fn from_byte(byte: u8) -> Self {
        WireFlags(byte)
    }

    /// Return the raw byte.
    pub fn as_byte(&self) -> u8 {
        self.0
    }

    /// Returns `true` if the compressed flag is set.
    pub fn is_compressed(&self) -> bool {
        self.0 & Self::COMPRESSED.0 != 0
    }

    /// Returns `true` if the checksummed flag is set.
    pub fn is_checksummed(&self) -> bool {
        self.0 & Self::CHECKSUMMED.0 != 0
    }

    /// Returns `true` if the encrypted flag is set.
    pub fn is_encrypted(&self) -> bool {
        self.0 & Self::ENCRYPTED.0 != 0
    }

    /// Set the compressed flag.
    pub fn with_compressed(mut self) -> Self {
        self.0 |= Self::COMPRESSED.0;
        self
    }

    /// Set the checksummed flag.
    pub fn with_checksummed(mut self) -> Self {
        self.0 |= Self::CHECKSUMMED.0;
        self
    }

    /// Set the encrypted flag.
    pub fn with_encrypted(mut self) -> Self {
        self.0 |= Self::ENCRYPTED.0;
        self
    }
}

impl Default for WireFlags {
    fn default() -> Self {
        Self::NONE
    }
}

impl fmt::Display for WireFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.is_compressed() {
            parts.push("compressed");
        }
        if self.is_checksummed() {
            parts.push("checksummed");
        }
        if self.is_encrypted() {
            parts.push("encrypted");
        }
        if parts.is_empty() {
            write!(f, "none")
        } else {
            write!(f, "{}", parts.join("|"))
        }
    }
}

// ---------------------------------------------------------------------------
// WireFormat
// ---------------------------------------------------------------------------

/// The wire format header that prefixes every binary message.
///
/// Layout (13 bytes total):
/// ```text
/// ┌───────────┬─────────┬────────────────┬───────┐
/// │ magic (4) │ ver (4) │ payload_len (4)│ flags │
/// └───────────┴─────────┴────────────────┴───────┘
/// ```
///
/// All multi-byte integers are little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WireFormat {
    /// The protocol version number.
    pub version: u32,
    /// The length of the payload in bytes (not including the header).
    pub payload_len: u32,
    /// Message flags (compression, checksum, encryption, etc.).
    pub flags: WireFlags,
}

impl WireFormat {
    /// Create a new wire format header with the current version.
    pub fn new(payload_len: u32) -> Self {
        Self {
            version: CURRENT_VERSION,
            payload_len,
            flags: WireFlags::NONE,
        }
    }

    /// Create a wire format header with specific flags.
    pub fn with_flags(payload_len: u32, flags: WireFlags) -> Self {
        Self {
            version: CURRENT_VERSION,
            payload_len,
            flags,
        }
    }

    /// Encode this header into a 13-byte array.
    pub fn encode_header(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(&MAGIC);
        buf[4..8].copy_from_slice(&self.version.to_le_bytes());
        buf[8..12].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[12] = self.flags.as_byte();
        buf
    }

    /// Decode a header from a byte slice.
    ///
    /// Returns a `WireError` if the input is too short, the magic bytes are
    /// wrong, or the version is unsupported.
    pub fn decode_header(data: &[u8]) -> Result<Self, WireError> {
        if data.len() < HEADER_SIZE {
            return Err(WireError::TruncatedHeader(data.len()));
        }

        let mut magic = [0u8; 4];
        magic.copy_from_slice(&data[0..4]);
        if magic != MAGIC {
            return Err(WireError::InvalidMagic(magic));
        }

        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version > CURRENT_VERSION {
            return Err(WireError::UnsupportedVersion { version });
        }

        let payload_len = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let flags = WireFlags::from_byte(data[12]);

        Ok(Self {
            version,
            payload_len,
            flags,
        })
    }

    /// Validate that the data slice has enough bytes for the full message
    /// (header + payload).
    pub fn validate_payload(&self, data: &[u8]) -> Result<(), WireError> {
        let total = HEADER_SIZE + self.payload_len as usize;
        if data.len() < total {
            return Err(WireError::TruncatedPayload {
                expected: self.payload_len,
                actual: data.len().saturating_sub(HEADER_SIZE),
            });
        }
        Ok(())
    }

    /// Extract the payload bytes from a full message (header + payload).
    pub fn payload<'a>(&self, data: &'a [u8]) -> Result<&'a [u8], WireError> {
        self.validate_payload(data)?;
        Ok(&data[HEADER_SIZE..HEADER_SIZE + self.payload_len as usize])
    }

    /// Returns `true` if this header's version is compatible with the current
    /// protocol version.
    pub fn is_compatible(&self) -> bool {
        self.version <= CURRENT_VERSION
    }

    /// Returns the total message size (header + payload).
    pub fn total_size(&self) -> usize {
        HEADER_SIZE + self.payload_len as usize
    }
}

impl fmt::Display for WireFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WireFormat(v{}, {} bytes, flags={})",
            self.version, self.payload_len, self.flags
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Constants ────────────────────────────────────────────────────────

    #[test]
    fn test_current_version() {
        assert_eq!(CURRENT_VERSION, 1);
    }

    #[test]
    fn test_magic_bytes() {
        assert_eq!(MAGIC, [0x57, 0x4B, 0x54, 0x52]);
        assert_eq!(&MAGIC, b"WKTR");
    }

    #[test]
    fn test_header_size() {
        assert_eq!(HEADER_SIZE, 13);
    }

    // ── WireFlags ───────────────────────────────────────────────────────

    #[test]
    fn test_flags_none() {
        let flags = WireFlags::NONE;
        assert_eq!(flags.as_byte(), 0);
        assert!(!flags.is_compressed());
        assert!(!flags.is_checksummed());
        assert!(!flags.is_encrypted());
    }

    #[test]
    fn test_flags_compressed() {
        let flags = WireFlags::NONE.with_compressed();
        assert!(flags.is_compressed());
        assert!(!flags.is_checksummed());
        assert!(!flags.is_encrypted());
        assert_eq!(flags.as_byte(), 1);
    }

    #[test]
    fn test_flags_checksummed() {
        let flags = WireFlags::NONE.with_checksummed();
        assert!(!flags.is_compressed());
        assert!(flags.is_checksummed());
        assert!(!flags.is_encrypted());
        assert_eq!(flags.as_byte(), 2);
    }

    #[test]
    fn test_flags_encrypted() {
        let flags = WireFlags::NONE.with_encrypted();
        assert!(!flags.is_compressed());
        assert!(!flags.is_checksummed());
        assert!(flags.is_encrypted());
        assert_eq!(flags.as_byte(), 4);
    }

    #[test]
    fn test_flags_combined() {
        let flags = WireFlags::NONE
            .with_compressed()
            .with_checksummed()
            .with_encrypted();
        assert!(flags.is_compressed());
        assert!(flags.is_checksummed());
        assert!(flags.is_encrypted());
        assert_eq!(flags.as_byte(), 0b0000_0111);
    }

    #[test]
    fn test_flags_from_byte() {
        let flags = WireFlags::from_byte(0b0000_0011);
        assert!(flags.is_compressed());
        assert!(flags.is_checksummed());
        assert!(!flags.is_encrypted());
    }

    #[test]
    fn test_flags_display_none() {
        assert_eq!(WireFlags::NONE.to_string(), "none");
    }

    #[test]
    fn test_flags_display_single() {
        assert_eq!(WireFlags::NONE.with_compressed().to_string(), "compressed");
    }

    #[test]
    fn test_flags_display_multiple() {
        let flags = WireFlags::NONE.with_compressed().with_checksummed();
        assert_eq!(flags.to_string(), "compressed|checksummed");
    }

    #[test]
    fn test_flags_default() {
        let flags = WireFlags::default();
        assert_eq!(flags, WireFlags::NONE);
    }

    #[test]
    fn test_flags_serde_roundtrip() {
        let flags = WireFlags::NONE.with_compressed().with_encrypted();
        let json = serde_json::to_string(&flags).expect("serialize");
        let deserialized: WireFlags = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(flags, deserialized);
    }

    // ── WireFormat ──────────────────────────────────────────────────────

    #[test]
    fn test_new() {
        let wf = WireFormat::new(100);
        assert_eq!(wf.version, CURRENT_VERSION);
        assert_eq!(wf.payload_len, 100);
        assert_eq!(wf.flags, WireFlags::NONE);
    }

    #[test]
    fn test_with_flags() {
        let flags = WireFlags::NONE.with_compressed();
        let wf = WireFormat::with_flags(256, flags);
        assert_eq!(wf.version, CURRENT_VERSION);
        assert_eq!(wf.payload_len, 256);
        assert!(wf.flags.is_compressed());
    }

    #[test]
    fn test_total_size() {
        let wf = WireFormat::new(100);
        assert_eq!(wf.total_size(), HEADER_SIZE + 100);
    }

    #[test]
    fn test_is_compatible() {
        let wf = WireFormat::new(0);
        assert!(wf.is_compatible());

        let wf_old = WireFormat {
            version: 0,
            payload_len: 0,
            flags: WireFlags::NONE,
        };
        assert!(wf_old.is_compatible());
    }

    #[test]
    fn test_encode_decode_header_roundtrip() {
        let wf = WireFormat::with_flags(42, WireFlags::NONE.with_checksummed());
        let encoded = wf.encode_header();
        assert_eq!(encoded.len(), HEADER_SIZE);

        let decoded = WireFormat::decode_header(&encoded).expect("decode");
        assert_eq!(wf, decoded);
    }

    #[test]
    fn test_encode_header_magic() {
        let wf = WireFormat::new(0);
        let buf = wf.encode_header();
        assert_eq!(&buf[0..4], &MAGIC);
    }

    #[test]
    fn test_encode_header_version_le() {
        let wf = WireFormat::new(0);
        let buf = wf.encode_header();
        let version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert_eq!(version, CURRENT_VERSION);
    }

    #[test]
    fn test_encode_header_payload_len_le() {
        let wf = WireFormat::new(0x1234_5678);
        let buf = wf.encode_header();
        let len = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        assert_eq!(len, 0x1234_5678);
    }

    #[test]
    fn test_encode_header_flags() {
        let flags = WireFlags::NONE.with_compressed().with_encrypted();
        let wf = WireFormat::with_flags(10, flags);
        let buf = wf.encode_header();
        assert_eq!(buf[12], flags.as_byte());
    }

    #[test]
    fn test_decode_header_truncated() {
        let short = [0u8; 5];
        let result = WireFormat::decode_header(&short);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::TruncatedHeader(len) => assert_eq!(len, 5),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_decode_header_invalid_magic() {
        let mut buf = WireFormat::new(0).encode_header();
        buf[0] = 0xFF;
        let result = WireFormat::decode_header(&buf);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::InvalidMagic(magic) => assert_eq!(magic[0], 0xFF),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_decode_header_unsupported_version() {
        let mut buf = WireFormat::new(0).encode_header();
        // Set version to something huge (future version).
        let future_version: u32 = CURRENT_VERSION + 100;
        buf[4..8].copy_from_slice(&future_version.to_le_bytes());
        let result = WireFormat::decode_header(&buf);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::UnsupportedVersion { version } => {
                assert_eq!(version, future_version);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_validate_payload_ok() {
        let wf = WireFormat::new(5);
        let data = vec![0u8; HEADER_SIZE + 5];
        assert!(wf.validate_payload(&data).is_ok());
    }

    #[test]
    fn test_validate_payload_truncated() {
        let wf = WireFormat::new(100);
        let data = vec![0u8; HEADER_SIZE + 10];
        let result = wf.validate_payload(&data);
        assert!(result.is_err());
        match result.unwrap_err() {
            WireError::TruncatedPayload { expected, actual } => {
                assert_eq!(expected, 100);
                assert_eq!(actual, 10);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_payload_extraction() {
        let payload_data = b"hello";
        let wf = WireFormat::new(payload_data.len() as u32);
        let header = wf.encode_header();

        let mut full_message = Vec::new();
        full_message.extend_from_slice(&header);
        full_message.extend_from_slice(payload_data);

        let extracted = wf.payload(&full_message).expect("extract payload");
        assert_eq!(extracted, payload_data);
    }

    #[test]
    fn test_payload_extraction_truncated() {
        let wf = WireFormat::new(100);
        let header = wf.encode_header();

        let mut short_message = Vec::new();
        short_message.extend_from_slice(&header);
        short_message.extend_from_slice(&[0u8; 10]);

        let result = wf.payload(&short_message);
        assert!(result.is_err());
    }

    #[test]
    fn test_display() {
        let wf = WireFormat::with_flags(42, WireFlags::NONE.with_compressed());
        let display = wf.to_string();
        assert!(display.contains("v1"));
        assert!(display.contains("42 bytes"));
        assert!(display.contains("compressed"));
    }

    #[test]
    fn test_display_no_flags() {
        let wf = WireFormat::new(0);
        let display = wf.to_string();
        assert!(display.contains("flags=none"));
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let wf = WireFormat::with_flags(256, WireFlags::NONE.with_checksummed());
        let json = serde_json::to_string(&wf).expect("serialize");
        let deserialized: WireFormat = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(wf, deserialized);
    }

    #[test]
    fn test_serde_bincode_roundtrip() {
        let wf = WireFormat::with_flags(
            1024,
            WireFlags::NONE.with_compressed().with_encrypted(),
        );
        let encoded = bincode::serialize(&wf).expect("serialize");
        let decoded: WireFormat = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(wf, decoded);
    }

    // ── WireError ───────────────────────────────────────────────────────

    #[test]
    fn test_error_display_invalid_magic() {
        let err = WireError::InvalidMagic([0xFF, 0x00, 0x00, 0x00]);
        let msg = err.to_string();
        assert!(msg.contains("invalid magic bytes"));
    }

    #[test]
    fn test_error_display_unsupported_version() {
        let err = WireError::UnsupportedVersion { version: 999 };
        let msg = err.to_string();
        assert!(msg.contains("999"));
        assert!(msg.contains(&CURRENT_VERSION.to_string()));
    }

    #[test]
    fn test_error_display_payload_too_large() {
        let err = WireError::PayloadTooLarge {
            size: 1_000_000,
            max: 100_000,
        };
        let msg = err.to_string();
        assert!(msg.contains("1000000"));
        assert!(msg.contains("100000"));
    }

    #[test]
    fn test_error_display_truncated_header() {
        let err = WireError::TruncatedHeader(5);
        let msg = err.to_string();
        assert!(msg.contains("5"));
        assert!(msg.contains(&HEADER_SIZE.to_string()));
    }

    #[test]
    fn test_error_display_truncated_payload() {
        let err = WireError::TruncatedPayload {
            expected: 100,
            actual: 50,
        };
        let msg = err.to_string();
        assert!(msg.contains("100"));
        assert!(msg.contains("50"));
    }

    #[test]
    fn test_error_display_encode() {
        let err = WireError::EncodeError("bad data".into());
        assert_eq!(err.to_string(), "encode error: bad data");
    }

    #[test]
    fn test_error_display_decode() {
        let err = WireError::DecodeError("corrupt".into());
        assert_eq!(err.to_string(), "decode error: corrupt");
    }

    #[test]
    fn test_error_display_checksum_mismatch() {
        let err = WireError::ChecksumMismatch {
            expected: "aabb".into(),
            actual: "ccdd".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("aabb"));
        assert!(msg.contains("ccdd"));
    }

    #[test]
    fn test_error_is_clone() {
        let err = WireError::EncodeError("test".into());
        let cloned = err.clone();
        assert_eq!(err.to_string(), cloned.to_string());
    }

    #[test]
    fn test_full_encode_decode_cycle() {
        let payload = b"this is a test payload";
        let wf = WireFormat::with_flags(
            payload.len() as u32,
            WireFlags::NONE.with_checksummed(),
        );

        // Encode
        let header = wf.encode_header();
        let mut message = Vec::with_capacity(wf.total_size());
        message.extend_from_slice(&header);
        message.extend_from_slice(payload);

        // Decode
        let decoded_header = WireFormat::decode_header(&message).expect("decode header");
        assert_eq!(decoded_header, wf);

        let decoded_payload = decoded_header.payload(&message).expect("extract payload");
        assert_eq!(decoded_payload, payload);
    }

    #[test]
    fn test_zero_length_payload() {
        let wf = WireFormat::new(0);
        let header = wf.encode_header();

        let decoded = WireFormat::decode_header(&header).expect("decode");
        assert_eq!(decoded.payload_len, 0);
        assert_eq!(decoded.total_size(), HEADER_SIZE);

        let payload = decoded.payload(&header).expect("extract");
        assert!(payload.is_empty());
    }

    #[test]
    fn test_extra_trailing_data_ignored() {
        let payload = b"data";
        let wf = WireFormat::new(payload.len() as u32);
        let header = wf.encode_header();

        let mut message = Vec::new();
        message.extend_from_slice(&header);
        message.extend_from_slice(payload);
        message.extend_from_slice(b"extra trailing garbage");

        let decoded = WireFormat::decode_header(&message).expect("decode");
        let extracted = decoded.payload(&message).expect("extract");
        assert_eq!(extracted, payload);
    }
}
