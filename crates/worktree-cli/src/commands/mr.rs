//! `wt mr` — merge requests: list, open, review, approve, merge, close,
//! comments, merge readiness checks and linked groups.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Value};

use worktree_sdk::remote::rest::seg;

use crate::commands::server_common::{
    cwd_origin_tenant, decode, nullable, render, tenant_path, tree_or_default, when, Api, Cell,
    Table, TreeRef,
};
use crate::commands::trees::{compare, render_compare, CompareResult, CompareView};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{plural, short};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct MrArgs {
    #[command(subcommand)]
    pub command: MrCommand,
}

#[derive(Debug, Subcommand)]
pub enum MrCommand {
    /// List merge requests
    List {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Which merge requests to list
        #[arg(long, value_name = "STATE", default_value = "open")]
        state: StateFilter,
    },
    /// Show a merge request
    Show {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Also show the changes it brings (JSON: {"merge_request", "diff"})
        #[arg(long)]
        diff: bool,
    },
    /// Open a merge request
    Create {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Branch with the changes
        #[arg(long, value_name = "B")]
        source: String,
        /// Branch to merge into
        #[arg(long, value_name = "B")]
        target: String,
        #[arg(long, value_name = "T")]
        title: String,
        #[arg(long, value_name = "D")]
        description: Option<String>,
    },
    /// Approve a merge request
    Approve {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Merge a merge request into its target on the server
    ///
    /// Without --method the server fast-forwards when it can and otherwise
    /// writes a merge snapshot (the first method branch protection allows).
    /// A merge with conflicts is refused: merge the target into the source
    /// locally, resolve and push.
    Merge {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// How to merge: move the target (fast-forward), write a merge
        /// snapshot with both tips as parents (merge), or one snapshot with
        /// the combined changes (squash)
        #[arg(long, value_name = "METHOD")]
        method: Option<MergeMethod>,
        /// Message of the merge or squash snapshot (default: "Merge branch
        /// 'SRC' into TGT (!N)" / the merge request title)
        #[arg(short, long, value_name = "MSG")]
        message: Option<String>,
    },
    /// Show which merge methods can merge a merge request, and its conflicts
    Mergeability {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Close a merge request without merging
    Close {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Review a merge request: approve, request changes or comment
    ///
    /// Approvals and change requests apply to the current source tip: when
    /// the source moves, approvals go stale and must be renewed; a change
    /// request blocks merging until the same reviewer approves.
    #[command(group(clap::ArgGroup::new("decision").required(true)
        .args(["approve", "request_changes", "comment"])))]
    Review {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Approve the current source tip
        #[arg(long)]
        approve: bool,
        /// Request changes (blocks merging until you approve)
        #[arg(long)]
        request_changes: bool,
        /// Comment without approving (needs -m)
        #[arg(long, requires = "message")]
        comment: bool,
        /// Review text
        #[arg(short, long, value_name = "TEXT")]
        message: Option<String>,
    },
    /// Comment on a merge request, optionally on a file line
    Comment {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Comment text
        #[arg(short, long, value_name = "TEXT")]
        message: String,
        /// File the comment refers to
        #[arg(long, value_name = "PATH")]
        path: Option<String>,
        /// Line of --path the comment refers to
        #[arg(long, value_name = "N", requires = "path",
              value_parser = clap::value_parser!(u32).range(1..))]
        line: Option<u32>,
    },
    /// List the comments of a merge request
    Comments {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Show whether a merge request can be merged, and what blocks it
    Checks {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Link merge requests of several trees into a group that merges atomically
    Link {
        /// Name of the group
        name: String,
        /// Members as TENANT/TREE#N (at least two)
        #[arg(value_name = "TREE#N", num_args = 2.., required = true)]
        members: Vec<MemberRef>,
        /// Tenant that owns the group (default: the first member's tenant)
        #[arg(long, value_name = "SLUG")]
        tenant: Option<String>,
    },
    /// Merge every merge request of a linked group, all or nothing
    MergeGroup {
        /// Group id (see `wt mr groups`)
        id: String,
        /// Tenant that owns the group (default: this worktree's origin tenant)
        #[arg(long, value_name = "SLUG")]
        tenant: Option<String>,
        /// Merge method for every member (default: each member's default)
        #[arg(long, value_name = "METHOD")]
        method: Option<MergeMethod>,
    },
    /// List the linked groups of a tenant
    Groups {
        /// Tenant (default: this worktree's origin tenant)
        #[arg(long, value_name = "SLUG")]
        tenant: Option<String>,
    },
}

/// A merge request of a linked group: `TENANT/TREE#N`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRef {
    pub tree: TreeRef,
    pub number: u32,
}

impl std::str::FromStr for MemberRef {
    type Err = String;

