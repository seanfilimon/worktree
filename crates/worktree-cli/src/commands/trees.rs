//! `wt trees` — trees on the server: create, settings, branches, tags,
//! history, compare, stats and staged work.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use worktree_sdk::remote::rest::path_segs;

use crate::commands::server_common::{
    bytes, colorize_patch_line, confirm, cwd_origin_tenant, decode, fields, nullable, or_dash,
    render, tenant_path, tree_or_default, when, yes_no, Api, Cell, Table, TreeRef,
};
use crate::commands::tenant::Tenant;
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{plural, short, stat_bar, subject};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct TreesArgs {
    #[command(subcommand)]
    pub command: TreesCommand,
}

#[derive(Debug, Subcommand)]
pub enum TreesCommand {
    /// List trees (default: the origin tenant, else every tenant you can see)
    List {
        /// Only trees of this tenant
        #[arg(long, value_name = "T")]
        tenant: Option<String>,
    },
    /// Create a tree
    Create {
        /// TENANT/NAME of the new tree
        #[arg(id = "tree_ref", value_name = "T/NAME")]
        tree: TreeRef,
        #[arg(long, value_name = "D")]
        description: Option<String>,
        /// Who can read the tree (server default: private)
        #[arg(long, value_name = "V")]
        visibility: Option<Visibility>,
        /// Name of the default branch (server default: main)
        #[arg(long, value_name = "B")]
        default_branch: Option<String>,
    },
    /// Show a tree and its counters
    Show {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Change a tree's settings
    Update {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        #[arg(long, value_name = "D")]
        description: Option<String>,
        #[arg(long, value_name = "V")]
        visibility: Option<Visibility>,
        #[arg(long, value_name = "B")]
        default_branch: Option<String>,
    },
    /// Delete a tree and everything in it
    Delete {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: TreeRef,
        /// Do not ask for confirmation
        #[arg(long)]
        yes: bool,
    },
    /// List a tree's branches
    Branches {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Create a branch on the server
    BranchCreate {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: TreeRef,
        name: String,
        /// Start point: snapshot id, branch or tag (default: the default branch tip)
        #[arg(long, value_name = "REV")]
        from: Option<String>,
    },
    /// Delete a branch on the server
    BranchDelete {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: TreeRef,
        name: String,
        /// Do not ask for confirmation
        #[arg(long)]
        yes: bool,
    },
    /// List a tree's tags
    Tags {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Create a tag on the server
    TagCreate {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: TreeRef,
        name: String,
        /// Snapshot id, branch or tag to tag
        #[arg(long, value_name = "REV")]
        target: String,
        /// Tag message
        #[arg(short, long, value_name = "MSG")]
        message: Option<String>,
    },
    /// Delete a tag on the server
    TagDelete {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: TreeRef,
        name: String,
        /// Do not ask for confirmation
        #[arg(long)]
        yes: bool,
    },
    /// Show snapshot history of a branch (newest first)
    Log {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Branch (default: the default branch)
        #[arg(long, value_name = "B")]
        branch: Option<String>,
        /// Number of snapshots (server default: 50)
        #[arg(short = 'n', value_name = "N")]
        max_count: Option<u32>,
    },
    /// Compare two revisions (snapshot ids, branches or tags)
    Compare {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Base revision (default: the first parent of --to)
        #[arg(long, value_name = "REV")]
        from: Option<String>,
        /// Revision to compare
        #[arg(long, value_name = "REV")]
        to: String,
        /// Show a diffstat
        #[arg(long, conflicts_with = "patch")]
        stat: bool,
        /// Show the unified diff
        #[arg(long)]
        patch: bool,
    },
    /// Show contributors, activity and languages of a branch
    Stats {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Activity window in days (1-365, server default: 30)
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=365))]
        days: Option<u32>,
    },
    /// Show the snapshots that changed a file
    FileLog {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: TreeRef,
        path: String,
    },
    /// Show the team's staged (work-in-progress) snapshots
    Staged {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Config-as-code: show the configuration applied from the tree's
    /// .wt-tree/ files, or validate them
    #[command(subcommand)]
    Config(crate::commands::config_as_code::ConfigCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Private,
    Internal,
    Public,
}

/// `domain.Tree`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Tree {
    pub name: String,
    pub description: String,
    pub default_branch: String,
    pub visibility: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TreeSummary {
    tree: Tree,
    branch_count: u64,
    snapshot_count: u64,
    staged_count: u64,
    open_mr_count: u64,
}

/// `domain.Branch`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Branch {
    pub name: String,
    pub tip: String,
    pub protected: bool,
    pub updated_at: String,
    /// config-as-code (§16.3).
    pub description: String,
    pub owner: String,
}

/// `domain.Snapshot`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub id: String,
    pub author: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Tag {
    name: String,
    target: String,
    message: String,
    tagger: String,
    created_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Staged {
    author: String,
    branch_name: String,
    files_added: u32,
    files_modified: u32,
    files_deleted: u32,
    message: String,
    staged_at: String,
}

/// `CompareResult` of §11.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CompareResult {
    pub from: Option<Snapshot>,
    pub to: Snapshot,
    pub merge_base: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub snapshots: Vec<Snapshot>,
    #[serde(deserialize_with = "nullable")]
    pub files: Vec<FileDiff>,
    pub stats: CompareStats,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CompareStats {
    pub files: usize,
    pub additions: usize,
    pub deletions: usize,
}

/// `FileDiff` of §11.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct FileDiff {
    pub path: String,
    pub status: String,
    pub binary: bool,
    pub too_large: bool,
    pub additions: usize,
    pub deletions: usize,
    pub patch: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileHistoryEntry {
    snapshot: Snapshot,
    change: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TreeStats {
    branch: String,
    snapshot_count: u64,
    #[serde(deserialize_with = "nullable")]
    contributors: Vec<Contributor>,
    #[serde(deserialize_with = "nullable")]
    activity: Vec<ActivityDay>,
    file_count: u64,
    total_bytes: u64,
    #[serde(deserialize_with = "nullable")]
    languages: Vec<LanguageShare>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Contributor {
    author: String,
    snapshots: u64,
    last_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ActivityDay {
    snapshots: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LanguageShare {
    language: String,
    files: u64,
    bytes: u64,
}

/// How `wt trees compare` / `wt mr show --diff` print file changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareView {
    /// Snapshot list plus one line per file.
    Summary,
    /// Diffstat bars.
    Stat,
    /// Colored unified diffs.
    Patch,
}

pub fn run(ctx: &Context, args: TreesArgs) -> Result<()> {
    // config-as-code (§16.3): local validation needs no server connection.
    let command = match args.command {
        TreesCommand::Config(c) => return crate::commands::config_as_code::run(ctx, c),
        other => other,
    };
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match command {
        TreesCommand::List { tenant } => list(ctx, &api, tenant),
        TreesCommand::Create {
            tree,
            description,
            visibility,
            default_branch,
        } => {
            let body = json!({
                "name": tree.name,
                "description": description.unwrap_or_default(),
                "visibility": visibility,
                "default_branch": default_branch.unwrap_or_default(),
            });
            let value = api.post(&format!("{}/trees", tenant_path(&tree.tenant)), &body)?;
            render(out, &value, |t, created: Tree| {
                t.line(format_args!(
                    "Created tree {} ({}, default branch {})",
                    tree.to_string().green().bold(),
                    created.visibility,
                    created.default_branch
                ));
            })
        }
        TreesCommand::Show { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&tree.api())?;
            render(out, &value, |t, summary: TreeSummary| {
                show(t, &tree, &summary);
            })
        }
        TreesCommand::Update {
            tree,
            description,
            visibility,
            default_branch,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let mut body = Map::new();
            if let Some(d) = description {
                body.insert("description".into(), d.into());
            }
            if let Some(v) = visibility {
                body.insert("visibility".into(), json!(v));
            }
            if let Some(b) = default_branch {
                body.insert("default_branch".into(), b.into());
            }
            if body.is_empty() {
                return Err(CliError::usage("nothing to change")
                    .with_hint("pass --description, --visibility and/or --default-branch"));
            }
            let value = api.patch(&tree.api(), &body)?;
            render(out, &value, |t, updated: Tree| {
                t.line(format_args!(
                    "Updated {}: {}, default branch {}",
                    tree.to_string().bold(),
                    updated.visibility,
                    updated.default_branch
                ));
            })
        }
        TreesCommand::Delete { tree, yes } => {
            confirm(yes, &format!("delete tree {tree} and all of its history"))?;
            api.delete(&tree.api(), &[])?;
            out.emit(&json!({ "tree": tree.to_string(), "deleted": true }), |t| {
                t.line(format_args!("Deleted tree {}", tree.to_string().bold()));
            })
        }
        TreesCommand::Branches { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&format!("{}/branches", tree.api()))?;
            render(out, &value, |t, branches: Vec<Branch>| {
                let mut table = Table::new(&[
                    "BRANCH",
                    "TIP",
                    "PROTECTED",
                    "OWNER",
                    "UPDATED",
                    "DESCRIPTION",
                ]);
                for b in &branches {
                    let protected = Cell::from(yes_no(b.protected));
                    table.row([
                        Cell::from(b.name.as_str()).green(),
                        Cell::from(or_dash(short(&b.tip))).yellow(),
                        if b.protected {
                            protected.red()
                        } else {
                            protected
                        },
                        or_dash(&b.owner).into(), // config-as-code (§16.3)
                        when(&b.updated_at).into(),
                        subject(&b.description).into(),
                    ]);
                }
                table.render(t, "No branches.");
            })
        }
        TreesCommand::BranchCreate { tree, name, from } => {
            let body = json!({ "name": name, "from": from.unwrap_or_default() });
            let value = api.post(&format!("{}/branches", tree.api()), &body)?;
            render(out, &value, |t, branch: Branch| {
                t.line(format_args!(
                    "Created branch {} at {} on {tree}",
                    branch.name.green().bold(),
                    or_dash(short(&branch.tip)).yellow()
                ));
            })
        }
        TreesCommand::BranchDelete { tree, name, yes } => {
            confirm(yes, &format!("delete branch {name} of {tree}"))?;
            api.delete(
                &format!("{}/branches/{}", tree.api(), path_segs(&name)),
                &[],
            )?;
            out.emit(
                &json!({ "tree": tree.to_string(), "branch": name, "deleted": true }),
                |t| {
                    t.line(format_args!("Deleted branch {} of {tree}", name.bold()));
                },
            )
        }
        TreesCommand::Tags { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&format!("{}/tags", tree.api()))?;
            render(out, &value, |t, tags: Vec<Tag>| {
                let mut table = Table::new(&["TAG", "TARGET", "TAGGER", "CREATED", "MESSAGE"]);
                for tag in &tags {
                    table.row([
                        Cell::from(tag.name.as_str()).yellow().bold(),
                        Cell::from(short(&tag.target)).yellow(),
                        or_dash(&tag.tagger).into(),
                        when(&tag.created_at).into(),
                        subject(&tag.message).into(),
                    ]);
                }
                table.render(t, "No tags.");
            })
        }
        TreesCommand::TagCreate {
            tree,
            name,
            target,
            message,
        } => {
            let body =
                json!({ "name": name, "target": target, "message": message.unwrap_or_default() });
            let value = api.post(&format!("{}/tags", tree.api()), &body)?;
            render(out, &value, |t, tag: Tag| {
                t.line(format_args!(
                    "Tagged {} as {} on {tree}",
                    short(&tag.target).yellow(),
                    tag.name.bold()
                ));
            })
        }
        TreesCommand::TagDelete { tree, name, yes } => {
            confirm(yes, &format!("delete tag {name} of {tree}"))?;
            api.delete(&format!("{}/tags/{}", tree.api(), path_segs(&name)), &[])?;
            out.emit(
                &json!({ "tree": tree.to_string(), "tag": name, "deleted": true }),
                |t| {
                    t.line(format_args!("Deleted tag {} of {tree}", name.bold()));
                },
            )
        }
        TreesCommand::Log {
            tree,
            branch,
            max_count,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let query = [
                ("branch", branch.unwrap_or_default()),
                (
                    "limit",
                    max_count.map(|n| n.to_string()).unwrap_or_default(),
                ),
            ];
            let value = api.get_query(&format!("{}/snapshots", tree.api()), &query)?;
            render(out, &value, |t, snapshots: Vec<Snapshot>| {
                snapshot_table(t, &snapshots);
            })
        }
        TreesCommand::Compare {
            tree,
            from,
            to,
            stat,
            patch,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = compare(&api, &tree, from.as_deref(), &to)?;
            let view = if patch {
                CompareView::Patch
            } else if stat {
                CompareView::Stat
            } else {
                CompareView::Summary
            };
            render(out, &value, |t, cmp: CompareResult| {
                render_compare(t, &cmp, view);
            })
        }
        TreesCommand::Stats { tree, days } => {
            let tree = tree_or_default(ctx, tree)?;
            let query = [("days", days.map(|d| d.to_string()).unwrap_or_default())];
            let value = api.get_query(&format!("{}/stats", tree.api()), &query)?;
            render(out, &value, |t, stats: TreeStats| {
                render_stats(t, &tree, &stats);
            })
        }
        TreesCommand::FileLog { tree, path } => {
            let value = api.get_query(&format!("{}/history", tree.api()), &[("path", path)])?;
            render(out, &value, |t, entries: Vec<FileHistoryEntry>| {
                let mut table = Table::new(&["SNAPSHOT", "CHANGE", "AUTHOR", "WHEN", "MESSAGE"]);
                for e in &entries {
                    table.row([
                        Cell::from(short(&e.snapshot.id)).yellow(),
                        change_cell(&e.change),
                        e.snapshot.author.as_str().into(),
                        when(&e.snapshot.created_at).into(),
                        subject(&e.snapshot.message).into(),
                    ]);
                }
                table.render(t, "No snapshots changed this path.");
            })
        }
        TreesCommand::Staged { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&format!("{}/staged", tree.api()))?;
            render(out, &value, |t, staged: Vec<Staged>| {
                let mut table = Table::new(&["AUTHOR", "BRANCH", "FILES", "STAGED", "MESSAGE"]);
                for s in &staged {
                    table.row([
                        Cell::from(s.author.as_str()).bold(),
                        Cell::from(s.branch_name.as_str()).green(),
                        format!(
                            "+{} ~{} -{}",
                            s.files_added, s.files_modified, s.files_deleted
                        )
                        .into(),
                        when(&s.staged_at).into(),
                        subject(&s.message).into(),
                    ]);
                }
                table.render(t, "Nothing staged.");
            })
        }
        TreesCommand::Config(_) => unreachable!("handled before connecting"),
    }
}

/// `GET {tree}/compare?from=&to=`.
pub fn compare(api: &Api, tree: &TreeRef, from: Option<&str>, to: &str) -> Result<Value> {
    let query = [
        ("to", to.to_string()),
        ("from", from.unwrap_or_default().to_string()),
    ];
    api.get_query(&format!("{}/compare", tree.api()), &query)
}

fn list(ctx: &Context, api: &Api, tenant: Option<String>) -> Result<()> {
    let slugs = match tenant.or_else(|| cwd_origin_tenant(ctx)) {
        Some(slug) => vec![slug],
        None => decode::<Vec<Tenant>>(&api.get("/api/v1/tenants")?)?
            .into_iter()
            .map(|t| t.slug)
            .collect(),
    };
    let mut raw = Vec::new();
    let mut rows = Vec::new();
    for slug in slugs {
        let value = api.get(&format!("{}/trees", tenant_path(&slug)))?;
        let summaries: Vec<TreeSummary> = decode(&value)?;
        if let Value::Array(items) = value {
            raw.extend(items);
        }
        rows.extend(summaries.into_iter().map(|s| (slug.clone(), s)));
    }
    ctx.out.emit(&raw, |t| {
        let mut table = Table::new(&[
            "TREE",
            "VISIBILITY",
            "BRANCHES",
            "SNAPSHOTS",
            "OPEN MRS",
            "UPDATED",
            "DESCRIPTION",
        ]);
        for (slug, s) in &rows {
            table.row([
                Cell::from(format!("{slug}/{}", s.tree.name)).bold(),
                s.tree.visibility.as_str().into(),
                s.branch_count.to_string().into(),
                s.snapshot_count.to_string().into(),
                s.open_mr_count.to_string().into(),
                when(&s.tree.updated_at).into(),
                subject(&s.tree.description).into(),
            ]);
        }
        table.render(t, "No trees.");
    })
}

fn show(t: &mut Text, tree: &TreeRef, s: &TreeSummary) {
    t.line(format_args!(
        "{}  ({})",
        tree.to_string().bold(),
        s.tree.visibility
    ));
    if !s.tree.description.is_empty() {
        t.line(format_args!("  {}", s.tree.description));
    }
    fields(
        t,
        &[
            ("default branch", s.tree.default_branch.clone()),
            ("branches", s.branch_count.to_string()),
            ("snapshots", s.snapshot_count.to_string()),
            ("staged", s.staged_count.to_string()),
            ("open MRs", s.open_mr_count.to_string()),
            ("created", when(&s.tree.created_at)),
            ("updated", when(&s.tree.updated_at)),
        ],
    );
}

/// SNAPSHOT AUTHOR WHEN MESSAGE.
fn snapshot_table(t: &mut Text, snapshots: &[Snapshot]) {
    let mut table = Table::new(&["SNAPSHOT", "AUTHOR", "WHEN", "MESSAGE"]);
    for s in snapshots {
        table.row([
            Cell::from(short(&s.id)).yellow(),
            s.author.as_str().into(),
            when(&s.created_at).into(),
            subject(&s.message).into(),
        ]);
    }
    table.render(t, "No snapshots.");
}

fn change_cell(status: &str) -> Cell {
    let cell = Cell::from(status);
    match status {
        "added" => cell.green(),
        "deleted" => cell.red(),
        _ => cell.yellow(),
    }
}

/// Print a compare result in the requested view.
pub fn render_compare(t: &mut Text, cmp: &CompareResult, view: CompareView) {
    let from = cmp
        .from
        .as_ref()
        .map_or("(nothing)", |s| short(&s.id))
        .to_string();
    let mut head = format!(
        "Comparing {} .. {}",
        from.yellow(),
        short(&cmp.to.id).yellow()
    );
    if let Some(base) = &cmp.merge_base {
        head.push_str(&format!(" (merge base {})", short(base).yellow()));
    }
    t.line(head);
    match view {
        CompareView::Summary => {
            if !cmp.snapshots.is_empty() {
                t.line(plural(cmp.snapshots.len(), "snapshot:", "snapshots:"));
                for s in &cmp.snapshots {
                    t.line(format_args!(
                        "  {} {}  {}",
                        short(&s.id).yellow(),
                        subject(&s.message),
                        format!("{}, {}", s.author, when(&s.created_at)).dimmed()
                    ));
                }
            }
            t.line(summary_line(&cmp.stats));
            let mut table = Table::new(&["CHANGE", "PATH", "+", "-"]);
            for f in &cmp.files {
                table.row([
                    change_cell(&f.status),
                    f.path.as_str().into(),
                    Cell::from(f.additions.to_string()).green(),
                    Cell::from(f.deletions.to_string()).red(),
                ]);
            }
            if !cmp.files.is_empty() {
                table.render(t, "");
            }
        }
        CompareView::Stat => render_stat(t, &cmp.files),
        CompareView::Patch => render_patches(t, &cmp.files),
    }
}

/// "3 files changed, 10 insertions(+), 2 deletions(-)".
fn summary_line(stats: &CompareStats) -> String {
    format!(
        "{} changed, {}(+), {}(-)",
        plural(stats.files, "file", "files"),
        plural(stats.additions, "insertion", "insertions"),
        plural(stats.deletions, "deletion", "deletions")
    )
}

/// Maximum width of the `+++---` bar in `--stat` output.
const STAT_BAR_WIDTH: usize = 40;

fn render_stat(t: &mut Text, files: &[FileDiff]) {
    let path_width = files
        .iter()
        .map(|f| f.path.chars().count())
        .max()
        .unwrap_or(0);
    let max_total = files
        .iter()
        .map(|f| f.additions + f.deletions)
        .max()
        .unwrap_or(0);
    let count_width = max_total.to_string().len().max(3);
    let mut stats = CompareStats::default();
    for f in files {
        stats.files += 1;
        stats.additions += f.additions;
        stats.deletions += f.deletions;
        if f.binary {
            t.line(format_args!(
                " {:<path_width$} | {:>count_width$}",
                f.path, "Bin"
            ));
            continue;
        }
        let (plus, minus) = stat_bar(f.additions, f.deletions, max_total, STAT_BAR_WIDTH);
        t.line(format_args!(
            " {:<path_width$} | {:>count_width$} {}{}",
            f.path,
            f.additions + f.deletions,
            "+".repeat(plus).green(),
            "-".repeat(minus).red()
        ));
    }
    t.line(format_args!(" {}", summary_line(&stats)));
}

fn render_patches(t: &mut Text, files: &[FileDiff]) {
    for f in files {
        t.line(format!("diff --wt a/{0} b/{0}", f.path).bold());
        match f.status.as_str() {
            "added" => t.line("new file".bold()),
            "deleted" => t.line("deleted file".bold()),
            _ => {}
        }
        if f.binary {
            t.line(format_args!("Binary files a/{0} and b/{0} differ", f.path));
        } else if f.too_large {
            t.line("(diff omitted: file too large)".dimmed());
        } else if let Some(patch) = &f.patch {
            for line in patch.lines() {
                t.line(colorize_patch_line(line));
            }
        } else {
            t.line("(no diff available)".dimmed());
        }
    }
}

fn render_stats(t: &mut Text, tree: &TreeRef, s: &TreeStats) {
    t.line(format_args!(
        "{} on {}: {}, {} ({})",
        tree.to_string().bold(),
        s.branch.green(),
        plural(s.snapshot_count as usize, "snapshot", "snapshots"),
        plural(s.file_count as usize, "file", "files"),
        bytes(s.total_bytes)
    ));
    let recent: u64 = s.activity.iter().map(|d| d.snapshots).sum();
    t.line(format_args!(
        "Activity (last {}): {}  {}",
        plural(s.activity.len(), "day", "days"),
        sparkline(&s.activity.iter().map(|d| d.snapshots).collect::<Vec<_>>()).cyan(),
        plural(recent as usize, "snapshot", "snapshots")
    ));
    t.blank();
    let mut table = Table::new(&["CONTRIBUTOR", "SNAPSHOTS", "LAST"]);
    for c in &s.contributors {
        table.row([
            Cell::from(c.author.as_str()).bold(),
            c.snapshots.to_string().into(),
            when(&c.last_at).into(),
        ]);
    }
    table.render(t, "No contributors.");
    t.blank();
    let mut table = Table::new(&["LANGUAGE", "FILES", "SIZE"]);
    for l in &s.languages {
        table.row([
            l.language.as_str().into(),
            l.files.to_string().into(),
            bytes(l.bytes).into(),
        ]);
    }
    table.render(t, "No files.");
}

/// One block character per value, scaled to the maximum.
fn sparkline(values: &[u64]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().copied().max().unwrap_or(0);
    values
        .iter()
        .map(|&v| {
            if max == 0 {
                BARS[0]
            } else {
                BARS[((v * 7).div_ceil(max)) as usize]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn trees(args: &[&str]) -> TreesCommand {
        let mut full = vec!["trees"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Trees(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    fn acme_web() -> TreeRef {
        "acme/web".parse().expect("tree ref")
    }

    #[test]
    fn parses_list_create_show_update_delete() {
        assert!(matches!(
            trees(&["list"]),
            TreesCommand::List { tenant: None }
        ));
        assert!(matches!(
            trees(&["list", "--tenant", "acme"]),
            TreesCommand::List { tenant: Some(t) } if t == "acme"
        ));
        let TreesCommand::Create {
            tree,
            description,
            visibility,
            default_branch,
        } = trees(&[
            "create",
            "acme/web",
            "--description",
            "Website",
            "--visibility",
            "public",
            "--default-branch",
            "trunk",
        ])
        else {
            panic!()
        };
        assert_eq!(tree, acme_web());
        assert_eq!(description.as_deref(), Some("Website"));
        assert_eq!(visibility, Some(Visibility::Public));
        assert_eq!(default_branch.as_deref(), Some("trunk"));
        assert!(matches!(
            trees(&["show"]),
            TreesCommand::Show { tree: None }
        ));
        assert!(matches!(
            trees(&["show", "acme/web"]),
            TreesCommand::Show { tree: Some(t) } if t == acme_web()
        ));
        assert!(matches!(
            trees(&["update", "--visibility", "internal"]),
            TreesCommand::Update {
                tree: None,
                visibility: Some(Visibility::Internal),
                ..
            }
        ));
        assert!(matches!(
            trees(&["delete", "acme/web", "--yes"]),
            TreesCommand::Delete { yes: true, .. }
        ));
        assert_eq!(
            parse_err(&["trees", "create", "web"]),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            parse_err(&["trees", "create", "acme/web", "--visibility", "secret"]),
            ErrorKind::InvalidValue
        );
        assert_eq!(
            parse_err(&["trees", "delete"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn parses_branches_and_tags() {
        assert!(matches!(
            trees(&["branches"]),
            TreesCommand::Branches { tree: None }
        ));
        assert!(matches!(
            trees(&["branch-create", "acme/web", "feat", "--from", "main"]),
            TreesCommand::BranchCreate { name, from: Some(f), .. } if name == "feat" && f == "main"
        ));
        assert!(matches!(
            trees(&["branch-delete", "acme/web", "feat"]),
            TreesCommand::BranchDelete { yes: false, .. }
        ));
        assert!(matches!(
            trees(&["tags", "acme/web"]),
            TreesCommand::Tags { tree: Some(_) }
        ));
        assert!(matches!(
            trees(&["tag-create", "acme/web", "v1", "--target", "main", "-m", "first"]),
            TreesCommand::TagCreate { target, message: Some(m), .. } if target == "main" && m == "first"
        ));
        assert_eq!(
            parse_err(&["trees", "tag-create", "acme/web", "v1"]),
            ErrorKind::MissingRequiredArgument
        );
        assert!(matches!(
            trees(&["tag-delete", "acme/web", "v1", "--yes"]),
            TreesCommand::TagDelete { yes: true, .. }
        ));
    }

    #[test]
    fn parses_history_commands() {
        assert!(matches!(
            trees(&["log", "acme/web", "--branch", "dev", "-n", "5"]),
            TreesCommand::Log { branch: Some(b), max_count: Some(5), .. } if b == "dev"
        ));
        assert!(matches!(
            trees(&["compare", "--to", "feat", "--patch"]),
            TreesCommand::Compare {
                tree: None,
                from: None,
                patch: true,
                stat: false,
                ..
            }
        ));
        assert!(matches!(
            trees(&["compare", "acme/web", "--from", "main", "--to", "feat", "--stat"]),
            TreesCommand::Compare {
                from: Some(_),
                stat: true,
                ..
            }
        ));
        assert_eq!(
            parse_err(&["trees", "compare", "--to", "x", "--stat", "--patch"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["trees", "compare", "acme/web"]),
            ErrorKind::MissingRequiredArgument
        );
        assert!(matches!(
            trees(&["stats", "--days", "7"]),
            TreesCommand::Stats { days: Some(7), .. }
        ));
        assert_eq!(
            parse_err(&["trees", "stats", "--days", "0"]),
            ErrorKind::ValueValidation
        );
        assert!(matches!(
            trees(&["file-log", "acme/web", "src/lib.rs"]),
            TreesCommand::FileLog { path, .. } if path == "src/lib.rs"
        ));
        assert!(matches!(
            trees(&["staged"]),
            TreesCommand::Staged { tree: None }
        ));
    }

    fn sample_compare() -> CompareResult {
        serde_json::from_value(json!({
            "from": {"id": "aaaaaaaa1111", "author": "ada", "message": "base", "created_at": ""},
            "to": {"id": "bbbbbbbb2222", "author": "ada", "message": "next", "created_at": ""},
            "snapshots": [{"id": "bbbbbbbb2222", "author": "ada", "message": "next\nbody", "created_at": ""}],
            "files": [
                {"path": "a.txt", "status": "modified", "binary": false, "too_large": false,
                 "additions": 1, "deletions": 1, "patch": "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n"},
                {"path": "img.png", "status": "added", "binary": true, "too_large": false,
                 "additions": 0, "deletions": 0}
            ],
            "stats": {"files": 2, "additions": 1, "deletions": 1}
        }))
        .expect("compare result")
    }

    #[test]
    fn renders_compare_views() {
        colored::control::set_override(false);
        let cmp = sample_compare();

        let mut t = Text::default();
        render_compare(&mut t, &cmp, CompareView::Summary);
        let text = t.as_str();
        assert!(
            text.starts_with("Comparing aaaaaaaa .. bbbbbbbb\n"),
            "{text}"
        );
        assert!(
            text.contains("1 snapshot:\n  bbbbbbbb next  ada, -\n"),
            "{text}"
        );
        assert!(
            text.contains("2 files changed, 1 insertion(+), 1 deletion(-)"),
            "{text}"
        );
        assert!(text.contains("modified  a.txt    1  1"), "{text}");

        let mut t = Text::default();
        render_compare(&mut t, &cmp, CompareView::Stat);
        assert!(t.as_str().contains(" a.txt   |   2 +-\n"), "{}", t.as_str());
        assert!(t.as_str().contains(" img.png | Bin\n"), "{}", t.as_str());

        let mut t = Text::default();
        render_compare(&mut t, &cmp, CompareView::Patch);
        let text = t.as_str();
        assert!(
            text.contains("diff --wt a/a.txt b/a.txt\n--- a/a.txt\n"),
            "{text}"
        );
        assert!(text.contains("-old\n+new\n"), "{text}");
        assert!(
            text.contains("new file\nBinary files a/img.png and b/img.png differ"),
            "{text}"
        );
    }

    #[test]
    fn root_compares_have_no_base() {
        colored::control::set_override(false);
        let cmp: CompareResult = serde_json::from_value(json!({
            "from": null, "to": {"id": "cccccccc"}, "snapshots": null, "files": null,
            "stats": {"files": 0, "additions": 0, "deletions": 0}
        }))
        .expect("root compare");
        let mut t = Text::default();
        render_compare(&mut t, &cmp, CompareView::Summary);
        assert_eq!(
            t.as_str(),
            "Comparing (nothing) .. cccccccc\n0 files changed, 0 insertions(+), 0 deletions(-)\n"
        );
    }

    #[test]
    fn sparklines() {
        assert_eq!(sparkline(&[]), "");
        assert_eq!(sparkline(&[0, 0]), "▁▁");
        assert_eq!(sparkline(&[0, 1, 2, 4]), "▁▃▅█");
    }

    #[test]
    fn visibility_serializes_lowercase() {
        assert_eq!(json!(Visibility::Internal), json!("internal"));
        assert_eq!(json!(None::<Visibility>), Value::Null);
    }
}
