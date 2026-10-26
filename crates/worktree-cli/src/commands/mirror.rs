//! `wt mirror` — live Git push mirrors of a server tree: list, add, show,
//! update, remove and sync now (ENGINEERING_PLAN §15.6).
//!
//! The server exports the tree's snapshots into Git commits (deterministic,
//! license-aware) and pushes the mirrored branches and tags to the remote
//! after every push, and on `wt mirror sync`. Every subcommand needs
//! `tree:admin` and takes `--tree TENANT/TREE` (default: the tree of the
//! current worktree).
//!
//! Credentials for http(s) remotes are read from an environment variable
//! (`--token-env VAR`) so they never appear in shell history or `ps`.

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::server_common::{
    confirm, decode, fields, nullable, or_dash, render, tree_or_default, when, yes_no, Api, Cell,
    Table, TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct MirrorArgs {
    #[command(subcommand)]
    pub command: MirrorCommand,
}

/// Which tree a command addresses.
#[derive(Debug, Clone, Default, Args)]
pub struct Scope {
    /// Mirrors of this server tree (TENANT/TREE; default: the current worktree's tree)
    // Shares the id of the global `--tree` so it replaces that flag here.
    #[arg(long, value_name = "TENANT/TREE", value_parser = tree_spec)]
    pub tree: Option<String>,
}

fn tree_spec(spec: &str) -> std::result::Result<String, String> {
    spec.parse::<TreeRef>().map(|_| spec.to_string())
}

#[derive(Debug, Subcommand)]
pub enum MirrorCommand {
    /// List the Git mirrors of a tree with their last sync status
    List {
        #[command(flatten)]
        scope: Scope,
    },
    /// Add a Git mirror (https://, http://, ssh:// or user@host:path remote)
    Add {
        /// Git remote URL (no password in it: use --token-env)
        url: String,
        /// Mirror only branches matching this glob (`*` within a segment,
        /// `**` across segments; repeatable; default: every branch)
        #[arg(long = "branch", short = 'b', value_name = "GLOB")]
        branches: Vec<String>,
        /// Do not mirror tags
        #[arg(long)]
        no_tags: bool,
        /// Read the remote credential (a token or `user:password`) from this
        /// environment variable (http(s) remotes only)
        #[arg(long, value_name = "VAR")]
        token_env: Option<String>,
        /// Create it paused
        #[arg(long)]
        inactive: bool,
        #[command(flatten)]
        scope: Scope,
    },
    /// Show a mirror
    Show {
        id: String,
        #[command(flatten)]
        scope: Scope,
    },
    /// Change a mirror (only the given fields)
    Update {
        id: String,
        /// New remote URL (forgets what was pushed to the old one)
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        /// Replace the branch globs (repeatable)
        #[arg(
            long = "branch",
            short = 'b',
            value_name = "GLOB",
            conflicts_with = "all_branches"
        )]
        branches: Vec<String>,
        /// Mirror every branch
        #[arg(long)]
        all_branches: bool,
        /// Mirror tags
        #[arg(long, conflicts_with = "no_tags")]
        tags: bool,
        /// Stop mirroring tags
        #[arg(long)]
        no_tags: bool,
        /// Replace the credential with the value of this environment variable
        #[arg(long, value_name = "VAR", conflicts_with = "clear_token")]
        token_env: Option<String>,
        /// Remove the credential
        #[arg(long)]
        clear_token: bool,
        /// Resume syncing
        #[arg(long, conflicts_with = "inactive")]
        active: bool,
        /// Pause syncing
        #[arg(long)]
        inactive: bool,
        #[command(flatten)]
        scope: Scope,
    },
    /// Remove a mirror (the remote repository is left as it is)
    Remove {
        id: String,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
        #[command(flatten)]
        scope: Scope,
    },
    /// Export and push a mirror now (exits 1 when the sync fails)
    Sync {
        id: String,
        /// Start the sync on the server and return at once
        #[arg(long)]
        no_wait: bool,
        #[command(flatten)]
        scope: Scope,
    },
}

