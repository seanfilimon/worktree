//! `wt fork` — fork a server tree into another tenant, list the forks of a
//! tree, show a fork's upstream and sync a fork with it
//! (ENGINEERING_PLAN §16.2, specs/tree/Forks.md).
//!
//! `wt fork TENANT/TREE` creates the fork on the server (in your personal
//! tenant unless `--into` names another); clone it with `wt clone`. Merge
//! requests from a fork are opened in the upstream with `wt mr create
//! UPSTREAM --from TENANT/TREE:BRANCH`.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::commands::server_common::{
    fields, nullable, or_dash, render, tenant_path, tree_or_default, when, Api, Cell, Table,
    TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::short;
use crate::output::Text;

/// `wt fork TENANT/TREE [--into TENANT] [--name N] [--branch B]...`, or one
/// of the subcommands.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ForkArgs {
    #[command(subcommand)]
    pub command: Option<ForkCommand>,
    #[command(flatten)]
    pub create: CreateArgs,
}

/// Arguments of `wt fork TENANT/TREE`.
#[derive(Debug, Default, Args)]
pub struct CreateArgs {
    /// Tree to fork
    #[arg(id = "source_ref", value_name = "TENANT/TREE")]
    pub source: Option<TreeRef>,
    /// Tenant that receives the fork (default: your personal tenant)
    #[arg(long, value_name = "TENANT")]
    pub into: Option<String>,
    /// Name of the fork (default: the upstream's name)
    #[arg(long, value_name = "N")]
    pub name: Option<String>,
    /// Copy only this branch (repeatable; default: every branch). Tags are
    /// always copied
    #[arg(long = "branch", short = 'b', value_name = "B")]
    pub branches: Vec<String>,
    /// Visibility of the fork (default: the upstream's; never wider)
    #[arg(long, value_name = "V")]
    pub visibility: Option<Visibility>,
    /// Description of the fork (default: the upstream's)
    #[arg(long, value_name = "D")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Visibility {
    Private,
    Internal,
    Public,
}

impl Visibility {
    fn as_str(self) -> &'static str {
        match self {
            Visibility::Private => "private",
            Visibility::Internal => "internal",
            Visibility::Public => "public",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ForkCommand {
    /// List the forks of a tree that you may read
    List {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Fast-forward a fork's branches to its upstream
    ///
    /// Without --branch every branch the fork and its upstream both have is
    /// synced; a --branch that exists only upstream is created. Diverged
    /// branches are reported (exit 1): merge the upstream locally and push.
    Sync {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Sync only this branch (repeatable)
        #[arg(long = "branch", short = 'b', value_name = "B")]
        branches: Vec<String>,
    },
    /// Show the upstream of a fork and how each branch relates to it
    Upstream {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
}

/// The source of a merge request from a fork: `TENANT/TREE:BRANCH`
/// (`wt mr create --from`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkSource {
    pub tree: TreeRef,
    pub branch: String,
}

impl std::str::FromStr for ForkSource {
    type Err = String;

    fn from_str(spec: &str) -> std::result::Result<Self, Self::Err> {
        let bad = || format!("'{spec}' is not of the form TENANT/TREE:BRANCH");
        let (tree, branch) = spec.split_once(':').ok_or_else(bad)?;
        if branch.is_empty() {
            return Err(bad());
        }
        Ok(Self {
            tree: tree.parse().map_err(|_| bad())?,
            branch: branch.to_string(),
        })
    }
}

impl std::fmt::Display for ForkSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.tree, self.branch)
    }
}

impl ForkSource {
    /// Add `source_tenant` / `source_tree` to a `POST /merge-requests` body.
    pub fn add_to(&self, body: &mut Value) {
        body["source_tenant"] = json!(self.tree.tenant);
        body["source_tree"] = json!(self.tree.name);
    }
}

