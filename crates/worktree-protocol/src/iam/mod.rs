//! # Identity & Access Management
//!
//! The IAM module provides a complete access control system for Worktree,
//! organized by feature domain:
//!
//! - **account** — User accounts (the principal identity in the system)
//! - **tenant** — Multi-tenant isolation and organization
//! - **team** — Groups of accounts within a tenant
//! - **role** — RBAC role definitions with permission sets
//! - **permission** — Atomic permission definitions
//! - **policy** — ABAC policy definitions with attribute conditions
//! - **scope** — Hierarchical scope system (global → tenant → tree → branch)
//! - **session** — Authentication sessions and tokens
//! - **engine** — Central access decision engine combining RBAC + ABAC

pub mod account;
pub mod tenant;
pub mod team;
pub mod role;
pub mod permission;
pub mod policy;
pub mod scope;
pub mod session;
pub mod engine;
