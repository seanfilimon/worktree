//! `wt git import|export|push|remote` over `worktree_git`.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_git::bridge::{
    export_repository, import_into_tree, import_repository, push_repository, ExportOptions,
    ImportOptions, ImportReport, PushRepoOptions,
};
use worktree_git::remote::auth::GitAuth;
use worktree_git::remote::transport::GitTransport;
use worktree_git::source::{self, GitSource};
use worktree_sdk::config::GitRemoteSection;
use worktree_sdk::{Repository, SdkError};

use crate::commands::archive::{license_policy, warn_restricted, LicenseFlags, RuleSource};
use crate::commands::server_common::{Cell, Table};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::plural;

#[derive(Debug, Args)]
pub struct GitArgs {
    #[command(subcommand)]
    pub command: GitCommand,
}

#[derive(Debug, Subcommand)]
pub enum GitCommand {
    /// Import a Git repository (URL or path) with its branches and tags
    ///
    /// A URL (https://, ssh://, user@host:path, file://) is cloned into a
    /// temporary directory first; a path is read in place. Without `--tree`
    /// a new worktree is created at DEST (default: ./<repository name>).
    /// Inside a worktree, `--tree NAME` imports into that tree instead
    /// (creating it as a nested tree at DEST or ./NAME if it does not
    /// exist); importing again adds only new commits and fast-forwards
    /// branches.
    Import {
        /// Git repository URL or path
        #[arg(value_name = "URL|PATH")]
        source: String,
        /// Destination directory
        dest: Option<PathBuf>,
        /// Import only these branches (comma-separated); a name without a
        /// local branch selects the remote-tracking branch of that name
        #[arg(
            long,
            value_name = "A,B",
            value_delimiter = ',',
            conflicts_with = "all_branches"
        )]
        branches: Vec<String>,
        /// Also import remote-tracking branches (refs/remotes/*) of a local
        /// repository (default: local branches only)
        #[arg(long)]
        all_branches: bool,
    },
    /// Export this tree's history into a Git repository (created if missing)
    Export {
        /// Git repository to write; a missing path ending in `.git` is created bare
        git_path: PathBuf,
        /// Branch to export (default: all branches)
        #[arg(long, value_name = "REV")]
        rev: Option<String>,
        #[command(flatten)]
        license: LicenseFlags,
    },
    /// Export this tree and push it to a Git remote
    ///
    /// TARGET is a remote added with `wt git remote add`, a URL or a path
    /// (a missing local path is created as a bare repository). The remote's
    /// history is fetched first so imported commits are reused and pushes
    /// fast-forward. Tags are pushed too.
    Push {
        /// Git remote name, URL or path
        #[arg(value_name = "NAME|URL|PATH")]
        target: String,
        /// Branch to push (repeatable; default: all branches)
        #[arg(long = "branch", short = 'b', value_name = "B")]
        branches: Vec<String>,
        /// Overwrite remote branches even when the push is not a fast-forward
        #[arg(long, short = 'f')]
        force: bool,
        #[command(flatten)]
        license: LicenseFlags,
    },
    /// Manage Git remotes (stored in .wt/config.toml [git_remote.NAME])
    Remote {
        #[command(subcommand)]
        command: GitRemoteCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum GitRemoteCommand {
    /// Add a Git remote (local paths are stored absolute)
    Add { name: String, url: String },
    /// List Git remotes
    List,
    /// Remove a Git remote
    Remove { name: String },
}

#[derive(Serialize)]
struct ImportResult<'a> {
    dest: &'a Path,
    tree: &'a str,
    branches: &'a [String],
    snapshots: usize,
    blobs: usize,
    tags: usize,
    checked_out: Option<&'a str>,
    /// Git remote recorded for `wt git push` (new worktrees only).
    git_remote: Option<&'a str>,
}

#[derive(Serialize)]
struct ExportResult<'a> {
    git_path: &'a Path,
    branches: &'a [String],
    commits: usize,
    tags: usize,
    restricted: &'a [String],
    license_rules: RuleSource,
}