    fn from_str(spec: &str) -> std::result::Result<Self, Self::Err> {
        let bad = || format!("'{spec}' is not of the form TENANT/TREE#N");
        let (tree, number) = spec.rsplit_once('#').ok_or_else(bad)?;
        let number: u32 = number.parse().map_err(|_| bad())?;
        if number == 0 {
            return Err(bad());
        }
        Ok(Self {
            tree: tree.parse().map_err(|_| bad())?,
            number,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StateFilter {
    Open,
    Merged,
    Closed,
    All,
}

impl StateFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }
}

/// A server-side merge method (`domain.Merge*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MergeMethod {
    /// Move the target branch to the source tip
    FastForward,
    /// Write a merge snapshot whose parents are the target and source tips
    Merge,
    /// Write one snapshot on the target with the combined changes
    Squash,
}

impl MergeMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FastForward => "fast-forward",
            Self::Merge => "merge",
            Self::Squash => "squash",
        }
    }
}

/// `review.Mergeability`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Mergeability {
    fast_forward: bool,
    up_to_date: bool,
    #[serde(deserialize_with = "nullable")]
    methods: Vec<String>,
    default: String,
    #[serde(deserialize_with = "nullable")]
    allowed: Vec<String>,
    merge_base: String,
    #[serde(deserialize_with = "nullable")]
    conflicts: Vec<MergeConflict>,
    #[serde(deserialize_with = "nullable")]
    reasons: std::collections::BTreeMap<String, String>,
}

/// `review.Conflict`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MergeConflict {
    tree: String,
    path: String,
    kind: String,
}

/// `domain.MergeRequest`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MergeRequest {
    number: u32,
    title: String,
    description: String,
    source_branch: String,
    target_branch: String,
    author: String,
    state: String,
    #[serde(deserialize_with = "nullable")]
    approvals: Vec<Approval>,
    created_at: String,
    updated_at: String,
    merged_at: Option<String>,
    merged_snapshot: Option<String>,
    linked_group: Option<String>,
    merge_method: Option<String>,
}

/// `domain.Approval`: a review decision ("" = a pre-v0.5 approval).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Approval {
    username: String,
    at: String,
    decision: String,
    snapshot_id: String,
    body: String,
}

impl Approval {
    fn approved(&self) -> bool {
        self.decision.is_empty() || self.decision == "approved"
    }
}

/// `domain.ReviewComment`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Comment {
    id: String,
    author: String,
    body: String,
    path: String,
    line: u32,
    snapshot_id: String,
    resolved: bool,
    created_at: String,
}

