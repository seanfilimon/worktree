//! W0rkTree end-to-end suite (one test binary, one module per scenario).
//!
//! Every scenario is gated on `WT_E2E=1`; without it each test prints
//! "skipping e2e (set WT_E2E=1)" and passes. Run the whole thing with
//! `bash scripts/e2e.sh` (or `scripts/e2e.ps1`), which builds `wt`,
//! `worktree-bg` and the Go server first.
//!
//! Scenarios are independent (own temp dir, own server on a free port, own
//! `WT_HOME` per simulated user) and safe to run in parallel.

mod common;

mod admin_api;
mod branch_protection_and_mr;
mod daemon_autosnapshot;
mod git_roundtrip;
mod local_workflow;
mod push_clone_pull;
mod secret_scan;
mod staged_visibility;
