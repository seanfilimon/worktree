//! `wt todo` — TODOs of a server tree: created from snapshot dependencies
//! (`wt snapshot --depend`) or by hand; list, show, claim, complete,
//! cancel, create (ENGINEERING_PLAN §13.5).

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::commands::server_common::{
    decode, fields, nullable, or_dash, render, tenant_path, tree_or_default, when, yes_no, Api,
    Cell, Table, TreeRef,
};
use crate::context::Context;
use crate::error::Result;
use crate::output::format::short;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct TodoArgs {
    #[command(subcommand)]
    pub command: TodoCommand,
}

#[derive(Debug, Subcommand)]
pub enum TodoCommand {
    /// List TODOs of a tree
    List {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Which TODOs to list (active = open and in progress)
        #[arg(long, value_name = "STATUS", default_value = "active")]
        status: StatusFilter,
        /// Only TODOs targeting this branch
        #[arg(long, value_name = "B")]
        branch: Option<String>,
        /// Only TODOs assigned to this user ("me" = you)
        #[arg(long, value_name = "USER")]
        assignee: Option<String>,
        /// List the TODOs of every tree of the tenant
        #[arg(long)]
        all_trees: bool,
    },
    /// Show one TODO
    Show {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Assign a TODO to yourself and mark it in progress
    Claim {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Mark a TODO done
    Complete {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Snapshot (id, branch or tag) that resolves it
        #[arg(long, value_name = "REV")]
        snapshot: Option<String>,
    },
    /// Cancel a TODO
    Cancel {
        number: u32,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Create a TODO by hand
    Create {
        title: String,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        #[arg(long, value_name = "TEXT")]
        details: Option<String>,
        #[arg(long, value_name = "PRIORITY", default_value = "medium")]
        priority: Priority,
        /// Blocks merging of the branches that depend on it
        #[arg(long)]
        blocking: bool,
        /// Branch the work is needed on (default: the tree's default branch)
        #[arg(long, value_name = "B")]
        branch: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFilter {
    Active,
    Open,
    InProgress,
    Done,
    Cancelled,
    All,
}

impl StatusFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Priority {
    fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// `domain.Todo`.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Todo {
    pub number: u32,
    pub tree_id: String,
    pub target_branch: String,
    pub title: String,
    pub details: String,
    pub priority: String,
    pub blocking: bool,
    pub linked: bool,
    pub status: String,
    pub source_tree_id: String,
    pub source_branch: String,
    pub source_snapshot_id: String,
    pub author: String,
    pub assignee: String,
    pub resolved_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TodoList {
    #[serde(deserialize_with = "nullable")]
    todos: Vec<Todo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TreeSummary {
    tree: TreeName,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TreeName {
    name: String,
}

pub fn run(ctx: &Context, args: TodoArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        TodoCommand::List {
            tree,
            status,
            branch,
            assignee,
            all_trees,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let query = [
                ("status", status.as_str().to_string()),
                ("branch", branch.unwrap_or_default()),
                ("assignee", assignee.unwrap_or_default()),
            ];
            if !all_trees {
                let value = api.get_query(&todos_path(&tree), &query)?;
                return render(out, &value, |t, list: TodoList| {
                    list_todos(t, &[(tree.clone(), list.todos)], false)
                });
            }
            let summaries: Vec<TreeSummary> =
                decode(&api.get(&format!("{}/trees", tenant_path(&tree.tenant)))?)?;
            let mut raw = Vec::new();
            let mut groups = Vec::new();
            for s in summaries {
                let tr = TreeRef {
                    tenant: tree.tenant.clone(),
                    name: s.tree.name,
                };
                let value = api.get_query(&todos_path(&tr), &query)?;
                let list: TodoList = decode(&value)?;
                raw.push(json!({ "tree": tr.to_string(), "todos": value["todos"] }));
                groups.push((tr, list.todos));
            }
            out.emit(&raw, |t| list_todos(t, &groups, true))
        }
        TodoCommand::Show { number, tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&todo_path(&tree, number))?;
            render(out, &value, |t, todo: Todo| show(t, &todo))
        }
        TodoCommand::Claim { number, tree } => {
            action(ctx, &api, tree, number, "claim", json!({}), |t, todo| {
                t.line(format_args!(
                    "Claimed {} {} (in progress, assigned to {})",
                    format!("#{}", todo.number).bold(),
                    todo.title,
                    todo.assignee
                ));
            })
        }
        TodoCommand::Complete {
            number,
            tree,
            snapshot,
        } => {
            let body = match snapshot {
                Some(rev) => json!({ "snapshot": rev }),
                None => json!({}),
            };
            action(ctx, &api, tree, number, "complete", body, |t, todo| {
                let by = if todo.resolved_by.is_empty() {
                    String::new()
                } else {
                    format!(" by {}", short(&todo.resolved_by).yellow())
                };
                t.line(format_args!(
                    "Completed {} {}{by}",
                    format!("#{}", todo.number).bold(),
                    todo.title
                ));
            })
        }
        TodoCommand::Cancel { number, tree } => {
            action(ctx, &api, tree, number, "cancel", json!({}), |t, todo| {
                t.line(format_args!(
                    "Cancelled {} {}",
                    format!("#{}", todo.number).bold(),
                    todo.title
                ));
            })
        }
        TodoCommand::Create {
            title,
            tree,
            details,
            priority,
            blocking,
            branch,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let body = json!({
                "title": title,
                "details": details.unwrap_or_default(),
                "priority": priority.as_str(),
                "blocking": blocking,
                "target_branch": branch.unwrap_or_default(),
            });
            let value = api.post(&todos_path(&tree), &body)?;
            render(out, &value, |t, todo: Todo| {
                t.line(format_args!(
                    "Created {} {} on {tree}",
                    format!("#{}", todo.number).green().bold(),
                    todo.title
                ));
            })
        }
    }
}

pub fn todos_path(tree: &TreeRef) -> String {
    format!("{}/todos", tree.api())
}

fn todo_path(tree: &TreeRef, number: u32) -> String {
    format!("{}/{number}", todos_path(tree))
}

/// POST `/todos/{n}/{verb}` and render the updated TODO.
fn action(
    ctx: &Context,
    api: &Api,
    tree: Option<TreeRef>,
    number: u32,
    verb: &str,
    body: Value,
    human: impl FnOnce(&mut Text, &Todo),
) -> Result<()> {
    let tree = tree_or_default(ctx, tree)?;
    let value = api.post(&format!("{}/{verb}", todo_path(&tree, number)), &body)?;
    render(&ctx.out, &value, |t, todo: Todo| human(t, &todo))
}

pub fn status_cell(status: &str) -> Cell {
    let cell = Cell::from(status);
    match status {
        "open" => cell.green(),
        "in_progress" => cell.yellow(),
        "done" => cell.cyan(),
        _ => cell.red(),
    }
}

fn priority_cell(priority: &str) -> Cell {
    let cell = Cell::from(priority);
    match priority {
        "critical" | "high" => cell.red(),
        "low" => cell.cyan(),
        _ => cell,
    }
}

fn list_todos(t: &mut Text, groups: &[(TreeRef, Vec<Todo>)], with_tree: bool) {
    let mut header = vec![];
    if with_tree {
        header.push("TREE");
    }
    header.extend([
        "TODO", "TITLE", "PRIORITY", "STATUS", "BRANCH", "ASSIGNEE", "UPDATED",
    ]);
    let mut table = Table::new(&header);
    for (tree, todos) in groups {
        for todo in todos {
            let mut row: Vec<Cell> = Vec::new();
            if with_tree {
                row.push(tree.to_string().into());
            }
            let mut title = todo.title.clone();
            if todo.blocking {
                title.push_str(" [blocking]");
            }
            row.extend([
                Cell::from(format!("#{}", todo.number)).bold(),
                title.into(),
                priority_cell(&todo.priority),
                status_cell(&todo.status),
                todo.target_branch.as_str().into(),
                or_dash(&todo.assignee).into(),
                when(&todo.updated_at).into(),
            ]);
            table.row(row);
        }
    }
    table.render(t, "No TODOs.");
}

fn show(t: &mut Text, todo: &Todo) {
    t.line(format_args!(
        "{} {}  [{}]",
        format!("#{}", todo.number).bold(),
        todo.title.bold(),
        status_cell(&todo.status).colored()
    ));
    let source = if todo.source_snapshot_id.is_empty() {
        "created by hand".to_string()
    } else {
        format!(
            "snapshot {} on {}",
            short(&todo.source_snapshot_id),
            or_dash(&todo.source_branch)
        )
    };
    fields(
        t,
        &[
            ("priority", todo.priority.clone()),
            ("blocking", yes_no(todo.blocking).to_string()),
            ("linked", yes_no(todo.linked).to_string()),
            ("branch", todo.target_branch.clone()),
            ("source", source),
            ("author", or_dash(&todo.author)),
            ("assignee", or_dash(&todo.assignee)),
            ("resolved by", or_dash(short(&todo.resolved_by))),
            ("created", when(&todo.created_at)),
            ("updated", when(&todo.updated_at)),
        ],
    );
    if !todo.details.is_empty() {
        t.blank();
        for line in todo.details.lines() {
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

    fn todo(args: &[&str]) -> TodoCommand {
        let mut full = vec!["todo"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Todo(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(
            todo(&["list"]),
            TodoCommand::List {
                tree: None,
                status: StatusFilter::Active,
                all_trees: false,
                ..
            }
        ));
        let TodoCommand::List {
            tree,
            status,
            branch,
            assignee,
            all_trees,
        } = todo(&[
            "list",
            "acme/web",
            "--status",
            "in-progress",
            "--branch",
            "main",
            "--assignee",
            "me",
            "--all-trees",
        ])
        else {
            panic!()
        };
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert_eq!(status.as_str(), "in_progress");
        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(assignee.as_deref(), Some("me"));
        assert!(all_trees);
        assert!(matches!(
            todo(&["show", "3", "acme/web"]),
            TodoCommand::Show {
                number: 3,
                tree: Some(_)
            }
        ));
        assert!(matches!(
            todo(&["claim", "1"]),
            TodoCommand::Claim { number: 1, .. }
        ));
        let TodoCommand::Complete { snapshot, .. } = todo(&["complete", "2", "--snapshot", "main"])
        else {
            panic!()
        };
        assert_eq!(snapshot.as_deref(), Some("main"));
        assert!(matches!(
            todo(&["cancel", "4"]),
            TodoCommand::Cancel { number: 4, .. }
        ));
        let TodoCommand::Create {
            title,
            priority,
            blocking,
            branch,
            ..
        } = todo(&[
            "create",
            "Do it",
            "--priority",
            "critical",
            "--blocking",
            "--branch",
            "dev",
        ])
        else {
            panic!()
        };
        assert_eq!(title, "Do it");
        assert_eq!(priority.as_str(), "critical");
        assert!(blocking);
        assert_eq!(branch.as_deref(), Some("dev"));
        assert_eq!(
            parse_err(&["todo", "show", "x"]),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            parse_err(&["todo", "list", "--status", "weird"]),
            ErrorKind::InvalidValue
        );
        assert_eq!(
            parse_err(&["todo", "create"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn renders_list_and_detail() {
        colored::control::set_override(false);
        let todo: Todo = serde_json::from_value(json!({
            "number": 3, "title": "Add endpoint", "details": "line one\nline two",
            "priority": "high", "blocking": true, "status": "in_progress",
            "target_branch": "main", "source_snapshot_id": "0123456789abcdef",
            "source_branch": "feat", "author": "ada", "assignee": "bob",
            "created_at": "", "updated_at": ""
        }))
        .expect("todo");
        let mut t = Text::default();
        show(&mut t, &todo);
        let text = t.as_str();
        assert!(
            text.starts_with("#3 Add endpoint  [in_progress]\n"),
            "{text}"
        );
        assert!(
            text.contains("source       snapshot 01234567 on feat"),
            "{text}"
        );
        assert!(text.contains("assignee     bob"), "{text}");
        assert!(text.ends_with("    line one\n    line two\n"), "{text}");

        let tree: TreeRef = "acme/api".parse().expect("tree");
        let mut t = Text::default();
        list_todos(&mut t, &[(tree.clone(), vec![todo.clone()])], false);
        assert!(
            t.as_str()
                .contains("#3    Add endpoint [blocking]  high      in_progress  main    bob"),
            "{}",
            t.as_str()
        );
        let mut t = Text::default();
        list_todos(&mut t, &[(tree, vec![todo])], true);
        assert!(t.as_str().starts_with("TREE      TODO"), "{}", t.as_str());
        let mut t = Text::default();
        list_todos(&mut t, &[], false);
        assert_eq!(t.as_str(), "No TODOs.\n");
    }
}