/// `review.MergeChecks`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Checks {
    mergeable: bool,
    #[serde(deserialize_with = "nullable")]
    blockers: Vec<Blocker>,
    approvals: ApprovalSummary,
    ci: CiSummary,
    todos: TodoSummary,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Blocker {
    kind: String,
    message: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ApprovalSummary {
    required: u32,
    current: u32,
    stale: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CiSummary {
    state: String,
    #[serde(deserialize_with = "nullable")]
    required: Vec<String>,
    #[serde(deserialize_with = "nullable")]
    missing: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TodoSummary {
    blocking_open: u32,
}

/// A linked group as served by `/tenants/{t}/linked-groups`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Group {
    id: String,
    name: String,
    state: String,
    created_by: String,
    created_at: String,
    #[serde(deserialize_with = "nullable")]
    members: Vec<GroupMember>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GroupMember {
    tree_id: String,
    mr_number: u32,
    tree: String,
    title: String,
    state: String,
}

pub fn run(ctx: &Context, args: MrArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        MrCommand::List { tree, state } => {
            let tree = tree_or_default(ctx, tree)?;
            let value =
                api.get_query(&mrs_path(&tree), &[("state", state.as_str().to_string())])?;
            render(out, &value, |t, mrs: Vec<MergeRequest>| list(t, &mrs))
        }
        MrCommand::Show { number, tree, diff } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&mr_path(&tree, number))?;
            if !diff {
                return render(out, &value, |t, mr: MergeRequest| show(t, &mr));
            }
            let mr: MergeRequest = decode(&value)?;
            let changes = mr_diff(&api, &tree, &mr)?;
            let cmp: CompareResult = decode(&changes)?;
            out.emit(&json!({ "merge_request": value, "diff": changes }), |t| {
                show(t, &mr);
                t.blank();
                render_compare(t, &cmp, CompareView::Patch);
            })
        }
        MrCommand::Create {
            tree,
            source,
            target,
            title,
            description,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let body = json!({
                "title": title,
                "description": description.unwrap_or_default(),
                "source_branch": source,
                "target_branch": target,
            });
            let value = api.post(&mrs_path(&tree), &body)?;
            render(out, &value, |t, mr: MergeRequest| {
                t.line(format_args!(
                    "Opened {} {} ({} -> {}) on {tree}",
                    format!("!{}", mr.number).green().bold(),
                    mr.title,
                    mr.source_branch.green(),
                    mr.target_branch.green()
                ));
            })
        }
        MrCommand::Approve { number, tree } => {
            action(ctx, &api, tree, number, "approve", |t, mr| {
                t.line(format_args!(
                    "Approved {} ({})",
                    format!("!{}", mr.number).bold(),
                    approvals(mr)
                ));
            })
        }
        MrCommand::Merge {
            number,
            tree,
            method,
            message,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api
                .post(
                    &format!("{}/merge", mr_path(&tree, number)),
                    &merge_body(method, message.as_deref()),
                )
                .map_err(|e| merge_error(e, number))?;
            render(out, &value, |t, mr: MergeRequest| {
                t.line(format_args!(
                    "Merged {} into {} at {}{}",
                    format!("!{}", mr.number).bold(),
                    mr.target_branch.green(),
                    short(mr.merged_snapshot.as_deref().unwrap_or_default()).yellow(),
                    method_suffix(&mr)
                ));
            })
        }
        MrCommand::Mergeability { number, tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&format!("{}/mergeability", mr_path(&tree, number)))?;
            render(out, &value, |t, m: Mergeability| {
                mergeability(t, number, &m)
            })
        }
        MrCommand::Close { number, tree } => action(ctx, &api, tree, number, "close", |t, mr| {
            t.line(format_args!("Closed {}", format!("!{}", mr.number).bold()));
        }),
        MrCommand::Review {
            number,
            tree,
            approve,
            request_changes,
            comment: _,
            message,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let decision = decision(approve, request_changes);
            let body = json!({ "decision": decision, "body": message.unwrap_or_default() });
            let value = api.post(&format!("{}/review", mr_path(&tree, number)), &body)?;
            render(out, &value, |t, mr: MergeRequest| {
                let what = match decision {
                    "approved" => "Approved",
                    "changes_requested" => "Requested changes on",
                    _ => "Commented on",
                };
                t.line(format_args!(
                    "{what} {} ({})",
                    format!("!{}", mr.number).bold(),
                    approvals(&mr)
                ));
            })
        }
        MrCommand::Comment {
            number,
            tree,
            message,
            path,
            line,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let mut body = json!({ "body": message });
            if let Some(path) = path {
                body["path"] = json!(path);
            }
            if let Some(line) = line {
                body["line"] = json!(line);
            }
            let value = api.post(&format!("{}/comments", mr_path(&tree, number)), &body)?;
            render(out, &value, |t, c: Comment| {
                t.line(format_args!(
                    "Commented on {} (comment {})",
                    format!("!{number}").bold(),
                    c.id
                ));
            })
        }
        MrCommand::Comments { number, tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&format!("{}/comments", mr_path(&tree, number)))?;
            render(out, &value, |t, cs: Vec<Comment>| comments(t, &cs))
        }
        MrCommand::Checks { number, tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&format!("{}/checks", mr_path(&tree, number)))?;
            render(out, &value, |t, c: Checks| checks(t, number, &c))
        }
        MrCommand::Link {
            name,
            members,
            tenant,
        } => {
            let tenant = tenant.unwrap_or_else(|| members[0].tree.tenant.clone());
            let body = json!({
                "name": name,
                "members": members
                    .iter()
                    .map(|m| json!({ "tree": m.tree.to_string(), "number": m.number }))
                    .collect::<Vec<_>>(),
            });
            let value = api.post(&groups_path(&tenant), &body)?;
            render(out, &value, |t, g: Group| {
                t.line(format_args!(
                    "Linked {} as group {} ({})",
                    plural(g.members.len(), "merge request", "merge requests"),
                    g.name.bold(),
                    g.id.yellow()
                ));
            })
        }
        MrCommand::MergeGroup { id, tenant, method } => {
            let tenant = group_tenant(ctx, tenant)?;
            let value = api
                .post(
                    &format!("{}/{}/merge", groups_path(&tenant), seg(&id)),
                    &merge_body(method, None),
                )
                .map_err(|e| merge_error(e, 0))?;
            render(out, &value, |t, g: Group| {
                t.line(format_args!(
                    "Merged group {} ({})",
                    g.name.bold(),
                    plural(g.members.len(), "merge request", "merge requests")
                ));
                group_members(t, &g);
            })
        }
        MrCommand::Groups { tenant } => {
            let tenant = group_tenant(ctx, tenant)?;
            let value = api.get(&groups_path(&tenant))?;
            render(out, &value, |t, gs: Vec<Group>| groups(t, &gs))
        }
    }
}

/// The review decision chosen by the flags (`--comment` otherwise).
fn decision(approve: bool, request_changes: bool) -> &'static str {
    if approve {
        "approved"
    } else if request_changes {
        "changes_requested"
    } else {
        "commented"
    }
}

fn groups_path(tenant: &str) -> String {
    format!("{}/linked-groups", tenant_path(tenant))
}

/// `explicit`, else the origin tenant of the current worktree.
fn group_tenant(ctx: &Context, explicit: Option<String>) -> Result<String> {
    explicit.or_else(|| cwd_origin_tenant(ctx)).ok_or_else(|| {
        CliError::usage("no --tenant given and not inside a worktree with an origin remote")
            .with_hint("pass --tenant SLUG")
    })
}