#[derive(Serialize)]
struct PushResult<'a> {
    url: &'a str,
    branches: &'a [String],
    commits: usize,
    refs: &'a [String],
    restricted: &'a [String],
    license_rules: RuleSource,
}

#[derive(Serialize)]
struct RemoteEntry<'a> {
    name: &'a str,
    url: &'a str,
}

pub fn run(ctx: &Context, args: GitArgs) -> Result<()> {
    match args.command {
        GitCommand::Import {
            source,
            dest,
            branches,
            all_branches,
        } => import(ctx, &source, dest, branches, all_branches),
        GitCommand::Export {
            git_path,
            rev,
            license,
        } => export(ctx, &git_path, rev, &license),
        GitCommand::Push {
            target,
            branches,
            force,
            license,
        } => push(ctx, &target, branches, force, &license),
        GitCommand::Remote { command } => remote(ctx, command),
    }
}

fn import(
    ctx: &Context,
    source_arg: &str,
    dest: Option<PathBuf>,
    branches: Vec<String>,
    all_branches: bool,
) -> Result<()> {
    if source::is_path(source_arg) && !ctx.absolute(Path::new(source_arg)).is_dir() {
        return Err(CliError::failure(format!(
            "no Git repository at '{source_arg}'"
        )));
    }
    let auth = GitAuth::auto();
    let source = GitSource::open(source_arg, ctx.cwd(), &auth)?;
    let opts = ImportOptions {
        branches,
        all_branches,
        name: ctx.tree_flag().map(str::to_owned),
        include_tags: true,
    };

    // `--tree NAME` inside a worktree: import into that tree.
    let existing_repo = match ctx.tree_flag() {
        Some(_) => match Repository::open(ctx.cwd()) {
            Ok(repo) => Some(repo),
            Err(SdkError::NotAWorktree) => None,
            Err(e) => return Err(e.into()),
        },
        None => None,
    };
    let mut recorded = None;
    let report: ImportReport = if let (Some(repo), Some(name)) = (&existing_repo, ctx.tree_flag()) {
        if !repo.trees()?.iter().any(|t| t.name == name) {
            let dir = match &dest {
                Some(dest) => ctx.absolute(dest),
                None => ctx.cwd().join(name),
            };
            let rel = crate::paths::relative_to(repo.root(), ctx.cwd(), &dir)?;
            if rel.is_empty() {
                return Err(CliError::usage(format!(
                    "tree '{name}' does not exist and the worktree root cannot become a new tree"
                ))
                .with_hint("pass a destination directory inside the worktree"));
            }
            repo.add_tree(name, Path::new(&rel))?;
        }
        import_into_tree(repo, name, source.path(), &opts)?
    } else {
        let dest = match dest {
            Some(dest) => ctx.absolute(&dest),
            None => {
                let name = repository_name(source_arg).ok_or_else(|| {
                    CliError::usage(format!("cannot derive a destination from '{source_arg}'"))
                        .with_hint("pass a destination: `wt git import URL|PATH DEST`")
                })?;
                ctx.cwd().join(name)
            }
        };
        let report = import_repository(source.path(), &dest, &opts)?;
        recorded = record_origin(&report.dest, source_arg, ctx)?;
        report
    };
    drop(source);

    let result = ImportResult {
        dest: &report.dest,
        tree: &report.tree,
        branches: &report.branches,
        snapshots: report.snapshots,
        blobs: report.blobs,
        tags: report.tags,
        checked_out: report.checked_out.as_deref(),
        git_remote: recorded.as_deref(),
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Imported {} ({}, {}, {}) into {}",
            plural(report.branches.len(), "branch", "branches"),
            plural(report.snapshots, "snapshot", "snapshots"),
            plural(report.blobs, "blob", "blobs"),
            plural(report.tags, "tag", "tags"),
            report.dest.display()
        ));
        if let Some(branch) = &report.checked_out {
            t.line(format_args!("Checked out {}", branch.green().bold()));
        }
        if let Some(url) = &recorded {
            t.line(format_args!("Added Git remote origin -> {url}"));
        }
    })
}

