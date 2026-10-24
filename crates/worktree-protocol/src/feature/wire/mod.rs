//! # Binary Wire Format
//!
//! Encoding, decoding, and versioning for the Worktree binary wire protocol.
//! All on-the-wire messages are length-prefixed and versioned to allow
//! forward- and backward-compatible evolution of the protocol.

pub mod encode;
pub mod decode;
pub mod format;
