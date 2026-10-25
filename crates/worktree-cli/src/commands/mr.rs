//! `wt mr` — merge requests: list, open, review, approve, merge, close.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::commands::server_common::{
    decode, nullable, render, tree_or_default, when, Api, Cell, Table, TreeRef,
};
use crate::commands::trees::{compare, render_compare, CompareResult, CompareView};
use crate::context::Context;
use crate::error::Result;
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
    /// Merge (fast-forward) a merge request into its target
    Merge {
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
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Approval {
    username: String,
    at: String,
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
        MrCommand::Merge { number, tree } => action(ctx, &api, tree, number, "merge", |t, mr| {
            t.line(format_args!(
                "Merged {} into {} at {}",
                format!("!{}", mr.number).bold(),
                mr.target_branch.green(),
                short(mr.merged_snapshot.as_deref().unwrap_or_default()).yellow()
            ));
        }),
        MrCommand::Close { number, tree } => action(ctx, &api, tree, number, "close", |t, mr| {
            t.line(format_args!("Closed {}", format!("!{}", mr.number).bold()));
        }),
    }
}

fn mrs_path(tree: &TreeRef) -> String {
    format!("{}/merge-requests", tree.api())
}

fn mr_path(tree: &TreeRef, number: u32) -> String {
    format!("{}/{number}", mrs_path(tree))
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
    plural(mr.approvals.len(), "approval", "approvals")
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
            mr.approvals.len().to_string().into(),
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
    if mr.approvals.is_empty() {
        t.line("  no approvals yet".dimmed());
    } else {
        let names: Vec<String> = mr
            .approvals
            .iter()
            .map(|a| format!("{} ({})", a.username, when(&a.at)))
            .collect();
        t.line(format_args!("  approved by {}", names.join(", ")));
    }
    if let Some(merged) = mr.merged_snapshot.as_deref().filter(|s| !s.is_empty()) {
        t.line(format_args!(
            "  merged as {} {}",
            short(merged).yellow(),
            when(mr.merged_at.as_deref().unwrap_or_default())
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
        assert!(matches!(
            mr(&["merge", "2", "acme/web"]),
            MrCommand::Merge {
                number: 2,
                tree: Some(_)
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
