use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::io;
use std::path::Path;
use std::str::FromStr;

/// A BLAKE3 content-addressable hash, stored as a 32-byte array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// The zero hash (all bytes zero).
    pub const ZERO: ContentHash = ContentHash([0u8; 32]);

    /// Construct from a raw byte array.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        ContentHash(bytes)
    }

    /// Return a reference to the underlying byte array.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Encode the hash as a lowercase hexadecimal string.
    pub fn to_hex(&self) -> String {
        self.to_string()
    }

    /// Parse a 64-character hex string (case-insensitive).
    pub fn from_hex(hex: &str) -> Result<Self, ContentHashParseError> {
        hex.parse()
    }

    /// Construct from a byte slice, returning `None` unless it is exactly
    /// 32 bytes long.
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(ContentHash)
    }

    /// Returns `true` if this is the zero hash.
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }

    /// The first `len` hex characters (at most 64), for display.
    pub fn short_hex(&self, len: usize) -> String {
        let mut hex = self.to_hex();
        hex.truncate(len.min(64));
        hex
    }
}

impl AsRef<[u8]> for ContentHash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Decode the ASCII hex digits in `hex` into `out` (`hex.len()` must be
/// `2 * out.len()`). Returns `None` on any non-hex byte. Unlike
/// `u8::from_str_radix`, this rejects sign characters such as `+`, and it
/// never slices inside a multi-byte UTF-8 character.
pub(crate) fn decode_hex_into(hex: &[u8], out: &mut [u8]) -> Option<()> {
    fn nibble(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    if hex.len() != out.len() * 2 {
        return None;
    }
    for (byte, pair) in out.iter_mut().zip(hex.as_chunks::<2>().0) {
        *byte = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(())
}

impl Default for ContentHash {
    fn default() -> Self {
        ContentHash::ZERO
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl FromStr for ContentHash {
    type Err = ContentHashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(ContentHashParseError::InvalidLength(s.len()));
        }
        let mut bytes = [0u8; 32];
        decode_hex_into(s.as_bytes(), &mut bytes).ok_or(ContentHashParseError::InvalidHex)?;
        Ok(ContentHash(bytes))
    }
}

/// Error returned when parsing a hex string into a [`ContentHash`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentHashParseError {
    /// The hex string had an unexpected length (expected 64 characters).
    InvalidLength(usize),
    /// The string contained non-hex characters.
    InvalidHex,
}

impl fmt::Display for ContentHashParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContentHashParseError::InvalidLength(len) => {
                write!(f, "expected 64 hex characters, got {}", len)
            }
            ContentHashParseError::InvalidHex => write!(f, "invalid hex character"),
        }
    }
}

impl std::error::Error for ContentHashParseError {}

impl Serialize for ContentHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_hex())
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            ContentHash::from_str(&s).map_err(serde::de::Error::custom)
        } else {
            struct BytesVisitor;

            impl<'de> serde::de::Visitor<'de> for BytesVisitor {
                type Value = ContentHash;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("32 bytes")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    if v.len() != 32 {
                        return Err(E::invalid_length(v.len(), &"32 bytes"));
                    }
                    let mut bytes = [0u8; 32];
                    bytes.copy_from_slice(v);
                    Ok(ContentHash(bytes))
                }

                fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                where
                    A: serde::de::SeqAccess<'de>,
                {
                    let mut bytes = [0u8; 32];
                    for (i, byte) in bytes.iter_mut().enumerate() {
                        *byte = seq
                            .next_element()?
                            .ok_or_else(|| serde::de::Error::invalid_length(i, &"32 bytes"))?;
                    }
                    Ok(ContentHash(bytes))
                }
            }

            deserializer.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Hash arbitrary bytes with BLAKE3, returning a [`ContentHash`].
pub fn hash_bytes(data: &[u8]) -> ContentHash {
    let hash = blake3::hash(data);
    ContentHash(*hash.as_bytes())
}

/// Hash the contents of a file at the given path with BLAKE3.
///
/// The file is streamed, so large files are hashed without loading them
/// into memory. The result equals `hash_bytes(&fs::read(path)?)`.
pub fn hash_file(path: &Path) -> io::Result<ContentHash> {
    let file = std::fs::File::open(path)?;
    hash_reader(io::BufReader::new(file))
}