fn mrs_path(tree: &TreeRef) -> String {
    format!("{}/merge-requests", tree.api())
}

fn mr_path(tree: &TreeRef, number: u32) -> String {
    format!("{}/{number}", mrs_path(tree))
}

/// The body of a merge request or linked group merge: `{"method"?, "message"?}`.
fn merge_body(method: Option<MergeMethod>, message: Option<&str>) -> Value {
    let mut body = json!({});
    if let Some(method) = method {
        body["method"] = json!(method.as_str());
    }
    if let Some(message) = message {
        body["message"] = json!(message);
    }
    body
}

/// Adds hints to merge refusals: conflicts exit with code 3 like `wt merge`.
fn merge_error(err: CliError, number: u32) -> CliError {
    let text = err.to_string();
    let target = if number > 0 {
        format!("`wt mr mergeability {number}`")
    } else {
        "`wt mr mergeability N`".to_string()
    };
    if text.contains("merge_conflict") {
        CliError::conflicts(text).with_hint(format!(
            "merge the target into the source locally (`wt merge`), resolve, push, and retry; {target} lists the conflicts"
        ))
    } else if text.contains("merge_method_not_allowed") {
        err.with_hint(format!(
            "{target} shows the methods branch protection allows"
        ))
    } else {
        err
    }
}

/// ` (squash)` for merges that did not fast-forward.
fn method_suffix(mr: &MergeRequest) -> String {
    match mr.merge_method.as_deref() {
        Some(m) if !m.is_empty() && m != "fast-forward" => format!(" ({m})"),
        _ => String::new(),
    }
}

fn mergeability(t: &mut Text, number: u32, m: &Mergeability) {
    let head = format!("!{number}");
    if m.methods.is_empty() {
        t.line(format_args!(
            "{} cannot be merged on the server",
            head.bold().red()
        ));
    } else {
        t.line(format_args!(
            "{} can be merged with {} (default {})",
            head.bold().green(),
            m.methods.join(", "),
            m.default.bold()
        ));
    }
    let state = if m.up_to_date {
        "up to date (the target already contains the source)"
    } else if m.fast_forward {
        "fast-forward possible"
    } else {
        "diverged (needs a merge or squash snapshot)"
    };
    t.line(format_args!("  history: {state}"));
    if !m.merge_base.is_empty() {
        t.line(format_args!(
            "  merge base: {}",
            short(&m.merge_base).yellow()
        ));
    }
    let allowed = if m.allowed.is_empty() {
        "none".to_string()
    } else {
        m.allowed.join(", ")
    };
    t.line(format_args!("  allowed by protection: {allowed}"));
    for (method, reason) in &m.reasons {
        t.line(format_args!("  {} {method}: {reason}", "-".dimmed()));
    }
    if !m.conflicts.is_empty() {
        t.line(format_args!(
            "  {}:",
            plural(m.conflicts.len(), "conflict", "conflicts").red()
        ));
        for c in &m.conflicts {
            let path = if c.tree.is_empty() {
                c.path.clone()
            } else {
                format!("{}:{}", c.tree, c.path)
            };
            t.line(format_args!("    {} {path} ({})", "x".red(), c.kind));
        }
    }
}

/// POST `/merge-requests/{n}/{verb}` and render the updated merge request.
fn action(
    ctx: &Context,
    api: &Api,
    tree: Option<TreeRef>,
    number: u32,
    verb: &str,
    human: impl FnOnce(&mut Text, &MergeRequest),
) -> Result<()> {
    let tree = tree_or_default(ctx, tree)?;
    let value = api.post(&format!("{}/{verb}", mr_path(&tree, number)), &json!({}))?;
    render(&ctx.out, &value, |t, mr: MergeRequest| human(t, &mr))
}

/// The changes of a merge request: for a merged one, its merged snapshot
/// against that snapshot's parent; otherwise the source branch against its
/// merge base with the target.
fn mr_diff(api: &Api, tree: &TreeRef, mr: &MergeRequest) -> Result<Value> {
    if let Some(merged) = mr.merged_snapshot.as_deref().filter(|s| !s.is_empty()) {
        return compare(api, tree, None, merged);
    }
    let direct = compare(api, tree, Some(&mr.target_branch), &mr.source_branch)?;
    match decode::<CompareResult>(&direct)?.merge_base {
        Some(base) => compare(api, tree, Some(&base), &mr.source_branch),
        None => Ok(direct),
    }
}

fn state_cell(state: &str) -> Cell {
    let cell = Cell::from(state);
    match state {
        "open" => cell.green(),
        "merged" => cell.cyan(),
        _ => cell.red(),
    }
}

fn approvals(mr: &MergeRequest) -> String {
    plural(
        mr.approvals.iter().filter(|a| a.approved()).count(),
        "approval",
        "approvals",
    )
}