/// The source tip of merge request `number` of `tree` as a snapshot of
/// `tree` (`GET .../merge-requests/{n}/source`): what diffs of a merge
/// request from a fork compare.
pub fn target_tip(api: &Api, tree: &TreeRef, number: u32) -> Result<String> {
    let value = api.get(&format!("{}/merge-requests/{number}/source", tree.api()))?;
    let src: MrSource = crate::commands::server_common::decode(&value)?;
    if src.missing || src.target_tip.is_empty() {
        return Err(CliError::failure(format!(
            "the source branch '{}' of merge request !{number} no longer exists in its fork",
            src.branch
        )));
    }
    Ok(src.target_tip)
}

/// `review.MRSource`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MrSource {
    branch: String,
    target_tip: String,
    missing: bool,
}

/// `domain.Tree`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Tree {
    name: String,
    tenant_id: String,
    visibility: String,
    default_branch: String,
    fork_of: String,
}

/// `fork.Fork`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Fork {
    tenant: String,
    name: String,
    visibility: String,
    #[serde(deserialize_with = "nullable")]
    branches: Vec<String>,
    created_by: String,
    created_at: String,
    last_sync_at: Option<String>,
}

/// `fork.Upstream`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Upstream {
    fork: bool,
    upstream_tree_id: String,
    upstream_missing: bool,
    upstream_hidden: bool,
    tenant: String,
    name: String,
    default_branch: String,
    forked_by: String,
    forked_at: Option<String>,
    last_sync_at: Option<String>,
    #[serde(deserialize_with = "nullable")]
    branches: Vec<BranchStatus>,
}

/// `fork.BranchStatus`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BranchStatus {
    name: String,
    tip: String,
    upstream_tip: String,
    status: String,
}

/// `fork.SyncResult`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SyncResult {
    #[serde(deserialize_with = "nullable")]
    branches: Vec<SyncBranch>,
    updated: u64,
    #[serde(deserialize_with = "nullable")]
    diverged: Vec<String>,
}

/// `fork.SyncBranch`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SyncBranch {
    name: String,
    result: String,
    old_tip: String,
    new_tip: String,
}

pub fn run(ctx: &Context, args: ForkArgs) -> Result<()> {
    match args.command {
        Some(ForkCommand::List { tree }) => list(ctx, tree),
        Some(ForkCommand::Sync { tree, branches }) => sync(ctx, tree, branches),
        Some(ForkCommand::Upstream { tree }) => upstream(ctx, tree),
        None => create(ctx, args.create),
    }
}

/// The POST body of `wt fork TENANT/TREE`.
fn create_body(args: &CreateArgs) -> Value {
    let mut body = Map::new();
    let mut set = |key: &str, value: Option<&str>| {
        if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
            body.insert(key.into(), v.into());
        }
    };
    set("tenant", args.into.as_deref());
    set("name", args.name.as_deref());
    set("description", args.description.as_deref());
    set("visibility", args.visibility.map(Visibility::as_str));
    let branches: Vec<&str> = args
        .branches
        .iter()
        .map(|b| b.trim())
        .filter(|b| !b.is_empty())
        .collect();
    if !branches.is_empty() {
        body.insert("branches".into(), json!(branches));
    }
    Value::Object(body)
}

