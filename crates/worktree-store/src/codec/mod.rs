//! Object frame codec (lands in WT-PHASE-2).
//!
//! On-disk object frame per Storage.md:
//!
//! ```text
//! [type: u8][uncompressed size: varint][zstd-compressed body][BLAKE3: 32 bytes]
//! ```
//!
//! Every read verifies the trailing hash before returning bytes.
