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
        let mut s = String::with_capacity(64);
        for byte in &self.0 {
            s.push_str(&format!("{:02x}", byte));
        }
        s
    }
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
        for i in 0..32 {
            let hex_byte = &s[i * 2..i * 2 + 2];
            bytes[i] =
                u8::from_str_radix(hex_byte, 16).map_err(|_| ContentHashParseError::InvalidHex)?;
        }
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
pub fn hash_file(path: &Path) -> io::Result<ContentHash> {
    let data = std::fs::read(path)?;
    Ok(hash_bytes(&data))
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
}
