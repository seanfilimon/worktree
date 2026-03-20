pub mod init;
pub mod status;
pub mod snapshot;
pub mod branch;
pub mod merge;
pub mod log;
pub mod sync;
pub mod tree;
pub mod permission;
pub mod git;
pub mod server;
pub mod diff;
pub mod tag;
pub mod config;
pub mod reflog;
pub mod revert;
pub mod archive;
pub mod depend;
pub mod staged;
pub mod ignore;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new worktree repository
    Init {
        /// Path to initialize (defaults to current directory)
        path: Option<String>,
    },
    /// Show the working tree status
    Status {
        /// Show team activity (staged snapshots from others)
        #[arg(long)]
        team: bool,
    },
    /// Create a snapshot of the current state
    Snapshot {
        /// Snapshot message
        #[arg(short, long)]
        message: String,
        /// Tree to snapshot (defaults to current)
        #[arg(short, long)]
        tree: Option<String>,
    },
    /// Show snapshot log
    Log {
        /// Number of entries to show
        #[arg(short = 'n', long, default_value = "20")]
        count: usize,
    },
    /// Manage branches
    Branch {
        #[command(subcommand)]
        action: BranchAction,
    },
    /// Merge a branch into the current branch
    Merge {
        /// Branch to merge
        branch: String,
        /// Merge strategy
        #[arg(long, default_value = "auto")]
        strategy: String,
    },
    /// Sync with remote server
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    /// Manage trees (nested namespaces)
    Tree {
        #[command(subcommand)]
        action: TreeAction,
    },
    /// Show differences
    Diff {
        /// First snapshot ID (or "working" for working tree)
        from: Option<String>,
        /// Second snapshot ID
        to: Option<String>,
        /// Show only file names
        #[arg(long)]
        name_only: bool,
        /// Show only statistics
        #[arg(long)]
        stat: bool,
    },
    /// Manage tags
    Tag {
        #[command(subcommand)]
        action: TagAction,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Show reflog (operation history)
    Reflog {
        /// Number of entries
        #[arg(short = 'n', long, default_value = "20")]
        count: usize,
    },
    /// Revert a snapshot
    Revert {
        /// Snapshot ID to revert
        snapshot: String,
    },
    /// Create an archive
    Archive {
        /// Output file path
        output: String,
        /// Format (tar.gz or zip)
        #[arg(long, default_value = "tar.gz")]
        format: String,
        /// Tree to archive
        #[arg(long)]
        tree: Option<String>,
    },
    /// Manage dependencies
    Depend {
        #[command(subcommand)]
        action: DependAction,
    },
    /// View staged snapshots (team activity)
    Staged {
        #[command(subcommand)]
        action: Option<StagedAction>,
    },
    /// Manage ignore patterns
    Ignore {
        #[command(subcommand)]
        action: IgnoreAction,
    },
    /// Manage permissions and access control
    Permission {
        #[command(subcommand)]
        action: PermissionAction,
    },
    /// Git interoperability commands
    Git {
        #[command(subcommand)]
        action: GitAction,
    },
    /// Manage the worktree background process
    Server {
        #[command(subcommand)]
        action: ServerAction,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum BranchAction {
    /// Create a new branch
    Create {
        name: String,
    },
    /// List all branches
    List,
    /// Switch to a branch
    Switch {
        name: String,
    },
    /// Delete a branch
    Delete {
        name: String,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum SyncAction {
    /// Push local snapshots to remote
    Push,
    /// Pull remote snapshots to local
    Pull,
    /// Pause sync
    Pause,
    /// Resume sync
    Resume,
}

#[derive(Subcommand, Clone, Debug)]
pub enum TreeAction {
    /// Add a new tree
    Add {
        path: String,
    },
    /// List all trees
    List,
    /// Remove a tree
    Remove {
        name: String,
    },
    /// Show tree status
    Status {
        name: Option<String>,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum TagAction {
    /// Create a new tag
    Create {
        name: String,
        /// Tag message
        #[arg(short, long)]
        message: Option<String>,
    },
    /// List all tags
    List,
    /// Delete a tag
    Delete {
        name: String,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum ConfigAction {
    /// Show current configuration
    Show,
    /// Get a specific config value
    Get {
        key: String,
    },
    /// Set a config value
    Set {
        key: String,
        value: String,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum DependAction {
    /// Add a dependency
    Add {
        /// Source tree
        tree: String,
        /// Target tree or branch
        target: String,
        /// Mark as blocking
        #[arg(long)]
        blocking: bool,
    },
    /// List dependencies
    List,
    /// Show todo items
    Todo,
}

#[derive(Subcommand, Clone, Debug)]
pub enum StagedAction {
    /// List staged snapshots
    List,
    /// Clear staged snapshots
    Clear,
}

#[derive(Subcommand, Clone, Debug)]
pub enum IgnoreAction {
    /// List active ignore patterns
    List,
    /// Add an ignore pattern
    Add {
        pattern: String,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum PermissionAction {
    /// Set permissions on a tree
    Set {
        tree: String,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        allow: String,
    },
    /// Get permissions for a tree
    Get {
        tree: String,
    },
    /// List all permissions
    List,
}

#[derive(Subcommand, Clone, Debug)]
pub enum GitAction {
    /// Import from a git repository
    Import {
        source: String,
    },
    /// Export to a git repository
    Export {
        tree: String,
        #[arg(long)]
        output: String,
        #[arg(long, default_value = "full")]
        mode: String,
    },
    /// Clone a git repository
    Clone {
        url: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Manage git remotes
    Remote {
        #[command(subcommand)]
        action: GitRemoteAction,
    },
    /// Push to a git remote
    Push {
        remote: String,
        branch: String,
    },
    /// Pull from a git remote
    Pull {
        remote: String,
        branch: String,
    },
    /// Mirror a tree to a git remote
    Mirror {
        tree: String,
        #[arg(long)]
        remote: String,
        #[arg(long)]
        branch: String,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum GitRemoteAction {
    /// Add a git remote
    Add {
        name: String,
        url: String,
    },
    /// List git remotes
    List,
    /// Remove a git remote
    Remove {
        name: String,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum ServerAction {
    /// Start the background process
    Start,
    /// Stop the background process
    Stop,
    /// Show status
    Status,
}

pub async fn execute(cmd: Commands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::Init { path } => init::execute(path).await,
        Commands::Status { team } => status::execute(team).await,
        Commands::Snapshot { message, tree } => snapshot::execute(tree, message).await,
        Commands::Log { count } => log::execute(count).await,
        Commands::Branch { action } => branch::execute(action).await,
        Commands::Merge { branch, strategy } => merge::execute(branch, strategy).await,
        Commands::Sync { action } => sync::execute(action).await,
        Commands::Tree { action } => tree::execute(action).await,
        Commands::Diff { from, to, name_only, stat } => diff::execute(from, to, name_only, stat).await,
        Commands::Tag { action } => tag::execute(action).await,
        Commands::Config { action } => config::execute(action).await,
        Commands::Reflog { count } => reflog::execute(count).await,
        Commands::Revert { snapshot } => revert::execute(snapshot).await,
        Commands::Archive { output, format, tree } => archive::execute(output, format, tree).await,
        Commands::Depend { action } => depend::execute(action).await,
        Commands::Staged { action } => staged::execute(action).await,
        Commands::Ignore { action } => ignore::execute(action).await,
        Commands::Permission { action } => permission::execute(action).await,
        Commands::Git { action } => git::execute(action).await,
        Commands::Server { action } => server::execute(action).await,
    }
}
