//! Working-directory scanning and hashing.

pub mod scan;

pub use scan::{collect_files, hash_file_quick, walk_files};