fn comments(t: &mut Text, cs: &[Comment]) {
    if cs.is_empty() {
        t.line("No comments.");
        return;
    }
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            t.blank();
        }
        let mut head = format!("#{} {} {}", c.id, c.author.bold(), when(&c.created_at));
        if !c.path.is_empty() {
            head.push_str(&format!("  {}", c.path.cyan()));
            if c.line > 0 {
                head.push_str(&format!(":{}", c.line));
            }
        }
        if !c.snapshot_id.is_empty() {
            head.push_str(&format!(" @ {}", short(&c.snapshot_id).yellow()));
        }
        if c.resolved {
            head.push_str(&format!("  {}", "[resolved]".dimmed()));
        }
        t.line(head);
        for line in c.body.lines() {
            t.line(format_args!("    {line}"));
        }
    }
}

fn checks(t: &mut Text, number: u32, c: &Checks) {
    if c.mergeable {
        t.line(format_args!(
            "{} can be merged",
            format!("!{number}").bold().green()
        ));
    } else {
        t.line(format_args!(
            "{} cannot be merged",
            format!("!{number}").bold().red()
        ));
    }
    let a = &c.approvals;
    let mut approvals = format!("{} of {} required", a.current, a.required);
    if a.stale > 0 {
        approvals.push_str(&format!(", {} stale", a.stale));
    }
    t.line(format_args!("  approvals: {approvals}"));
    let state = if c.ci.state.is_empty() {
        "no checks"
    } else {
        c.ci.state.as_str()
    };
    let mut ci = state.to_string();
    if !c.ci.required.is_empty() {
        ci.push_str(&format!(", required {}", c.ci.required.join(", ")));
    }
    if !c.ci.missing.is_empty() {
        ci.push_str(&format!(", missing {}", c.ci.missing.join(", ")));
    }
    t.line(format_args!("  ci: {ci}"));
    t.line(format_args!("  blocking todos: {}", c.todos.blocking_open));
    for b in &c.blockers {
        t.line(format_args!("  {} {}: {}", "x".red(), b.kind, b.message));
    }
}

fn group_members(t: &mut Text, g: &Group) {
    for m in &g.members {
        let tree = if m.tree.is_empty() {
            m.tree_id.as_str()
        } else {
            m.tree.as_str()
        };
        let mut line = format!("  {tree}!{}", m.mr_number);
        if !m.title.is_empty() {
            line.push_str(&format!(" {}", m.title));
        }
        if !m.state.is_empty() {
            line.push_str(&format!(" [{}]", m.state));
        }
        t.line(line);
    }
}

fn groups(t: &mut Text, gs: &[Group]) {
    let mut table = Table::new(&["ID", "NAME", "STATE", "MEMBERS", "CREATED BY", "CREATED"]);
    for g in gs {
        let members: Vec<String> = g
            .members
            .iter()
            .map(|m| {
                let tree = if m.tree.is_empty() {
                    &m.tree_id
                } else {
                    &m.tree
                };
                format!("{tree}!{}", m.mr_number)
            })
            .collect();
        table.row([
            Cell::from(g.id.as_str()).yellow(),
            Cell::from(g.name.as_str()).bold(),
            state_cell(&g.state),
            members.join(", ").into(),
            g.created_by.as_str().into(),
            when(&g.created_at).into(),
        ]);
    }
    table.render(t, "No linked groups.");
}

fn list(t: &mut Text, mrs: &[MergeRequest]) {
    let mut table = Table::new(&[
        "MR",
        "TITLE",
        "BRANCHES",
        "AUTHOR",
        "STATE",
        "APPROVALS",
        "UPDATED",
    ]);
    for mr in mrs {
        table.row([
            Cell::from(format!("!{}", mr.number)).bold(),
            mr.title.as_str().into(),
            format!("{} -> {}", mr.source_branch, mr.target_branch).into(),
            mr.author.as_str().into(),
            state_cell(&mr.state),
            mr.approvals
                .iter()
                .filter(|a| a.approved())
                .count()
                .to_string()
                .into(),
            when(&mr.updated_at).into(),
        ]);
    }
    table.render(t, "No merge requests.");
}

