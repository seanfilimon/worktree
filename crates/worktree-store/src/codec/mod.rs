//! Object frame codec.
//!
//! On-disk object frame per Storage.md:
//!
//! ```text
//! [type: u8][uncompressed size: varint][zstd-compressed body][BLAKE3: 32 bytes]
//! ```
//!
//! Every read verifies the trailing hash before returning bytes.

mod frame;

pub use frame::{decode, encode};