/// Record the import source as Git remote `origin` of a new worktree.
fn record_origin(dest: &Path, source_arg: &str, ctx: &Context) -> Result<Option<String>> {
    let mut repo = Repository::open(dest)?;
    if repo.config().git_remote.contains_key("origin") {
        return Ok(None);
    }
    let url = normalize_url(ctx, source_arg);
    let mut config = repo.config().clone();
    config
        .git_remote
        .insert("origin".into(), GitRemoteSection { url: url.clone() });
    repo.set_config(config)?;
    Ok(Some(url))
}

/// Absolute path for local paths, the URL itself otherwise.
fn normalize_url(ctx: &Context, url: &str) -> String {
    if source::is_path(url) {
        crate::paths::normalize(&ctx.absolute(Path::new(url)))
            .to_string_lossy()
            .into_owned()
    } else {
        url.to_owned()
    }
}

fn export(ctx: &Context, git_path: &Path, rev: Option<String>, flags: &LicenseFlags) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    if let Some(rev) = &rev {
        if !repo
            .branches(&tree.name, true)?
            .iter()
            .any(|b| &b.name == rev)
        {
            return Err(CliError::failure(format!(
                "'{rev}' is not a branch of tree '{}'",
                tree.name
            ))
            .with_hint("`--rev` names the branch to export; list them with `wt branch --all`"));
        }
    }
    let (policy, rule_source) = license_policy(ctx, &repo, &tree, flags)?;
    let target = ctx.absolute(git_path);
    let bare = !target.exists() && target.extension().is_some_and(|ext| ext == "git");
    let opts = ExportOptions {
        tree: Some(tree.name.clone()),
        branches: rev.into_iter().collect(),
        include_tags: true,
        bare,
        license: policy,
    };
    let report = export_repository(&repo, &target, &opts)?;
    warn_restricted(&report.restricted);
    let result = ExportResult {
        git_path: &report.git_path,
        branches: &report.branches,
        commits: report.commits,
        tags: report.tags,
        restricted: &report.restricted,
        license_rules: rule_source,
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Exported {} ({} new, {}) to {}",
            plural(report.branches.len(), "branch", "branches"),
            plural(report.commits, "commit", "commits"),
            plural(report.tags, "tag", "tags"),
            report.git_path.display()
        ));
    })
}

fn push(
    ctx: &Context,
    target: &str,
    branches: Vec<String>,
    force: bool,
    flags: &LicenseFlags,
) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let url = match repo.config().git_remote.get(target) {
        Some(remote) => remote.url.clone(),
        None => {
            if GitTransport::from_url(target).is_none() {
                return Err(CliError::usage(format!(
                    "'{target}' is neither a Git remote nor a supported URL"
                ))
                .with_hint("list Git remotes with `wt git remote list`"));
            }
            normalize_url(ctx, target)
        }
    };
    let (policy, rule_source) = license_policy(ctx, &repo, &tree, flags)?;
    let opts = PushRepoOptions {
        tree: Some(tree.name.clone()),
        branches,
        force,
        license: policy,
    };
    let report = push_repository(&repo, &url, &opts, &GitAuth::auto())?;
    warn_restricted(&report.restricted);
    let result = PushResult {
        url: &report.url,
        branches: &report.branches,
        commits: report.commits,
        refs: &report.refs,
        restricted: &report.restricted,
        license_rules: rule_source,
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Pushed {} ({}) to {}",
            plural(report.branches.len(), "branch", "branches"),
            plural(report.refs.len(), "ref", "refs"),
            report.url
        ));
    })
}