/// Hash everything readable from `reader` with BLAKE3.
pub fn hash_reader<R: io::Read>(mut reader: R) -> io::Result<ContentHash> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(&buf[..n]);
    }
    Ok(ContentHash(*hasher.finalize().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_hex() {
        let h = hash_bytes(b"hello world");
        let hex = h.to_hex();
        let parsed: ContentHash = hex.parse().unwrap();
        assert_eq!(h, parsed);
    }

    #[test]
    fn test_display_and_fromstr_roundtrip() {
        let h = hash_bytes(b"test data");
        let display = format!("{}", h);
        assert_eq!(display.len(), 64);
        let parsed: ContentHash = display.parse().unwrap();
        assert_eq!(h, parsed);
    }

    #[test]
    fn test_uniqueness() {
        let h1 = hash_bytes(b"data one");
        let h2 = hash_bytes(b"data two");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_zero() {
        assert_eq!(ContentHash::ZERO.as_bytes(), &[0u8; 32]);
        assert_eq!(ContentHash::default(), ContentHash::ZERO);
    }

    #[test]
    fn test_from_bytes() {
        let bytes = [42u8; 32];
        let h = ContentHash::from_bytes(bytes);
        assert_eq!(h.as_bytes(), &bytes);
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let h = hash_bytes(b"json test");
        let json = serde_json::to_string(&h).unwrap();
        // Human-readable format should be a hex string
        assert!(json.starts_with('"'));
        let deserialized: ContentHash = serde_json::from_str(&json).unwrap();
        assert_eq!(h, deserialized);
    }

    #[test]
    fn test_serde_bincode_roundtrip() {
        let h = hash_bytes(b"bincode test");
        let encoded = bincode::serialize(&h).unwrap();
        let decoded: ContentHash = bincode::deserialize(&encoded).unwrap();
        assert_eq!(h, decoded);
    }

    #[test]
    fn test_fromstr_invalid_length() {
        let result = "abcd".parse::<ContentHash>();
        assert!(result.is_err());
        match result.unwrap_err() {
            ContentHashParseError::InvalidLength(len) => assert_eq!(len, 4),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_fromstr_invalid_hex() {
        let bad = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert_eq!(bad.len(), 64);
        let result = bad.parse::<ContentHash>();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ContentHashParseError::InvalidHex);
    }

    #[test]
    fn test_hash_matches_blake3_reference() {
        // BLAKE3 of the empty input (official test vector).
        assert_eq!(
            hash_bytes(b"").to_hex(),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }

    #[test]
    fn test_fromstr_rejects_sign_characters() {
        // Regression: `u8::from_str_radix("+f", 16)` succeeds, so "+f+f..."
        // used to parse as a valid hash.
        let bad = "+f".repeat(32);
        assert_eq!(
            bad.parse::<ContentHash>(),
            Err(ContentHashParseError::InvalidHex)
        );
    }

    #[test]
    fn test_fromstr_multibyte_does_not_panic() {
        // Regression: 64 bytes long, but with 2-byte characters straddling
        // the byte offsets the old implementation sliced at (panicked).
        let bad = format!("a{}b", "\u{e9}".repeat(31));
        assert_eq!(bad.len(), 64);
        assert_eq!(
            bad.parse::<ContentHash>(),
            Err(ContentHashParseError::InvalidHex)
        );
    }

    #[test]
    fn test_fromstr_accepts_uppercase() {
        let h = hash_bytes(b"case");
        let upper = h.to_hex().to_uppercase();
        assert_eq!(ContentHash::from_hex(&upper).unwrap(), h);
    }

    #[test]
    fn test_from_slice_and_helpers() {
        let h = hash_bytes(b"slice");
        assert_eq!(ContentHash::from_slice(h.as_bytes()), Some(h));
        assert_eq!(ContentHash::from_slice(&[0u8; 31]), None);
        assert_eq!(ContentHash::from_slice(&[0u8; 33]), None);
        assert!(ContentHash::ZERO.is_zero());
        assert!(!h.is_zero());
        assert_eq!(h.short_hex(8), h.to_hex()[..8]);
        assert_eq!(h.short_hex(1000).len(), 64);
        assert_eq!(h.as_ref(), h.as_bytes());
    }

    #[test]
    fn test_hash_reader_and_file_match_hash_bytes() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        assert_eq!(hash_reader(&data[..]).unwrap(), hash_bytes(&data));

        let dir = std::env::temp_dir().join(format!("wt-hash-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("blob.bin");
        std::fs::write(&path, &data).unwrap();
        let hashed = hash_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(hashed.unwrap(), hash_bytes(&data));
    }

    #[test]
    fn test_bincode_rejects_wrong_length() {
        let encoded = bincode::serialize(&vec![1u8; 31]).unwrap();
        assert!(bincode::deserialize::<ContentHash>(&encoded).is_err());
    }
}