/// `domain.Mirror`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Mirror {
    id: String,
    url: String,
    #[serde(deserialize_with = "nullable")]
    branches: Vec<String>,
    tags: bool,
    active: bool,
    has_credential: bool,
    created_by: String,
    created_at: String,
    last_sync_at: Option<String>,
    last_status: String,
    last_error: String,
}

/// `mirror.GitStatus`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GitStatus {
    available: bool,
    version: String,
    error: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MirrorList {
    #[serde(deserialize_with = "nullable")]
    mirrors: Vec<Mirror>,
    git: Option<GitStatus>,
}

/// `mirror.RefUpdate`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RefUpdate {
    #[serde(rename = "ref")]
    name: String,
    new: String,
    status: String,
    reason: String,
}

/// `mirror.SyncResult`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SyncResult {
    commits: u64,
    #[serde(deserialize_with = "nullable")]
    refs: Vec<RefUpdate>,
    pushed: bool,
    #[serde(deserialize_with = "nullable")]
    excluded: Vec<String>,
    #[serde(deserialize_with = "nullable")]
    skipped: Vec<String>,
    error: String,
    duration_ms: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SyncResponse {
    mirror: Mirror,
    result: Option<SyncResult>,
}

fn base(tree: &TreeRef) -> String {
    format!("{}/mirrors", tree.api())
}

fn item(tree: &TreeRef, id: &str) -> String {
    format!("{}/{}", base(tree), seg(id))
}

fn resolve(ctx: &Context, scope: &Scope) -> Result<TreeRef> {
    let tree = match &scope.tree {
        Some(spec) => Some(spec.parse::<TreeRef>().map_err(CliError::usage)?),
        None => None,
    };
    tree_or_default(ctx, tree)
}

/// The credential in environment variable `var`.
fn token_from_env(var: &str) -> Result<String> {
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => Ok(v.trim().to_string()),
        _ => Err(
            CliError::usage(format!("environment variable {var} is not set or empty")).with_hint(
                format!("export {var}=<token> (or user:password) before running wt mirror"),
            ),
        ),
    }
}

