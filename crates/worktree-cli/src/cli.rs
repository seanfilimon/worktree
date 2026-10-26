//! Command-line definition (clap derive). Per-command arguments live next
//! to their implementation in [`crate::commands`].

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::commands::{
    access, admin, api, archive, audit, auth, blame, branch, cherry_pick, ci, completions, config,
    daemon, deps, diff, doctor, events, fsck, gc, git, grant, history, iam, identity, ignore,
    inbox, init, invite, license, merge, mfa, mirror, mr, open, policy, protect, rebase, release,
    remote, role, sa, search, session, snapshot, sparse, sso, stash, status, sync, synchronize,
    tag, team, tenant, todo, token, tree, trees, webhook,
};

#[derive(Debug, Parser)]
#[command(
    name = "wt",
    bin_name = "wt",
    version,
    about = "W0rkTree — next-generation version control",
    long_about = "W0rkTree is a version control system with multi-tenant servers, \
                  nested trees, automatic snapshots and team-visible staged work.",
    propagate_version = true,
    max_term_width = 100
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// Print machine-readable JSON (one document) on stdout
    #[arg(long, global = true)]
    pub json: bool,

    /// Run as if wt was started in PATH
    #[arg(short = 'C', value_name = "PATH", global = true)]
    pub dir: Option<PathBuf>,

    /// Operate on this tree instead of the one containing the current directory
    #[arg(long, value_name = "NAME", global = true)]
    pub tree: Option<String>,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Server for server-side commands (default: this worktree's origin, else the only saved login)
    #[arg(long, value_name = "URL", global = true, env = "WT_SERVER")]
    pub server: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new worktree
    Init(init::InitArgs),
    /// Show the working tree status
    Status(status::StatusArgs),
    /// Record the working state as a new snapshot
    Snapshot(snapshot::SnapshotArgs),
    /// Show snapshot history
    Log(history::LogArgs),
    /// Show a snapshot and its changes
    Show(history::ShowArgs),
    /// Show changes between the working tree and snapshots
    Diff(diff::DiffArgs),
    /// List, create, archive or restore branches
    Branch(branch::BranchArgs),
    /// Switch branches
    Switch(branch::SwitchArgs),
    /// Merge a branch into the current branch
    Merge(merge::MergeArgs),
    /// Create a snapshot that undoes another snapshot
    Revert(merge::RevertArgs),
    /// Restore working files from a snapshot
    Restore(merge::RestoreArgs),
    /// List, create or delete tags
    Tag(tag::TagArgs),
    /// Manage nested trees
    Tree(tree::TreeArgs),
    /// Show the reference log
    Reflog(history::ReflogArgs),
    /// Manage ignore patterns
    Ignore(ignore::IgnoreArgs),
    /// Read and write worktree configuration
    Config(config::ConfigArgs),
    /// Manage remote servers
    Remote(remote::RemoteArgs),
    /// Log in to a server
    Login(auth::LoginArgs),
    /// Forget the saved credential for a server
    Logout(auth::LogoutArgs),
    /// Show who you are logged in as
    Whoami(auth::WhoamiArgs),
    /// Create an account on a server
    Register(auth::RegisterArgs),
    /// Set a new password with a one-time reset code from an administrator
    PasswordReset(auth::PasswordResetArgs),
    /// Upload a branch to a remote
    Push(sync::PushArgs),
    /// Download remote branches without touching local ones
    Fetch(sync::FetchArgs),
    /// Fetch and integrate a remote branch
    Pull(sync::PullArgs),
    /// Clone a tree from a server
    Clone(sync::CloneArgs),
    /// Upload work in progress as a team-visible staged snapshot
    Stage(sync::StageArgs),
    // daemon workstream (§13.11)
    /// Remove your staged snapshot from the remote
    Unstage(sync::UnstageArgs),
    /// Write a snapshot's files to a tar.gz or zip archive
    Archive(archive::ArchiveArgs),
    /// Import from or export to Git
    Git(git::GitArgs),
    /// Control the background daemon
    Daemon(daemon::DaemonArgs),
    /// Manage tenants (organizations) and their members on the server
    Tenant(tenant::TenantArgs),
    /// Manage trees on the server: create, settings, branches, tags, compare, stats
    Trees(trees::TreesArgs),
    /// Merge requests: list, open, review, approve, merge, close
    Mr(mr::MrArgs),
    /// Branch protection rules of a server tree
    Protect(protect::ProtectArgs),
    /// Path license rules of a server tree
    License(license::LicenseArgs),
    /// Query the server audit log
    Audit(audit::AuditArgs),
    /// Server administration: accounts, system information, statistics
    Admin(admin::AdminArgs),
    /// Search tenants, trees, branches and accounts on the server (`wt search code`: file contents)
    Search(search::SearchArgs),
    /// Stream live server events
    Events(events::EventsArgs),
    /// Teams within a tenant and their members
    Team(team::TeamArgs),
    /// Custom roles of a tenant
    Role(role::RoleArgs),
    /// Access policies (allow/deny statements) of a tenant
    Policy(policy::PolicyArgs),
    /// Per-tree access grants for users, teams and service accounts
    Grant(grant::GrantArgs),
    /// Service accounts and their keys
    Sa(sa::SaArgs),
    /// Your personal API tokens
    Token(token::TokenArgs),
    /// Your sign-in sessions
    Session(session::SessionArgs),
    /// Two-factor authentication (TOTP) and recovery codes
    Mfa(mfa::MfaArgs),
    /// Tenant invitations
    Invite(invite::InviteArgs),
    /// Inspect access: simulate requests, effective permissions, action catalog
    Iam(iam::IamArgs),
    /// Single sign-on (OpenID Connect) configuration
    Sso(sso::SsoArgs),
    /// TODOs created from snapshot dependencies: list, show, claim, complete, cancel
    Todo(todo::TodoArgs),
    /// Dependencies declared by snapshots: status and graph
    Deps(deps::DepsArgs),
    /// Releases of a server tree: list, create, show, edit, delete, assets
    Release(release::ReleaseArgs),
    /// CI check results: report a status, show checks of a snapshot or branch
    Ci(ci::CiArgs),
    /// Declarative access config (.wt/access): validate, plan, apply, pull
    Access(access::AccessArgs),
    /// Snapshot signing keys: generate, show, register, list, revoke
    Identity(identity::IdentityArgs),
    /// Outgoing webhooks of a tenant or tree: list, create, update, delete, deliveries, redeliver, test
    Webhook(webhook::WebhookArgs),
    /// Show which snapshot last changed each line of a file
    Blame(blame::BlameArgs),
    /// Apply the changes of existing snapshots onto the current branch
    CherryPick(cherry_pick::CherryPickArgs),
    /// Replay the current branch's snapshots onto another base
    Rebase(rebase::RebaseArgs),
    /// Set working changes aside and restore them later
    Stash(stash::StashArgs),
    /// Remove unreachable local objects and compact the local store
    Gc(gc::GcArgs),
    /// Verify the integrity of the local store and history
    Fsck(fsck::FsckArgs),
    /// Print shell completion scripts (bash, zsh, fish, powershell, elvish)
    Completions(completions::CompletionsArgs),
    /// Diagnose the local setup: config, credentials, server, daemon
    Doctor(doctor::DoctorArgs),
    /// Open the current tree (or a merge request, snapshot or file) in the admin panel
    Open(open::OpenArgs),
    /// Call any REST API endpoint of the server with your credentials
    Api(api::ApiArgs),
    /// Sparse checkout: check out and fetch only the paths you need
    Sparse(sparse::SparseArgs),
    /// Fetch, integrate (merge or rebase) and push in one step; manage the offline queue
    Sync(synchronize::SyncArgs),
    /// Your notifications: list, read, mark done, watch settings
    Inbox(inbox::InboxArgs),
    /// Live Git mirrors of a server tree: list, add, update, remove, sync now
    Mirror(mirror::MirrorArgs),
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::error::ErrorKind;
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Cli {
        let argv = std::iter::once("wt").chain(args.iter().copied());
        match Cli::try_parse_from(argv) {
            Ok(cli) => cli,
            Err(e) => panic!("failed to parse {args:?}: {e}"),
        }
    }

    fn parse_err(args: &[&str]) -> ErrorKind {
        let argv = std::iter::once("wt").chain(args.iter().copied());
        match Cli::try_parse_from(argv) {
            Ok(cli) => panic!("{args:?} should not parse, got {cli:?}"),
            Err(e) => e.kind(),
        }
    }

    #[test]
    fn definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn global_flags_anywhere() {
        let cli = parse(&[
            "--json",
            "-C",
            "some/dir",
            "status",
            "--tree",
            "web",
            "--no-color",
        ]);
        assert!(cli.global.json);
        assert!(cli.global.no_color);
        assert_eq!(cli.global.dir, Some(PathBuf::from("some/dir")));
        assert_eq!(cli.global.tree.as_deref(), Some("web"));
        assert!(matches!(cli.command, Command::Status(_)));

        let cli = parse(&["log", "--json"]);
        assert!(cli.global.json);
        assert!(!cli.global.no_color);
        assert_eq!(cli.global.dir, None);
    }

    #[test]
    fn missing_or_unknown_subcommand_is_a_usage_error() {
        assert_eq!(
            parse_err(&[]),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert_eq!(parse_err(&["frobnicate"]), ErrorKind::InvalidSubcommand);
        assert_eq!(
            parse_err(&["status", "--bogus"]),
            ErrorKind::UnknownArgument
        );
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse_err(&["--help"]), ErrorKind::DisplayHelp);
        assert_eq!(parse_err(&["--version"]), ErrorKind::DisplayVersion);
        assert_eq!(parse_err(&["branch", "--help"]), ErrorKind::DisplayHelp);
    }

    #[test]
    fn init() {
        let Command::Init(a) = parse(&["init"]).command else {
            panic!()
        };
        assert_eq!((a.path, a.name, a.branch), (None, None, None));
        let Command::Init(a) = parse(&["init", "proj", "--name", "p", "--branch", "trunk"]).command
        else {
            panic!()
        };
        assert_eq!(a.path, Some(PathBuf::from("proj")));
        assert_eq!(a.name.as_deref(), Some("p"));
        assert_eq!(a.branch.as_deref(), Some("trunk"));
    }

    #[test]
    fn status() {
        let Command::Status(a) = parse(&["status"]).command else {
            panic!()
        };
        assert!(!a.team);
        let Command::Status(a) = parse(&["status", "--team"]).command else {
            panic!()
        };
        assert!(a.team);
    }

    #[test]
    fn snapshot() {
        let Command::Snapshot(a) = parse(&["snapshot", "-m", "msg"]).command else {
            panic!()
        };
        assert_eq!(a.message, "msg");
        assert!(!a.allow_empty && !a.no_scan);
        let Command::Snapshot(a) =
            parse(&["snapshot", "--message", "m", "--allow-empty", "--no-scan"]).command
        else {
            panic!()
        };
        assert!(a.allow_empty && a.no_scan);
        assert_eq!(parse_err(&["snapshot"]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn log() {
        let Command::Log(a) = parse(&["log"]).command else {
            panic!()
        };
        assert_eq!((a.max_count, a.rev, a.oneline), (None, None, false));
        let Command::Log(a) = parse(&["log", "-n", "5", "feature", "--oneline"]).command else {
            panic!()
        };
        assert_eq!(a.max_count, Some(5));
        assert_eq!(a.rev.as_deref(), Some("feature"));
        assert!(a.oneline);
        assert_eq!(
            parse_err(&["log", "-n", "many"]),
            ErrorKind::ValueValidation
        );
    }

    #[test]
    fn show() {
        let Command::Show(a) = parse(&["show", "HEAD~1", "--stat"]).command else {
            panic!()
        };
        assert_eq!(a.rev, "HEAD~1");
        assert!(a.stat);
        assert_eq!(parse_err(&["show"]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn diff() {
        let Command::Diff(a) = parse(&["diff"]).command else {
            panic!()
        };
        assert!(a.args.is_empty() && a.paths.is_empty() && !a.name_only && !a.stat);
        let Command::Diff(a) = parse(&[
            "diff",
            "HEAD~2",
            "HEAD",
            "src/lib.rs",
            "--stat",
            "--",
            "docs",
        ])
        .command
        else {
            panic!()
        };
        assert_eq!(a.args, ["HEAD~2", "HEAD", "src/lib.rs"]);
        assert_eq!(a.paths, [PathBuf::from("docs")]);
        assert!(a.stat);
        let Command::Diff(a) = parse(&["diff", "--name-only"]).command else {
            panic!()
        };
        assert!(a.name_only);
        assert_eq!(
            parse_err(&["diff", "--stat", "--name-only"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn branch() {
        let Command::Branch(a) = parse(&["branch"]).command else {
            panic!()
        };
        assert!(a.action.is_none() && !a.all);
        let Command::Branch(a) = parse(&["branch", "-a"]).command else {
            panic!()
        };
        assert!(a.all);
        let Command::Branch(a) = parse(&["branch", "list", "--all"]).command else {
            panic!()
        };
        assert!(matches!(
            a.action,
            Some(branch::BranchCommand::List { all: true })
        ));
        let Command::Branch(a) = parse(&["branch", "create", "feat", "--from", "HEAD~1"]).command
        else {
            panic!()
        };
        let Some(branch::BranchCommand::Create { name, from }) = a.action else {
            panic!()
        };
        assert_eq!((name.as_str(), from.as_deref()), ("feat", Some("HEAD~1")));
        let Command::Branch(a) = parse(&["branch", "archive", "old"]).command else {
            panic!()
        };
        assert!(matches!(a.action, Some(branch::BranchCommand::Archive { name }) if name == "old"));
        let Command::Branch(a) = parse(&["branch", "restore", "old"]).command else {
            panic!()
        };
        assert!(matches!(a.action, Some(branch::BranchCommand::Restore { name }) if name == "old"));
        assert_eq!(
            parse_err(&["branch", "create"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn switch() {
        let Command::Switch(a) = parse(&["switch", "dev"]).command else {
            panic!()
        };
        assert_eq!(a.name, "dev");
        assert!(!a.create && !a.force);
        let Command::Switch(a) = parse(&["switch", "-c", "new", "--force"]).command else {
            panic!()
        };
        assert!(a.create && a.force);
        assert_eq!(parse_err(&["switch"]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn merge() {
        let Command::Merge(a) = parse(&["merge", "feature", "-m", "msg", "--no-ff"]).command else {
            panic!()
        };
        assert_eq!(a.source.as_deref(), Some("feature"));
        assert_eq!(a.message.as_deref(), Some("msg"));
        assert!(a.no_ff && !a.resume && !a.abort);
        let Command::Merge(a) = parse(&["merge", "--continue", "-m", "done"]).command else {
            panic!()
        };
        assert!(a.resume && a.source.is_none());
        let Command::Merge(a) = parse(&["merge", "--abort"]).command else {
            panic!()
        };
        assert!(a.abort);
        assert_eq!(parse_err(&["merge"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(
            parse_err(&["merge", "x", "--abort"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["merge", "--continue", "--abort"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["merge", "--abort", "-m", "x"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["merge", "--continue", "--no-ff"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn revert_and_restore() {
        let Command::Revert(a) = parse(&["revert", "abcd1234"]).command else {
            panic!()
        };
        assert_eq!(a.rev, "abcd1234");
        assert_eq!(parse_err(&["revert"]), ErrorKind::MissingRequiredArgument);

        let Command::Restore(a) = parse(&["restore"]).command else {
            panic!()
        };
        assert!(a.paths.is_empty() && a.from.is_none());
        let Command::Restore(a) = parse(&["restore", "a.txt", "dir", "--from", "HEAD~1"]).command
        else {
            panic!()
        };
        assert_eq!(a.paths, [PathBuf::from("a.txt"), PathBuf::from("dir")]);
        assert_eq!(a.from.as_deref(), Some("HEAD~1"));
    }

    #[test]
    fn tag() {
        let Command::Tag(a) = parse(&["tag"]).command else {
            panic!()
        };
        assert!(a.action.is_none());
        let Command::Tag(a) = parse(&["tag", "list"]).command else {
            panic!()
        };
        assert!(matches!(a.action, Some(tag::TagCommand::List)));
        let Command::Tag(a) = parse(&["tag", "create", "v1", "HEAD~1", "-m", "first"]).command
        else {
            panic!()
        };
        let Some(tag::TagCommand::Create {
            name, rev, message, ..
        }) = a.action
        else {
            panic!()
        };
        assert_eq!(name, "v1");
        assert_eq!(rev.as_deref(), Some("HEAD~1"));
        assert_eq!(message.as_deref(), Some("first"));
        let Command::Tag(a) = parse(&["tag", "delete", "v1"]).command else {
            panic!()
        };
        assert!(matches!(a.action, Some(tag::TagCommand::Delete { name }) if name == "v1"));
    }

    #[test]
    fn tree() {
        let Command::Tree(a) = parse(&["tree", "list"]).command else {
            panic!()
        };
        assert!(matches!(a.command, tree::TreeCommand::List));
        let Command::Tree(a) = parse(&["tree", "add", "web", "apps/web"]).command else {
            panic!()
        };
        let tree::TreeCommand::Add { name, path } = a.command else {
            panic!()
        };
        assert_eq!((name.as_str(), path), ("web", PathBuf::from("apps/web")));
        let Command::Tree(a) = parse(&["tree", "remove", "web"]).command else {
            panic!()
        };
        assert!(matches!(a.command, tree::TreeCommand::Remove { name } if name == "web"));
        assert_eq!(
            parse_err(&["tree"]),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn reflog() {
        let Command::Reflog(a) = parse(&["reflog"]).command else {
            panic!()
        };
        assert_eq!(a.max_count, None);
        let Command::Reflog(a) = parse(&["reflog", "-n", "3"]).command else {
            panic!()
        };
        assert_eq!(a.max_count, Some(3));
    }

    #[test]
    fn ignore() {
        let Command::Ignore(a) = parse(&["ignore", "list"]).command else {
            panic!()
        };
        assert!(matches!(a.command, ignore::IgnoreCommand::List { .. }));
        let Command::Ignore(a) = parse(&["ignore", "add", "*.log"]).command else {
            panic!()
        };
        assert!(
            matches!(a.command, ignore::IgnoreCommand::Add { pattern, .. } if pattern == "*.log")
        );
        let Command::Ignore(a) = parse(&["ignore", "check", "build/out.o"]).command else {
            panic!()
        };
        assert!(
            matches!(a.command, ignore::IgnoreCommand::Check { path } if path == Path::new("build/out.o"))
        );
    }

    #[test]
    fn config() {
        let Command::Config(a) = parse(&["config", "list"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            config::ConfigCommand::List { show_origin: false }
        ));
        let Command::Config(a) = parse(&["config", "get", "user.name"]).command else {
            panic!()
        };
        assert!(matches!(a.command, config::ConfigCommand::Get { key, .. } if key == "user.name"));
        let Command::Config(a) = parse(&["config", "set", "user.name", "Ada Lovelace"]).command
        else {
            panic!()
        };
        let config::ConfigCommand::Set { key, value, .. } = a.command else {
            panic!()
        };
        assert_eq!(
            (key.as_str(), value.as_str()),
            ("user.name", "Ada Lovelace")
        );
        assert_eq!(
            parse_err(&["config", "set", "k"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn remote() {
        let Command::Remote(a) = parse(&["remote", "list"]).command else {
            panic!()
        };
        assert!(matches!(a.command, remote::RemoteCommand::List));
        let Command::Remote(a) = parse(&[
            "remote",
            "add",
            "origin",
            "http://localhost:7420",
            "--tenant",
            "acme",
        ])
        .command
        else {
            panic!()
        };
        let remote::RemoteCommand::Add { name, url, tenant } = a.command else {
            panic!()
        };
        assert_eq!(
            (name.as_str(), url.as_str(), tenant.as_str()),
            ("origin", "http://localhost:7420", "acme")
        );
        let Command::Remote(a) = parse(&["remote", "remove", "origin"]).command else {
            panic!()
        };
        assert!(matches!(a.command, remote::RemoteCommand::Remove { name } if name == "origin"));
        assert_eq!(
            parse_err(&["remote", "add", "origin", "http://x"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn auth() {
        let Command::Login(a) = parse(&["login"]).command else {
            panic!()
        };
        assert!(a.server.is_none() && a.username.is_none() && !a.password_stdin);
        let Command::Login(a) = parse(&[
            "login",
            "http://localhost:7420",
            "-u",
            "ada",
            "--password-stdin",
        ])
        .command
        else {
            panic!()
        };
        assert_eq!(a.server.as_deref(), Some("http://localhost:7420"));
        assert_eq!(a.username.as_deref(), Some("ada"));
        assert!(a.password_stdin);

        let Command::Logout(a) = parse(&["logout"]).command else {
            panic!()
        };
        assert!(a.server.is_none());
        let Command::Logout(a) = parse(&["logout", "http://s"]).command else {
            panic!()
        };
        assert_eq!(a.server.as_deref(), Some("http://s"));

        let Command::Whoami(a) = parse(&["whoami", "http://s"]).command else {
            panic!()
        };
        assert_eq!(a.server.as_deref(), Some("http://s"));

        let Command::Register(a) = parse(&[
            "register",
            "http://s",
            "-u",
            "ada",
            "-e",
            "ada@example.com",
            "--password-stdin",
        ])
        .command
        else {
            panic!()
        };
        assert_eq!(a.server.as_deref(), Some("http://s"));
        assert_eq!(
            (a.username.as_str(), a.email.as_str()),
            ("ada", "ada@example.com")
        );
        assert!(a.password_stdin);
        assert_eq!(
            parse_err(&["register", "-u", "ada"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn sync() {
        let Command::Push(a) = parse(&["push"]).command else {
            panic!()
        };
        assert!(a.remote.is_none() && a.branch.is_none());
        let Command::Push(a) = parse(&["push", "--remote", "up", "--branch", "dev"]).command else {
            panic!()
        };
        assert_eq!(
            (a.remote.as_deref(), a.branch.as_deref()),
            (Some("up"), Some("dev"))
        );

        let Command::Fetch(a) = parse(&["fetch", "--remote", "up"]).command else {
            panic!()
        };
        assert_eq!(a.remote.as_deref(), Some("up"));

        let Command::Pull(a) = parse(&["pull", "--branch", "dev"]).command else {
            panic!()
        };
        assert_eq!(a.branch.as_deref(), Some("dev"));

        let Command::Clone(a) =
            parse(&["clone", "http://localhost:7420", "acme/web", "dest"]).command
        else {
            panic!()
        };
        assert_eq!(a.url, "http://localhost:7420");
        assert_eq!(a.spec, "acme/web");
        assert_eq!(a.dest, Some(PathBuf::from("dest")));
        assert_eq!(
            parse_err(&["clone", "http://x"]),
            ErrorKind::MissingRequiredArgument
        );

        let Command::Stage(a) = parse(&["stage", "--remote", "up"]).command else {
            panic!()
        };
        assert_eq!(a.remote.as_deref(), Some("up"));
    }

    #[test]
    fn archive() {
        let Command::Archive(a) = parse(&["archive", "-o", "out.zip"]).command else {
            panic!()
        };
        assert_eq!(a.output, PathBuf::from("out.zip"));
        assert!(a.rev.is_none() && a.format.is_none());
        let Command::Archive(a) =
            parse(&["archive", "v1", "--output", "x.bin", "--format", "tar.gz"]).command
        else {
            panic!()
        };
        assert_eq!(a.rev.as_deref(), Some("v1"));
        assert_eq!(a.format, Some(archive::ArchiveFormat::TarGz));
        let Command::Archive(a) = parse(&["archive", "-o", "x", "--format", "zip"]).command else {
            panic!()
        };
        assert_eq!(a.format, Some(archive::ArchiveFormat::Zip));
        assert_eq!(
            parse_err(&["archive", "-o", "x", "--format", "rar"]),
            ErrorKind::InvalidValue
        );
        assert_eq!(parse_err(&["archive"]), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn git() {
        let Command::Git(a) = parse(&["git", "import", "../repo"]).command else {
            panic!()
        };
        // git-export (§13.8): the source may be a URL, so it is a String.
        let git::GitCommand::Import { source, dest, .. } = a.command else {
            panic!()
        };
        assert_eq!((source.as_str(), dest), ("../repo", None));
        let Command::Git(a) = parse(&["git", "import", "../repo", "here"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            git::GitCommand::Import { dest: Some(_), .. }
        ));
        let Command::Git(a) = parse(&["git", "export", "out.git", "--rev", "main"]).command else {
            panic!()
        };
        let git::GitCommand::Export { git_path, rev, .. } = a.command else {
            panic!()
        };
        assert_eq!(
            (git_path, rev.as_deref()),
            (PathBuf::from("out.git"), Some("main"))
        );
    }

    #[test]
    fn daemon() {
        let Command::Daemon(a) = parse(&["daemon", "start"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            daemon::DaemonCommand::Start { foreground: false }
        ));
        let Command::Daemon(a) = parse(&["daemon", "start", "--foreground"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            daemon::DaemonCommand::Start { foreground: true }
        ));
        for (sub, check) in [
            (
                "stop",
                (|c: &daemon::DaemonCommand| matches!(c, daemon::DaemonCommand::Stop))
                    as fn(&_) -> bool,
            ),
            ("status", |c| matches!(c, daemon::DaemonCommand::Status)),
            ("pause", |c| matches!(c, daemon::DaemonCommand::Pause)),
            ("resume", |c| matches!(c, daemon::DaemonCommand::Resume)),
            ("watch", |c| {
                matches!(c, daemon::DaemonCommand::Watch { path: None })
            }),
            ("unwatch", |c| {
                matches!(c, daemon::DaemonCommand::Unwatch { path: None })
            }),
        ] {
            let Command::Daemon(a) = parse(&["daemon", sub]).command else {
                panic!()
            };
            assert!(check(&a.command), "{sub}");
        }
        let Command::Daemon(a) = parse(&["daemon", "watch", "proj"]).command else {
            panic!()
        };
        assert!(
            matches!(a.command, daemon::DaemonCommand::Watch { path: Some(p) } if p == Path::new("proj"))
        );
        let Command::Daemon(a) = parse(&["daemon", "unwatch", "proj"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            daemon::DaemonCommand::Unwatch { path: Some(_) }
        ));
    }

    // daemon workstream (§13.11)
    #[test]
    fn daemon_v05_commands() {
        let Command::Daemon(a) = parse(&["daemon", "restart"]).command else {
            panic!()
        };
        assert!(matches!(a.command, daemon::DaemonCommand::Restart));
        let Command::Daemon(a) = parse(&["daemon", "logs"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            daemon::DaemonCommand::Logs {
                lines: 50,
                follow: false
            }
        ));
        let Command::Daemon(a) = parse(&["daemon", "logs", "-n", "5", "--follow"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            daemon::DaemonCommand::Logs {
                lines: 5,
                follow: true
            }
        ));
        let Command::Daemon(a) = parse(&["daemon", "snapshot"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            daemon::DaemonCommand::Snapshot {
                path: None,
                all: false
            }
        ));
        let Command::Daemon(a) = parse(&["daemon", "snapshot", "--all"]).command else {
            panic!()
        };
        assert!(matches!(
            a.command,
            daemon::DaemonCommand::Snapshot { all: true, .. }
        ));
        assert_eq!(
            parse_err(&["daemon", "snapshot", "p", "--all"]),
            ErrorKind::ArgumentConflict
        );

        let Command::Init(a) = parse(&["init", "--watch"]).command else {
            panic!()
        };
        assert!(a.watch);
        let Command::Clone(a) = parse(&["clone", "http://x", "a/b", "--watch"]).command else {
            panic!()
        };
        assert!(a.watch);
        let Command::Stage(a) = parse(&["stage", "--discard"]).command else {
            panic!()
        };
        assert!(a.discard);
        let Command::Unstage(a) = parse(&["unstage"]).command else {
            panic!()
        };
        assert_eq!((a.branch, a.all, a.remote), (None, false, None));
        let Command::Unstage(a) = parse(&["unstage", "--branch", "dev", "--remote", "up"]).command
        else {
            panic!()
        };
        assert_eq!(a.branch.as_deref(), Some("dev"));
        assert_eq!(a.remote.as_deref(), Some("up"));
        assert_eq!(
            parse_err(&["unstage", "--all", "--branch", "x"]),
            ErrorKind::ArgumentConflict
        );
    }
}