fn show(t: &mut Text, mr: &MergeRequest) {
    let state = state_cell(&mr.state);
    t.line(format_args!(
        "{} {}  [{}]",
        format!("!{}", mr.number).bold(),
        mr.title.bold(),
        state.colored()
    ));
    t.line(format_args!(
        "  {} -> {}, opened by {} {}, updated {}",
        mr.source_branch.green(),
        mr.target_branch.green(),
        mr.author,
        when(&mr.created_at),
        when(&mr.updated_at)
    ));
    let review = |a: &Approval| {
        let mut s = format!("{} ({}", a.username, when(&a.at));
        if !a.snapshot_id.is_empty() {
            s.push_str(&format!(" at {}", short(&a.snapshot_id)));
        }
        s.push(')');
        s
    };
    let approved: Vec<String> = mr
        .approvals
        .iter()
        .filter(|a| a.approved())
        .map(review)
        .collect();
    let changes: Vec<&Approval> = mr
        .approvals
        .iter()
        .filter(|a| a.decision == "changes_requested")
        .collect();
    if approved.is_empty() {
        t.line("  no approvals yet".dimmed());
    } else {
        t.line(format_args!("  approved by {}", approved.join(", ")));
    }
    for a in changes {
        t.line(format_args!(
            "  {} {}",
            "changes requested by".red(),
            review(a)
        ));
        for line in a.body.lines() {
            t.line(format_args!("      {line}"));
        }
    }
    if let Some(group) = mr.linked_group.as_deref().filter(|g| !g.is_empty()) {
        t.line(format_args!(
            "  linked group {} (merge with `wt mr merge-group`)",
            group.yellow()
        ));
    }
    if let Some(merged) = mr.merged_snapshot.as_deref().filter(|s| !s.is_empty()) {
        t.line(format_args!(
            "  merged as {} {}{}",
            short(merged).yellow(),
            when(mr.merged_at.as_deref().unwrap_or_default()),
            method_suffix(mr)
        ));
    }
    if !mr.description.is_empty() {
        t.blank();
        for line in mr.description.lines() {
            t.line(format_args!("    {line}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn mr(args: &[&str]) -> MrCommand {
        let mut full = vec!["mr"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Mr(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(
            mr(&["list"]),
            MrCommand::List {
                tree: None,
                state: StateFilter::Open
            }
        ));
        assert!(matches!(
            mr(&["list", "acme/web", "--state", "all"]),
            MrCommand::List {
                tree: Some(_),
                state: StateFilter::All
            }
        ));
        assert!(matches!(
            mr(&["show", "3", "acme/web", "--diff"]),
            MrCommand::Show {
                number: 3,
                tree: Some(_),
                diff: true
            }
        ));
        assert!(matches!(
            mr(&["show", "3"]),
            MrCommand::Show {
                number: 3,
                tree: None,
                diff: false
            }
        ));
        let MrCommand::Create {
            tree,
            source,
            target,
            title,
            description,
        } = mr(&[
            "create", "--source", "feat", "--target", "main", "--title", "Add feat",
        ])
        else {
            panic!()
        };
        assert!(tree.is_none() && description.is_none());
        assert_eq!(
            (source.as_str(), target.as_str(), title.as_str()),
            ("feat", "main", "Add feat")
        );
        assert!(matches!(
            mr(&["approve", "1"]),
            MrCommand::Approve { number: 1, .. }
        ));
        // v0.6: no --method = the server's default method.
        assert!(matches!(
            mr(&["merge", "2", "acme/web"]),
            MrCommand::Merge {
                number: 2,
                tree: Some(_),
                method: None,
                message: None,
            }
        ));
        assert!(matches!(
            mr(&["close", "4"]),
            MrCommand::Close { number: 4, .. }
        ));
        assert_eq!(
            parse_err(&["mr", "create", "--source", "a", "--target", "b"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(parse_err(&["mr", "show", "x"]), ErrorKind::ValueValidation);
        assert_eq!(
            parse_err(&["mr", "list", "--state", "draft"]),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn parses_merge_methods() {
        let MrCommand::Merge {
            number,
            method,
            message,
            ..
        } = mr(&["merge", "3", "--method", "squash", "-m", "Squash it"])
        else {
            panic!()
        };
        assert_eq!(
            (number, method, message.as_deref()),
            (3, Some(MergeMethod::Squash), Some("Squash it"))
        );
        assert!(matches!(
            mr(&["merge", "3", "acme/web", "--method", "fast-forward"]),
            MrCommand::Merge {
                method: Some(MergeMethod::FastForward),
                ..
            }
        ));
        assert!(matches!(
            mr(&["merge", "3", "--method", "merge"]),
            MrCommand::Merge {
                method: Some(MergeMethod::Merge),
                ..
            }
        ));
        assert_eq!(
            parse_err(&["mr", "merge", "3", "--method", "rebase"]),
            ErrorKind::InvalidValue
        );
        assert!(matches!(
            mr(&["mergeability", "4", "acme/web"]),
            MrCommand::Mergeability {
                number: 4,
                tree: Some(_)
            }
        ));
        assert!(matches!(
            mr(&["merge-group", "g1", "--method", "squash"]),
            MrCommand::MergeGroup {
                method: Some(MergeMethod::Squash),
                ..
            }
        ));

        assert_eq!(merge_body(None, None), json!({}));
        assert_eq!(
            merge_body(Some(MergeMethod::FastForward), Some("m")),
            json!({"method": "fast-forward", "message": "m"})
        );
        let conflict = merge_error(
            CliError::failure("cannot be merged automatically (merge_conflict, HTTP 409)"),
            3,
        );
        assert_eq!(conflict.exit_code(), 3);
        let refused = merge_error(
            CliError::failure("not allowed (merge_method_not_allowed, HTTP 409)"),
            3,
        );
        assert_eq!(refused.exit_code(), 1);
        assert_eq!(
            merge_error(CliError::failure("x"), 3),
            CliError::failure("x")
        );
    }

    #[test]
    fn renders_mergeability() {
        colored::control::set_override(false);
        let m: Mergeability = serde_json::from_value(json!({
            "fast_forward": false, "up_to_date": false,
            "methods": [], "default": "", "allowed": ["merge", "squash"],
            "merge_base": "0123456789abcdef",
            "conflicts": [{"path": "a.txt", "kind": "content"}, {"tree": "acme/api", "path": "b", "kind": "binary"}],
            "reasons": {"fast-forward": "target has diverged", "merge": "2 conflicting paths"}
        }))
        .expect("mergeability");
        let mut t = Text::default();
        mergeability(&mut t, 5, &m);
        assert_eq!(
            t.as_str(),
            "!5 cannot be merged on the server\n  history: diverged (needs a merge or squash snapshot)\n  merge base: 01234567\n  allowed by protection: merge, squash\n  - fast-forward: target has diverged\n  - merge: 2 conflicting paths\n  2 conflicts:\n    x a.txt (content)\n    x acme/api:b (binary)\n"
        );
        let m: Mergeability = serde_json::from_value(json!({
            "fast_forward": true, "methods": ["fast-forward", "squash"], "default": "fast-forward",
            "allowed": null, "conflicts": null, "reasons": null
        }))
        .expect("mergeability");
        let mut t = Text::default();
        mergeability(&mut t, 5, &m);
        assert_eq!(
            t.as_str(),
            "!5 can be merged with fast-forward, squash (default fast-forward)\n  history: fast-forward possible\n  allowed by protection: none\n"
        );
        let merged = MergeRequest {
            merge_method: Some("squash".into()),
            ..MergeRequest::default()
        };
        assert_eq!(method_suffix(&merged), " (squash)");
        assert_eq!(method_suffix(&MergeRequest::default()), "");
    }

    #[test]
    fn parses_review_commands() {
        let MrCommand::Review {
            number,
            tree,
            approve,
            request_changes,
            comment,
            message,
        } = mr(&["review", "3", "acme/web", "--request-changes", "-m", "fix"])
        else {
            panic!()
        };
        assert_eq!(number, 3);
        assert!(tree.is_some() && request_changes && !approve && !comment);
        assert_eq!(message.as_deref(), Some("fix"));
        assert_eq!(decision(false, true), "changes_requested");
        assert_eq!(decision(true, false), "approved");
        assert_eq!(decision(false, false), "commented");
        assert!(matches!(
            mr(&["review", "3", "--approve"]),
            MrCommand::Review { approve: true, .. }
        ));
        assert!(matches!(
            mr(&["review", "3", "--comment", "-m", "hi"]),
            MrCommand::Review { comment: true, .. }
        ));
        assert_eq!(
            parse_err(&["mr", "review", "3"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["mr", "review", "3", "--approve", "--request-changes"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["mr", "review", "3", "--comment"]),
            ErrorKind::MissingRequiredArgument
        );

        let MrCommand::Comment {
            number,
            message,
            path,
            line,
            ..
        } = mr(&[
            "comment", "2", "-m", "why?", "--path", "src/a.rs", "--line", "7",
        ])
        else {
            panic!()
        };
        assert_eq!((number, message.as_str()), (2, "why?"));
        assert_eq!((path.as_deref(), line), (Some("src/a.rs"), Some(7)));
        assert_eq!(
            parse_err(&["mr", "comment", "2", "-m", "x", "--line", "3"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["mr", "comment", "2"]),
            ErrorKind::MissingRequiredArgument
        );
        assert!(matches!(
            mr(&["comments", "2", "acme/web"]),
            MrCommand::Comments {
                number: 2,
                tree: Some(_)
            }
        ));
        assert!(matches!(
            mr(&["checks", "5"]),
            MrCommand::Checks {
                number: 5,
                tree: None
            }
        ));
    }

    #[test]
    fn parses_group_commands() {
        let MrCommand::Link {
            name,
            members,
            tenant,
        } = mr(&["link", "release", "acme/web#2", "other/api#10"])
        else {
            panic!()
        };
        assert_eq!(name, "release");
        assert!(tenant.is_none());
        assert_eq!(members[0].tree.to_string(), "acme/web");
        assert_eq!((members[0].number, members[1].number), (2, 10));
        assert_eq!(members[1].tree.tenant, "other");
        assert_eq!(
            parse_err(&["mr", "link", "release", "acme/web#2"]),
            ErrorKind::TooFewValues
        );
        assert_eq!(
            parse_err(&["mr", "link", "g", "acme/web", "acme/api#1"]),
            ErrorKind::ValueValidation
        );
        for bad in ["acme/web#0", "acme/web#x", "web#1", "#1"] {
            assert!(bad.parse::<MemberRef>().is_err(), "{bad}");
        }
        assert!(matches!(
            mr(&["merge-group", "abc", "--tenant", "acme"]),
            MrCommand::MergeGroup {
                tenant: Some(_),
                ..
            }
        ));
        assert!(matches!(
            mr(&["groups"]),
            MrCommand::Groups { tenant: None }
        ));
    }

    #[test]
    fn renders_reviews_checks_comments_and_groups() {
        colored::control::set_override(false);
        let mr: MergeRequest = serde_json::from_value(json!({
            "number": 4, "title": "T", "source_branch": "feat", "target_branch": "main",
            "author": "ada", "state": "open", "created_at": "", "updated_at": "",
            "linked_group": "g1",
            "approvals": [
                {"username": "bob", "at": "", "decision": "approved", "snapshot_id": "0123456789abcdef"},
                {"username": "cy", "at": "", "decision": "changes_requested", "body": "fix x"},
                {"username": "old", "at": ""}
            ]
        }))
        .expect("mr");
        assert_eq!(approvals(&mr), "2 approvals");
        let mut t = Text::default();
        show(&mut t, &mr);
        assert_eq!(
            t.as_str(),
            "!4 T  [open]\n  feat -> main, opened by ada -, updated -\n  approved by bob (- at 01234567), old (-)\n  changes requested by cy (-)\n      fix x\n  linked group g1 (merge with `wt mr merge-group`)\n"
        );

        let c: Checks = serde_json::from_value(json!({
            "mergeable": false,
            "blockers": [{"kind": "ci", "message": "required CI checks have not passed: missing test"}],
            "approvals": {"required": 2, "current": 1, "stale": 1},
            "ci": {"state": "success", "required": ["build", "test"], "missing": ["test"]},
            "todos": {"blocking_open": 0}
        }))
        .expect("checks");
        let mut t = Text::default();
        checks(&mut t, 4, &c);
        assert_eq!(
            t.as_str(),
            "!4 cannot be merged\n  approvals: 1 of 2 required, 1 stale\n  ci: success, required build, test, missing test\n  blocking todos: 0\n  x ci: required CI checks have not passed: missing test\n"
        );
        let mut t = Text::default();
        checks(
            &mut t,
            4,
            &Checks {
                mergeable: true,
                ..Checks::default()
            },
        );
        assert!(
            t.as_str().starts_with("!4 can be merged\n"),
            "{}",
            t.as_str()
        );
        assert!(t.as_str().contains("ci: no checks"), "{}", t.as_str());

        let cs: Vec<Comment> = serde_json::from_value(json!([
            {"id": "1", "author": "bob", "body": "why?", "path": "a.txt", "line": 3,
             "snapshot_id": "0123456789abcdef", "resolved": true, "created_at": ""},
            {"id": "2", "author": "ada", "body": "because\nreasons", "created_at": ""}
        ]))
        .expect("comments");
        let mut t = Text::default();
        comments(&mut t, &cs);
        assert_eq!(
            t.as_str(),
            "#1 bob -  a.txt:3 @ 01234567  [resolved]\n    why?\n\n#2 ada -\n    because\n    reasons\n"
        );
        let mut t = Text::default();
        comments(&mut t, &[]);
        assert_eq!(t.as_str(), "No comments.\n");

        let gs: Vec<Group> = serde_json::from_value(json!([{
            "id": "g1", "name": "rel", "state": "open", "created_by": "ada", "created_at": "",
            "members": [
                {"tree_id": "t1", "mr_number": 2, "tree": "acme/web", "title": "Web", "state": "open"},
                {"tree_id": "t2", "mr_number": 1}
            ]
        }]))
        .expect("groups");
        let mut t = Text::default();
        groups(&mut t, &gs);
        assert_eq!(
            t.as_str(),
            "ID  NAME  STATE  MEMBERS           CREATED BY  CREATED\ng1  rel   open   acme/web!2, t2!1  ada         -\n"
        );
        let mut t = Text::default();
        group_members(&mut t, &gs[0]);
        assert_eq!(t.as_str(), "  acme/web!2 Web [open]\n  t2!1\n");
    }

    #[test]
    fn renders_list_and_detail() {
        colored::control::set_override(false);
        let mr: MergeRequest = serde_json::from_value(json!({
            "number": 7, "title": "Add feature", "description": "Line one\nLine two",
            "source_branch": "feat", "target_branch": "main", "author": "ada",
            "state": "merged", "approvals": [{"username": "bob", "at": ""}],
            "created_at": "", "updated_at": "", "merged_snapshot": "0123456789abcdef"
        }))
        .expect("mr");
        let mut t = Text::default();
        show(&mut t, &mr);
        assert_eq!(
            t.as_str(),
            "!7 Add feature  [merged]\n  feat -> main, opened by ada -, updated -\n  approved by bob (-)\n  merged as 01234567 -\n\n    Line one\n    Line two\n"
        );
        let mut t = Text::default();
        list(&mut t, &[mr]);
        assert!(
            t.as_str()
                .contains("!7  Add feature  feat -> main  ada     merged  1"),
            "{}",
            t.as_str()
        );
        assert_eq!(approvals(&MergeRequest::default()), "0 approvals");
    }
}
