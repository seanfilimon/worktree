//! # Resource-Level Access Control
//!
//! Provides per-tree and per-branch access control rules. These are the
//! applied access control entries (ACEs) that bind IAM subjects to specific
//! version control resources.

pub mod resource;
pub mod tree_access;
pub mod branch_access;
