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
mod iam_common;

mod admin_api;
mod branch_protection_and_mr;
mod daemon_autosnapshot;
mod git_roundtrip;
mod local_workflow;
mod push_clone_pull;
mod secret_scan;
mod staged_visibility;

// v0.4 IAM (ENGINEERING_PLAN §12).
mod iam_cli_everything;
mod iam_custom_roles;
mod iam_invitations;
mod iam_mfa_login;
mod iam_password_reset;
mod iam_policies;
mod iam_service_accounts;
mod iam_sessions_and_tokens;
mod iam_teams_and_grants;

// v0.5 spec completeness (ENGINEERING_PLAN §13).
mod access_config;
mod chunked_files;
mod daemon_watch;
mod deps_todos;
mod ops_gc_quota;
mod path_security;
mod releases;
mod review_ci;
mod signing;

// v0.6 feature expansion (ENGINEERING_PLAN §14).
mod blame;
mod cli_ux;
mod code_search;
mod policy_history;
mod rewrite;
mod server_merges;
mod storage;
mod webhooks;

// v0.7 build-out (ENGINEERING_PLAN §15).
mod sparse_checkout;