pub fn run(ctx: &Context, args: MirrorArgs) -> Result<()> {
    let out = &ctx.out;
    match args.command {
        MirrorCommand::List { scope } => {
            let tree = resolve(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            let value = api.get(&base(&tree))?;
            render(out, &value, |t, list: MirrorList| {
                list_mirrors(t, &list, &tree)
            })
        }
        MirrorCommand::Add {
            url,
            branches,
            no_tags,
            token_env,
            inactive,
            scope,
        } => {
            let credential = token_env.as_deref().map(token_from_env).transpose()?;
            let tree = resolve(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            let body = create_body(url, branches, no_tags, credential, inactive);
            let value = api.post(&base(&tree), &body)?;
            render(out, &value, |t, m: Mirror| {
                t.line(format_args!(
                    "Added mirror {} of {} -> {}",
                    m.id.green().bold(),
                    tree,
                    m.url
                ));
                if m.active {
                    t.line(
                        "  The first sync starts in a few seconds; `wt mirror sync ID` runs it now."
                            .dimmed(),
                    );
                }
            })
        }
        MirrorCommand::Show { id, scope } => {
            let tree = resolve(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            let value = api.get(&item(&tree, &id))?;
            render(out, &value, |t, m: Mirror| show_mirror(t, &m, &tree))
        }
        MirrorCommand::Update {
            id,
            url,
            branches,
            all_branches,
            tags,
            no_tags,
            token_env,
            clear_token,
            active,
            inactive,
            scope,
        } => {
            let credential = token_env.as_deref().map(token_from_env).transpose()?;
            let body = update_body(UpdateFlags {
                url,
                branches,
                all_branches,
                tags,
                no_tags,
                credential,
                clear_token,
                active,
                inactive,
            });
            if body.is_empty() {
                return Err(CliError::usage("nothing to change").with_hint(
                    "pass --url, --branch, --all-branches, --tags, --no-tags, --token-env, --clear-token, --active or --inactive",
                ));
            }
            let tree = resolve(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            let value = api.patch(&item(&tree, &id), &Value::Object(body))?;
            render(out, &value, |t, m: Mirror| {
                t.line(format_args!("Updated mirror {}", m.id.bold()));
            })
        }
        MirrorCommand::Remove { id, yes, scope } => {
            let tree = resolve(ctx, &scope)?;
            confirm(yes, &format!("remove mirror {id} of {tree}"))?;
            let api = Api::connect(ctx)?;
            api.delete(&item(&tree, &id), &[])?;
            out.emit(&json!({ "deleted": id }), |t| {
                t.line(format_args!("Removed mirror {}", id.bold()));
            })
        }
        MirrorCommand::Sync { id, no_wait, scope } => {
            let tree = resolve(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            let path = format!("{}/sync", item(&tree, &id));
            let value = if no_wait {
                api.post(&format!("{path}?wait=false"), &json!({}))?
            } else {
                api.post(&path, &json!({}))?
            };
            let resp: SyncResponse = decode(&value)?;
            out.emit(&value, |t| sync_summary(t, &resp))?;
            match &resp.result {
                Some(r) if !r.error.is_empty() => Err(CliError::failure(format!(
                    "mirror {} failed to sync: {}",
                    resp.mirror.id, r.error
                ))),
                _ => Ok(()),
            }
        }
    }
}

/// The POST body of `wt mirror add`.
fn create_body(
    url: String,
    branches: Vec<String>,
    no_tags: bool,
    credential: Option<String>,
    inactive: bool,
) -> Value {
    let mut body = Map::new();
    body.insert("url".into(), url.trim().into());
    let branches = clean(branches);
    if !branches.is_empty() {
        body.insert("branches".into(), branches.into());
    }
    body.insert("tags".into(), (!no_tags).into());
    if let Some(c) = credential {
        body.insert("credential".into(), c.into());
    }
    body.insert("active".into(), (!inactive).into());
    Value::Object(body)
}

/// Flags of `wt mirror update`.
#[derive(Debug, Default)]
struct UpdateFlags {
    url: Option<String>,
    branches: Vec<String>,
    all_branches: bool,
    tags: bool,
    no_tags: bool,
    credential: Option<String>,
    clear_token: bool,
    active: bool,
    inactive: bool,
}

/// The PATCH body of `wt mirror update` (only the given fields).
fn update_body(f: UpdateFlags) -> Map<String, Value> {
    let mut body = Map::new();
    if let Some(url) = f.url {
        body.insert("url".into(), url.trim().into());
    }
    let branches = clean(f.branches);
    if f.all_branches {
        body.insert("branches".into(), json!([]));
    } else if !branches.is_empty() {
        body.insert("branches".into(), branches.into());
    }
    if f.tags || f.no_tags {
        body.insert("tags".into(), f.tags.into());
    }
    if let Some(c) = f.credential {
        body.insert("credential".into(), c.into());
    } else if f.clear_token {
        body.insert("credential".into(), "".into());
    }
    if f.active || f.inactive {
        body.insert("active".into(), f.active.into());
    }
    body
}

fn clean(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn status_cell(m: &Mirror) -> Cell {
    if !m.active {
        return Cell::from("paused").yellow();
    }
    match m.last_status.as_str() {
        "ok" => Cell::from("ok").green(),
        "running" => Cell::from("running").cyan(),
        "error" => Cell::from("error").red(),
        "disabled" => Cell::from("disabled").red(),
        _ => Cell::from("pending").cyan(),
    }
}

fn branches_text(m: &Mirror) -> String {
    if m.branches.is_empty() {
        "all".into()
    } else {
        m.branches.join(",")
    }
}

fn last_sync(m: &Mirror) -> String {
    m.last_sync_at
        .as_deref()
        .map_or_else(|| "never".to_string(), when)
}

fn list_mirrors(t: &mut Text, list: &MirrorList, tree: &TreeRef) {
    if let Some(git) = &list.git {
        if !git.available {
            t.line(format_args!(
                "{} {}",
                "Mirrors are disabled on this server:".red().bold(),
                git.error
            ));
            t.blank();
        }
    }
    let mut table = Table::new(&["ID", "URL", "BRANCHES", "TAGS", "STATUS", "LAST SYNC"]);
    for m in &list.mirrors {
        table.row([
            Cell::from(m.id.as_str()).bold(),
            m.url.as_str().into(),
            branches_text(m).into(),
            yes_no(m.tags).into(),
            status_cell(m),
            last_sync(m).into(),
        ]);
    }
    table.render(t, &format!("No mirrors on tree {tree}."));
    for m in &list.mirrors {
        if !m.last_error.is_empty() {
            t.line(format_args!("{} {}: {}", "!".red(), m.id, m.last_error));
        }
    }
}

fn show_mirror(t: &mut Text, m: &Mirror, tree: &TreeRef) {
    t.line(format_args!(
        "{}  [{}]",
        m.id.bold(),
        status_cell(m).colored()
    ));
    fields(
        t,
        &[
            ("tree", tree.to_string()),
            ("url", m.url.clone()),
            ("branches", branches_text(m)),
            ("tags", yes_no(m.tags).to_string()),
            ("credential", yes_no(m.has_credential).to_string()),
            ("active", yes_no(m.active).to_string()),
            ("created by", or_dash(&m.created_by)),
            ("created", when(&m.created_at)),
            ("last sync", last_sync(m)),
            ("last error", or_dash(&m.last_error)),
        ],
    );
}

fn sync_summary(t: &mut Text, resp: &SyncResponse) {
    let m = &resp.mirror;
    let Some(r) = &resp.result else {
        t.line(format_args!(
            "Started syncing mirror {} -> {}",
            m.id.bold(),
            m.url
        ));
        return;
    };
    if r.error.is_empty() {
        t.line(format_args!(
            "{} Synced mirror {} -> {}: {} new commit{}, {} ref{} in {} ms",
            "✓".green(),
            m.id.bold(),
            m.url,
            r.commits,
            if r.commits == 1 { "" } else { "s" },
            r.refs.len(),
            if r.refs.len() == 1 { "" } else { "s" },
            r.duration_ms
        ));
    } else {
        t.line(format_args!(
            "{} Mirror {} -> {} failed: {}",
            "✗".red(),
            m.id.bold(),
            m.url,
            r.error
        ));
    }
    let mut table = Table::new(&["REF", "STATUS", "OBJECT", "REASON"]);
    for u in &r.refs {
        let status = match u.status.as_str() {
            "rejected" | "error" => Cell::from(u.status.as_str()).red(),
            "up_to_date" => Cell::from("up to date"),
            other => Cell::from(other).green(),
        };
        let short: String = u.new.chars().take(12).collect();
        table.row([
            u.name.as_str().into(),
            status,
            or_dash(&short).into(),
            or_dash(&u.reason).into(),
        ]);
    }
    if !r.refs.is_empty() {
        t.blank();
        table.render(t, "");
    }
    if !r.excluded.is_empty() {
        t.blank();
        t.line(format_args!(
            "{} path{} not mirrored (license forbids redistribution): {}",
            r.excluded.len(),
            if r.excluded.len() == 1 { "" } else { "s" },
            r.excluded.join(", ")
        ));
    }
    if !r.skipped.is_empty() {
        t.line(format_args!(
            "Skipped (not representable in Git): {}",
            r.skipped.join(", ")
        ));
    }
    if !r.pushed && r.error.is_empty() {
        t.line("Nothing to push.".dimmed());
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn mirror(args: &[&str]) -> MirrorCommand {
        let mut full = vec!["mirror"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Mirror(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        let MirrorCommand::List { scope } = mirror(&["list", "--tree", "acme/web"]) else {
            panic!()
        };
        assert_eq!(scope.tree.as_deref(), Some("acme/web"));
        assert_eq!(
            parse_err(&["mirror", "list", "--tree", "nope"]),
            ErrorKind::ValueValidation
        );
        let MirrorCommand::Add {
            url,
            branches,
            no_tags,
            token_env,
            inactive,
            ..
        } = mirror(&[
            "add",
            "https://github.com/acme/web.git",
            "--branch",
            "main",
            "-b",
            "release/*",
            "--no-tags",
            "--token-env",
            "GH_TOKEN",
        ])
        else {
            panic!()
        };
        assert_eq!(url, "https://github.com/acme/web.git");
        assert_eq!(branches, ["main", "release/*"]);
        assert!(no_tags && !inactive);
        assert_eq!(token_env.as_deref(), Some("GH_TOKEN"));
        assert_eq!(
            parse_err(&["mirror", "add"]),
            ErrorKind::MissingRequiredArgument
        );
        assert!(matches!(
            mirror(&["show", "m1"]),
            MirrorCommand::Show { .. }
        ));
        assert!(matches!(
            mirror(&[
                "update",
                "m1",
                "--all-branches",
                "--inactive",
                "--clear-token"
            ]),
            MirrorCommand::Update {
                all_branches: true,
                inactive: true,
                clear_token: true,
                ..
            }
        ));
        assert_eq!(
            parse_err(&["mirror", "update", "m1", "--tags", "--no-tags"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["mirror", "update", "m1", "-b", "x", "--all-branches"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&[
                "mirror",
                "update",
                "m1",
                "--token-env",
                "X",
                "--clear-token"
            ]),
            ErrorKind::ArgumentConflict
        );
        assert!(matches!(
            mirror(&["remove", "m1", "-y"]),
            MirrorCommand::Remove { yes: true, .. }
        ));
        assert!(matches!(
            mirror(&["sync", "m1", "--no-wait"]),
            MirrorCommand::Sync { no_wait: true, .. }
        ));
    }

    #[test]
    fn paths_and_bodies() {
        let tree: TreeRef = "acme/my web".parse().expect("tree");
        assert_eq!(base(&tree), "/api/v1/trees/acme/my%20web/mirrors");
        assert_eq!(
            item(&tree, "m/1"),
            "/api/v1/trees/acme/my%20web/mirrors/m%2F1"
        );
        assert_eq!(
            create_body(
                " https://x/r.git ".into(),
                vec![" main ".into(), "".into()],
                true,
                Some("tok".into()),
                false
            ),
            json!({"url": "https://x/r.git", "branches": ["main"], "tags": false, "credential": "tok", "active": true})
        );
        assert_eq!(
            create_body("https://x".into(), vec![], false, None, true),
            json!({"url": "https://x", "tags": true, "active": false})
        );
        assert_eq!(
            Value::Object(update_body(UpdateFlags {
                all_branches: true,
                no_tags: true,
                clear_token: true,
                active: true,
                ..Default::default()
            })),
            json!({"branches": [], "tags": false, "credential": "", "active": true})
        );
        assert_eq!(
            Value::Object(update_body(UpdateFlags {
                url: Some("ssh://git@x/r".into()),
                branches: vec!["dev".into()],
                credential: Some("c".into()),
                ..Default::default()
            })),
            json!({"url": "ssh://git@x/r", "branches": ["dev"], "credential": "c"})
        );
        assert!(update_body(UpdateFlags::default()).is_empty());
    }

    #[test]
    fn reads_the_token_from_the_environment() {
        std::env::set_var("WT_MIRROR_TEST_TOKEN", " s3cret \n");
        assert_eq!(token_from_env("WT_MIRROR_TEST_TOKEN").unwrap(), "s3cret");
        std::env::remove_var("WT_MIRROR_TEST_TOKEN_MISSING");
        let err = token_from_env("WT_MIRROR_TEST_TOKEN_MISSING").unwrap_err();
        assert!(err.to_string().contains("not set"), "{err}");
    }

    #[test]
    fn renders_mirrors_and_sync_results() {
        colored::control::set_override(false);
        let tree: TreeRef = "acme/web".parse().expect("tree");
        let list: MirrorList = serde_json::from_value(json!({
            "mirrors": [
                {"id": "m1", "url": "https://github.com/acme/web.git", "branches": null, "tags": true,
                 "active": true, "last_status": "error", "last_error": "push rejected",
                 "last_sync_at": "2020-01-01T00:00:00Z"},
                {"id": "m2", "url": "git@host:x.git", "branches": ["main", "rel/*"], "tags": false, "active": false}
            ],
            "git": {"available": false, "error": "git binary \"git\" not found"}
        }))
        .expect("list");
        let mut t = Text::default();
        list_mirrors(&mut t, &list, &tree);
        let s = t.as_str();
        assert!(
            s.starts_with("Mirrors are disabled on this server: git binary"),
            "{s}"
        );
        assert!(
            s.contains("m1  https://github.com/acme/web.git  all"),
            "{s}"
        );
        assert!(s.contains("error"), "{s}");
        assert!(s.contains("m2  git@host:x.git"), "{s}");
        assert!(s.contains("main,rel/*  no    paused  never"), "{s}");
        assert!(s.contains("! m1: push rejected"), "{s}");
        let mut t = Text::default();
        list_mirrors(&mut t, &MirrorList::default(), &tree);
        assert_eq!(t.as_str(), "No mirrors on tree acme/web.\n");

        let mut t = Text::default();
        show_mirror(&mut t, &list.mirrors[1], &tree);
        assert!(t.as_str().starts_with("m2  [paused]\n"), "{}", t.as_str());
        assert!(t.as_str().contains("main,rel/*"), "{}", t.as_str());
        assert!(t.as_str().contains("credential  no"), "{}", t.as_str());

        let ok: SyncResponse = serde_json::from_value(json!({
            "mirror": {"id": "m1", "url": "file:///r.git"},
            "result": {"commits": 1, "pushed": true, "duration_ms": 5, "excluded": ["secret/key"],
                       "refs": [{"ref": "refs/heads/main", "new": "0123456789abcdef0123", "status": "updated"},
                                {"ref": "refs/tags/v1", "new": "ab", "status": "up_to_date"}]}
        }))
        .expect("sync");
        let mut t = Text::default();
        sync_summary(&mut t, &ok);
        let s = t.as_str();
        assert!(
            s.starts_with("✓ Synced mirror m1 -> file:///r.git: 1 new commit, 2 refs in 5 ms"),
            "{s}"
        );
        assert!(
            s.contains("refs/heads/main  updated     0123456789ab"),
            "{s}"
        );
        assert!(s.contains("up to date"), "{s}");
        assert!(
            s.contains("1 path not mirrored (license forbids redistribution): secret/key"),
            "{s}"
        );

        let failed: SyncResponse = serde_json::from_value(json!({
            "mirror": {"id": "m1", "url": "u"},
            "result": {"commits": 0, "pushed": false, "error": "stale info", "refs": [
                {"ref": "refs/heads/main", "status": "rejected", "reason": "[rejected] (stale info)"}]}
        }))
        .expect("failed");
        let mut t = Text::default();
        sync_summary(&mut t, &failed);
        assert!(
            t.as_str()
                .starts_with("✗ Mirror m1 -> u failed: stale info"),
            "{}",
            t.as_str()
        );
        let started: SyncResponse =
            serde_json::from_value(json!({"mirror": {"id": "m1", "url": "u"}})).expect("started");
        let mut t = Text::default();
        sync_summary(&mut t, &started);
        assert_eq!(t.as_str(), "Started syncing mirror m1 -> u\n");
    }
}
