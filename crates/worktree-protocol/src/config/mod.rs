pub mod hierarchy;
pub mod tree_config;
pub mod worktree_config;

// config-as-code (ENGINEERING_PLAN §16.3): `.wt-tree/{config,branches,
// protections}.toml` and `.wt-tree/access/` validation with located
// diagnostics, the same rules and codes as the Go server.
pub mod as_code;
pub mod branches;
pub mod protections;