fn remote(ctx: &Context, command: GitRemoteCommand) -> Result<()> {
    let mut repo = ctx.open()?;
    match command {
        GitRemoteCommand::Add { name, url } => {
            validate_remote_name(&name)?;
            if GitTransport::from_url(&url).is_none() {
                return Err(CliError::usage(format!("unsupported Git URL: {url}")));
            }
            if repo.config().git_remote.contains_key(&name) {
                return Err(
                    CliError::failure(format!("Git remote '{name}' already exists")).with_hint(
                        format!("remove it first with `wt git remote remove {name}`"),
                    ),
                );
            }
            let url = normalize_url(ctx, &url);
            let mut config = repo.config().clone();
            config
                .git_remote
                .insert(name.clone(), GitRemoteSection { url: url.clone() });
            repo.set_config(config)?;
            ctx.out.emit(
                &RemoteEntry {
                    name: &name,
                    url: &url,
                },
                |t| {
                    t.line(format_args!("Added Git remote {} -> {url}", name.bold()));
                },
            )
        }
        GitRemoteCommand::List => {
            let config = repo.config();
            let entries: Vec<RemoteEntry<'_>> = config
                .git_remote
                .iter()
                .map(|(name, r)| RemoteEntry { name, url: &r.url })
                .collect();
            ctx.out.emit(&entries, |t| {
                let mut table = Table::new(&["NAME", "URL"]);
                for e in &entries {
                    table.row([Cell::from(e.name).bold(), e.url.into()]);
                }
                table.render(t, "No Git remotes.");
            })
        }
        GitRemoteCommand::Remove { name } => {
            let mut config = repo.config().clone();
            if config.git_remote.remove(&name).is_none() {
                return Err(CliError::failure(format!("no Git remote named '{name}'"))
                    .with_hint("list Git remotes with `wt git remote list`"));
            }
            repo.set_config(config)?;
            ctx.out
                .emit(&serde_json::json!({ "name": name, "removed": true }), |t| {
                    t.line(format_args!("Removed Git remote {}", name.bold()))
                })
        }
    }
}

fn validate_remote_name(name: &str) -> Result<()> {
    let valid = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(CliError::usage(format!(
            "invalid Git remote name '{name}': use letters, digits, '.', '_' and '-'"
        )))
    }
}

/// `../proj` -> `proj`, `../proj.git` -> `proj`, `../proj/.git` -> `proj`,
/// `https://host/org/proj.git` -> `proj`, `git@host:org/proj` -> `proj`.
fn repository_name(source_arg: &str) -> Option<String> {
    let name = if source::is_path(source_arg) {
        let path = crate::paths::normalize(Path::new(source_arg));
        let mut name = path.file_name()?.to_string_lossy().into_owned();
        if name == ".git" {
            name = path.parent()?.file_name()?.to_string_lossy().into_owned();
        }
        name
    } else {
        let trimmed = source_arg.trim_end_matches('/');
        let trimmed = trimmed.strip_suffix("/.git").unwrap_or(trimmed);
        trimmed.rsplit(['/', ':', '\\']).next()?.to_owned()
    };
    let name = name.strip_suffix(".git").unwrap_or(&name);
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_names() {
        for (source, want) in [
            ("/src/proj", "proj"),
            ("/src/proj.git", "proj"),
            ("/src/proj/.git", "proj"),
            ("/src/proj/", "proj"),
            ("/src/proj/sub/..", "proj"),
            ("https://example.com/org/proj.git", "proj"),
            ("https://example.com/org/proj/", "proj"),
            ("git@example.com:org/proj.git", "proj"),
            ("git@example.com:proj", "proj"),
            ("file:///srv/git/proj.git", "proj"),
        ] {
            assert_eq!(repository_name(source).as_deref(), Some(want), "{source}");
        }
    }

    #[test]
    fn remote_names() {
        assert!(validate_remote_name("origin").is_ok());
        assert!(validate_remote_name("git-hub_2.x").is_ok());
        for bad in ["", "-x", ".x", "a b", "a/b"] {
            assert!(validate_remote_name(bad).is_err(), "{bad:?}");
        }
    }
}