fn create(ctx: &Context, args: CreateArgs) -> Result<()> {
    let Some(source) = args.source.clone() else {
        return Err(CliError::usage("missing the tree to fork").with_hint(
            "wt fork TENANT/TREE [--into TENANT] [--name N] [--branch B]...; see also `wt fork list|sync|upstream`",
        ));
    };
    let api = Api::connect(ctx)?;
    let value = api.post(&format!("{}/forks", source.api()), &create_body(&args))?;
    let created: Tree = crate::commands::server_common::decode(&value)?;
    // The response names the tenant by id: look its slug up for the message.
    let tenant = match &args.into {
        Some(t) => t.trim().to_string(),
        None if !ctx.out.is_json() => api
            .get(&tenant_path(&created.tenant_id))
            .ok()
            .and_then(|v| v.get("slug").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default(),
        None => String::new(),
    };
    render(&ctx.out, &value, |t, tree: Tree| {
        let into = if tenant.is_empty() {
            tree.name.clone()
        } else {
            format!("{tenant}/{}", tree.name)
        };
        t.line(format_args!(
            "Forked {source} into {} ({}, default branch {})",
            into.green().bold(),
            tree.visibility,
            tree.default_branch
        ));
        if !tree.fork_of.is_empty() {
            t.line(
                "  Clone it with `wt clone URL TENANT/TREE DIR`; open merge requests upstream with `wt mr create --from`."
                    .dimmed(),
            );
        }
    })
}

fn list(ctx: &Context, tree: Option<TreeRef>) -> Result<()> {
    let tree = tree_or_default(ctx, tree)?;
    let api = Api::connect(ctx)?;
    let value = api.get(&format!("{}/forks", tree.api()))?;
    render(&ctx.out, &value, |t, forks: Vec<Fork>| {
        list_forks(t, &forks, &tree)
    })
}

fn list_forks(t: &mut Text, forks: &[Fork], tree: &TreeRef) {
    let mut table = Table::new(&[
        "FORK",
        "VISIBILITY",
        "BRANCHES",
        "BY",
        "CREATED",
        "LAST SYNC",
    ]);
    for f in forks {
        table.row([
            Cell::from(format!("{}/{}", f.tenant, f.name)).bold(),
            f.visibility.as_str().into(),
            f.branches.len().to_string().into(),
            or_dash(&f.created_by).into(),
            when(&f.created_at).into(),
            f.last_sync_at
                .as_deref()
                .map_or_else(|| "-".into(), when)
                .into(),
        ]);
    }
    table.render(t, &format!("No forks of {tree} that you can see."));
}

fn status_cell(status: &str) -> Cell {
    let cell = Cell::from(status.replace('_', " "));
    match status {
        "up_to_date" => cell.green(),
        "behind" => cell.yellow(),
        "diverged" => cell.red(),
        "ahead" => cell.cyan(),
        _ => cell,
    }
}

fn upstream(ctx: &Context, tree: Option<TreeRef>) -> Result<()> {
    let tree = tree_or_default(ctx, tree)?;
    let api = Api::connect(ctx)?;
    let value = api.get(&format!("{}/fork", tree.api()))?;
    render(&ctx.out, &value, |t, u: Upstream| {
        show_upstream(t, &u, &tree)
    })
}

fn show_upstream(t: &mut Text, u: &Upstream, tree: &TreeRef) {
    if !u.fork {
        t.line(format_args!("{tree} is not a fork."));
        return;
    }
    if u.upstream_missing {
        t.line(format_args!(
            "{tree} is a fork of a tree that was deleted ({}); its history is intact.",
            u.upstream_tree_id
        ));
        return;
    }
    if u.upstream_hidden {
        t.line(format_args!(
            "{tree} is a fork of tree {} (you cannot read it).",
            u.upstream_tree_id
        ));
        return;
    }
    t.line(format_args!(
        "{tree} is a fork of {}",
        format!("{}/{}", u.tenant, u.name).bold()
    ));
    fields(
        t,
        &[
            ("forked by", or_dash(&u.forked_by)),
            (
                "forked",
                u.forked_at.as_deref().map_or_else(|| "-".into(), when),
            ),
            (
                "last sync",
                u.last_sync_at.as_deref().map_or_else(|| "-".into(), when),
            ),
            ("upstream default", or_dash(&u.default_branch)),
        ],
    );
    t.blank();
    let mut table = Table::new(&["BRANCH", "STATUS", "FORK", "UPSTREAM"]);
    for b in &u.branches {
        table.row([
            Cell::from(b.name.as_str()).bold(),
            status_cell(&b.status),
            or_dash(short(&b.tip)).into(),
            or_dash(short(&b.upstream_tip)).into(),
        ]);
    }
    table.render(t, "No branches.");
    if u.branches.iter().any(|b| b.status == "behind") {
        t.line("  `wt fork sync` fast-forwards the branches that are behind.".dimmed());
    }
}

fn sync(ctx: &Context, tree: Option<TreeRef>, branches: Vec<String>) -> Result<()> {
    let tree = tree_or_default(ctx, tree)?;
    let api = Api::connect(ctx)?;
    let body = if branches.is_empty() {
        json!({})
    } else {
        json!({ "branches": branches })
    };
    let value = api.post(&format!("{}/fork/sync", tree.api()), &body)?;
    let res: SyncResult = crate::commands::server_common::decode(&value)?;
    ctx.out.emit(&value, |t| sync_summary(t, &res, &tree))?;
    if res.diverged.is_empty() {
        Ok(())
    } else {
        Err(CliError::failure(format!(
            "{} diverged from the upstream: {}",
            if res.diverged.len() == 1 {
                "a branch has"
            } else {
                "branches have"
            },
            res.diverged.join(", ")
        ))
        .with_hint("merge the upstream branch locally (wt fetch + wt merge) and push"))
    }
}

fn sync_summary(t: &mut Text, res: &SyncResult, tree: &TreeRef) {
    let mut table = Table::new(&["BRANCH", "RESULT", "FROM", "TO"]);
    for b in &res.branches {
        let result = Cell::from(b.result.replace('_', " "));
        let result = match b.result.as_str() {
            "fast_forwarded" | "created" => result.green(),
            "diverged" | "moved" => result.red(),
            _ => result,
        };
        table.row([
            Cell::from(b.name.as_str()).bold(),
            result,
            or_dash(short(&b.old_tip)).into(),
            or_dash(short(&b.new_tip)).into(),
        ]);
    }
    table.render(t, "No branches to sync.");
    t.line(format_args!(
        "Synced {tree}: {} branch{} updated",
        res.updated,
        if res.updated == 1 { "" } else { "es" }
    ));
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn fork(args: &[&str]) -> ForkArgs {
        let mut full = vec!["fork"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Fork(a) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_create_and_subcommands() {
        let a = fork(&[
            "acme/web",
            "--into",
            "bob",
            "--name",
            "web2",
            "-b",
            "main",
            "--branch",
            "dev",
            "--visibility",
            "private",
        ]);
        assert!(a.command.is_none());
        assert_eq!(
            a.create.source.as_ref().map(|t| t.to_string()),
            Some("acme/web".into())
        );
        assert_eq!(a.create.into.as_deref(), Some("bob"));
        assert_eq!(a.create.branches, ["main", "dev"]);
        assert_eq!(
            create_body(&a.create),
            json!({"tenant": "bob", "name": "web2", "visibility": "private", "branches": ["main", "dev"]})
        );
        assert_eq!(create_body(&fork(&["acme/web"]).create), json!({}));

        assert!(matches!(
            fork(&["list"]).command,
            Some(ForkCommand::List { tree: None })
        ));
        assert!(matches!(
            fork(&["list", "acme/web"]).command,
            Some(ForkCommand::List { tree: Some(_) })
        ));
        let Some(ForkCommand::Sync { tree, branches }) =
            fork(&["sync", "bob/web", "-b", "main"]).command
        else {
            panic!()
        };
        assert_eq!(tree.map(|t| t.to_string()), Some("bob/web".into()));
        assert_eq!(branches, ["main"]);
        assert!(matches!(
            fork(&["upstream"]).command,
            Some(ForkCommand::Upstream { tree: None })
        ));
        assert_eq!(parse_err(&["fork", "nope"]), ErrorKind::ValueValidation);
        assert_eq!(
            parse_err(&["fork", "acme/web", "--visibility", "secret"]),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn parses_fork_sources() {
        let s: ForkSource = "bob/web:feat/x".parse().expect("parse");
        assert_eq!(s.tree.to_string(), "bob/web");
        assert_eq!(s.branch, "feat/x");
        assert_eq!(s.to_string(), "bob/web:feat/x");
        let mut body = json!({"title": "t"});
        s.add_to(&mut body);
        assert_eq!(
            body,
            json!({"title": "t", "source_tenant": "bob", "source_tree": "web"})
        );
        for bad in ["bob/web", "bob/web:", "bob:feat", "a/b/c:x", ":x"] {
            assert!(bad.parse::<ForkSource>().is_err(), "{bad:?}");
        }
        // `wt mr create --from` replaces --source.
        let Command::Mr(m) = parse(&[
            "mr",
            "create",
            "acme/web",
            "--from",
            "bob/web:feat",
            "--target",
            "main",
            "--title",
            "T",
        ]) else {
            panic!()
        };
        let crate::commands::mr::MrCommand::Create { source, from, .. } = m.command else {
            panic!()
        };
        assert!(source.is_none());
        assert_eq!(from.map(|f| f.to_string()), Some("bob/web:feat".into()));
        assert_eq!(
            parse_err(&[
                "mr",
                "create",
                "--from",
                "bob/web:feat",
                "--source",
                "x",
                "--target",
                "main",
                "--title",
                "T"
            ]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["mr", "create", "--target", "main", "--title", "T"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn renders_upstream_and_sync() {
        colored::control::set_override(false);
        let tree: TreeRef = "bob/web".parse().expect("tree");
        let u: Upstream = serde_json::from_value(json!({
            "fork": true, "upstream_tree_id": "t1", "upstream_missing": false, "upstream_hidden": false,
            "tenant": "acme", "name": "web", "default_branch": "main", "forked_by": "bob",
            "branches": [
                {"name": "main", "tip": "aaaaaaaaaaaa", "upstream_tip": "bbbbbbbbbbbb", "status": "behind"},
                {"name": "dev", "tip": "", "upstream_tip": "cccccccccccc", "status": "upstream_only"}
            ]
        }))
        .expect("upstream");
        let mut t = Text::default();
        show_upstream(&mut t, &u, &tree);
        let s = t.as_str();
        assert!(s.contains("bob/web is a fork of acme/web"), "{s}");
        assert!(s.contains("behind") && s.contains("upstream only"), "{s}");
        assert!(s.contains("wt fork sync"), "{s}");

        let mut t = Text::default();
        let missing = Upstream {
            fork: true,
            upstream_missing: true,
            upstream_tree_id: "gone".into(),
            ..Upstream::default()
        };
        show_upstream(&mut t, &missing, &tree);
        assert!(t.as_str().contains("was deleted"), "{}", t.as_str());
        let mut t = Text::default();
        show_upstream(&mut t, &Upstream::default(), &tree);
        assert_eq!(t.as_str(), "bob/web is not a fork.\n");

        let res: SyncResult = serde_json::from_value(json!({
            "branches": [{"name": "main", "result": "fast_forwarded", "old_tip": "a1", "new_tip": "b2"}],
            "updated": 1, "diverged": []
        }))
        .expect("sync");
        let mut t = Text::default();
        sync_summary(&mut t, &res, &tree);
        assert!(t.as_str().contains("fast forwarded"), "{}", t.as_str());
        assert!(
            t.as_str().ends_with("Synced bob/web: 1 branch updated\n"),
            "{}",
            t.as_str()
        );

        let forks: Vec<Fork> = serde_json::from_value(json!([
            {"tenant": "bob", "name": "web", "visibility": "internal", "branches": ["main"], "created_by": "bob", "created_at": ""}
        ]))
        .expect("forks");
        let mut t = Text::default();
        list_forks(&mut t, &forks, &tree);
        assert!(
            t.as_str().contains("bob/web") && t.as_str().contains("internal"),
            "{}",
            t.as_str()
        );
        let mut t = Text::default();
        list_forks(&mut t, &[], &tree);
        assert!(t.as_str().contains("No forks"), "{}", t.as_str());
    }
}
