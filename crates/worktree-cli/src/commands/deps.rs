//! `wt deps` — dependencies declared by snapshots (`wt snapshot --depend`):
//! what a branch needs from other trees, what other trees need from it, and
//! the dependency graph (ENGINEERING_PLAN §13.5).

use std::collections::BTreeMap;

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Deserialize;

use crate::commands::server_common::{
    nullable, or_dash, render, tree_or_default, Api, Cell, Table, TreeRef,
};
use crate::commands::todo::{status_cell, Todo};
use crate::context::Context;
use crate::error::Result;
use crate::output::format::short;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct DepsArgs {
    #[command(subcommand)]
    pub command: DepsCommand,
}

#[derive(Debug, Subcommand)]
pub enum DepsCommand {
    /// Dependencies of a branch and their TODOs, plus TODOs requested from the tree
    Status {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Branch whose snapshots declare the dependencies (default: the default branch)
        #[arg(long, value_name = "B")]
        branch: Option<String>,
    },
    /// Dependency graph between trees (text or Graphviz dot)
    Graph {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        #[arg(long, value_name = "B")]
        branch: Option<String>,
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        format: GraphFormat,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GraphFormat {
    Text,
    Dot,
}

/// `todo.Report`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Report {
    branch: String,
    #[serde(deserialize_with = "nullable")]
    outgoing: Vec<Outgoing>,
    #[serde(deserialize_with = "nullable")]
    incoming: Vec<Todo>,
    #[serde(deserialize_with = "nullable")]
    trees: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Outgoing {
    snapshot_id: String,
    dependency: Dependency,
    todo: Option<TodoRef>,
}

/// `domain.Dependency` (JSON names of the server).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Dependency {
    tenant_slug: String,
    tree: String,
    branch: String,
    title: String,
    blocking: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TodoRef {
    tenant: String,
    tree: String,
    number: u32,
    status: String,
}

pub fn run(ctx: &Context, args: DepsArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    match args.command {
        DepsCommand::Status { tree, branch } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = fetch(&api, &tree, branch)?;
            render(&ctx.out, &value, |t, rep: Report| status(t, &tree, &rep))
        }
        DepsCommand::Graph {
            tree,
            branch,
            format,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = fetch(&api, &tree, branch)?;
            render(&ctx.out, &value, |t, rep: Report| {
                let edges = edges(&tree, &rep);
                match format {
                    GraphFormat::Text => graph_text(t, &tree, &edges),
                    GraphFormat::Dot => graph_dot(t, &tree, &edges),
                }
            })
        }
    }
}

fn fetch(api: &Api, tree: &TreeRef, branch: Option<String>) -> Result<serde_json::Value> {
    api.get_query(
        &format!("{}/dependencies", tree.api()),
        &[("branch", branch.unwrap_or_default())],
    )
}

/// `TENANT/TREE` of a dependency target (the tenant defaults to `from`'s).
fn target_name(from: &TreeRef, d: &Dependency) -> String {
    let tenant = if d.tenant_slug.is_empty() {
        &from.tenant
    } else {
        &d.tenant_slug
    };
    format!("{tenant}/{}", d.tree)
}

fn status(t: &mut Text, tree: &TreeRef, rep: &Report) {
    t.line(format_args!(
        "{} {} needs from other trees:",
        tree.to_string().bold(),
        rep.branch.green()
    ));
    let mut table = Table::new(&["SNAPSHOT", "TARGET", "TITLE", "TODO", "STATUS"]);
    for o in &rep.outgoing {
        let mut target = target_name(tree, &o.dependency);
        if !o.dependency.branch.is_empty() {
            target = format!("{target}@{}", o.dependency.branch);
        }
        let mut title = o.dependency.title.clone();
        if o.dependency.blocking {
            title.push_str(" [blocking]");
        }
        let (todo, state) = match &o.todo {
            Some(r) => (
                Cell::from(format!("#{}", r.number)).bold(),
                status_cell(&r.status),
            ),
            None => ("-".into(), Cell::from("unknown").yellow()),
        };
        table.row([
            Cell::from(short(&o.snapshot_id)).yellow(),
            target.into(),
            title.into(),
            todo,
            state,
        ]);
    }
    table.render(t, "  (no dependencies declared)");
    t.blank();
    t.line(format_args!("Requested from {}:", tree.to_string().bold()));
    let mut table = Table::new(&["TODO", "FROM", "TITLE", "PRIORITY", "STATUS", "BRANCH"]);
    for todo in &rep.incoming {
        let from = rep
            .trees
            .get(&todo.source_tree_id)
            .cloned()
            .unwrap_or_else(|| "(hidden)".into());
        table.row([
            Cell::from(format!("#{}", todo.number)).bold(),
            from.into(),
            todo.title.as_str().into(),
            todo.priority.as_str().into(),
            status_cell(&todo.status),
            or_dash(&todo.target_branch).into(),
        ]);
    }
    table.render(t, "  (nothing requested)");
}

/// One edge of the dependency graph: `from` needs `to` (label = TODO).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    from: String,
    to: String,
    label: String,
}

fn edges(tree: &TreeRef, rep: &Report) -> Vec<Edge> {
    let me = tree.to_string();
    let mut out: Vec<Edge> = rep
        .outgoing
        .iter()
        .map(|o| {
            let (to, label) = match &o.todo {
                Some(r) => (
                    format!("{}/{}", r.tenant, r.tree),
                    format!("#{} {} ({})", r.number, o.dependency.title, r.status),
                ),
                None => (
                    target_name(tree, &o.dependency),
                    format!("{} (no TODO)", o.dependency.title),
                ),
            };
            Edge {
                from: me.clone(),
                to,
                label,
            }
        })
        .collect();
    out.extend(rep.incoming.iter().map(|todo| {
        Edge {
            from: rep
                .trees
                .get(&todo.source_tree_id)
                .cloned()
                .unwrap_or_else(|| "(hidden)".into()),
            to: me.clone(),
            label: format!("#{} {} ({})", todo.number, todo.title, todo.status),
        }
    }));
    out.sort();
    out.dedup();
    out
}

fn graph_text(t: &mut Text, tree: &TreeRef, edges: &[Edge]) {
    if edges.is_empty() {
        t.line(format!("{tree} has no dependencies").dimmed());
        return;
    }
    for e in edges {
        t.line(format_args!("{} -> {}  {}", e.from, e.to, e.label));
    }
}

fn dot_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn graph_dot(t: &mut Text, tree: &TreeRef, edges: &[Edge]) {
    t.line("digraph dependencies {");
    t.line("  rankdir=LR;");
    t.line(format_args!(
        "  {} [style=bold];",
        dot_quote(&tree.to_string())
    ));
    for e in edges {
        t.line(format_args!(
            "  {} -> {} [label={}];",
            dot_quote(&e.from),
            dot_quote(&e.to),
            dot_quote(&e.label)
        ));
    }
    t.line("}");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use serde_json::json;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn deps(args: &[&str]) -> DepsCommand {
        let mut full = vec!["deps"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Deps(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            deps(&["status"]),
            DepsCommand::Status {
                tree: None,
                branch: None
            }
        ));
        let DepsCommand::Status { branch, .. } = deps(&["status", "acme/web", "--branch", "dev"])
        else {
            panic!()
        };
        assert_eq!(branch.as_deref(), Some("dev"));
        assert!(matches!(
            deps(&["graph", "--format", "dot"]),
            DepsCommand::Graph {
                format: GraphFormat::Dot,
                ..
            }
        ));
        assert!(matches!(
            deps(&["graph"]),
            DepsCommand::Graph {
                format: GraphFormat::Text,
                ..
            }
        ));
        assert_eq!(
            parse_err(&["deps", "graph", "--format", "svg"]),
            ErrorKind::InvalidValue
        );
    }

    fn report() -> Report {
        serde_json::from_value(json!({
            "branch": "main",
            "outgoing": [
                {"snapshot_id": "aaaaaaaaaaaa", "dependency": {"tree": "api", "title": "Add \"v2\"", "blocking": true},
                 "todo": {"tenant": "acme", "tree": "api", "number": 1, "status": "open"}},
                {"snapshot_id": "bbbbbbbbbbbb", "dependency": {"tenant_slug": "other", "tree": "lib", "branch": "next", "title": "Bump"},
                 "todo": null}
            ],
            "incoming": [
                {"number": 4, "title": "Docs", "priority": "low", "status": "done", "source_tree_id": "t-docs", "target_branch": "main"},
                {"number": 5, "title": "Secret", "priority": "medium", "status": "open", "source_tree_id": "t-hidden"}
            ],
            "trees": {"t-web": "acme/web", "t-docs": "acme/docs"}
        }))
        .expect("report")
    }

    #[test]
    fn renders_status() {
        colored::control::set_override(false);
        let tree: TreeRef = "acme/web".parse().expect("tree");
        let mut t = Text::default();
        status(&mut t, &tree, &report());
        let s = t.as_str();
        assert!(s.contains("acme/web main needs from other trees:"), "{s}");
        assert!(
            s.contains("aaaaaaaa  acme/api        Add \"v2\" [blocking]  #1    open"),
            "{s}"
        );
        assert!(s.contains("bbbbbbbb  other/lib@next  Bump"), "{s}");
        assert!(s.contains("unknown"), "{s}");
        assert!(s.contains("#4    acme/docs  Docs"), "{s}");
        assert!(s.contains("(hidden)"), "{s}");

        let mut t = Text::default();
        status(&mut t, &tree, &Report::default());
        assert!(t.as_str().contains("(no dependencies declared)"));
        assert!(t.as_str().contains("(nothing requested)"));
    }

    #[test]
    fn renders_graphs() {
        colored::control::set_override(false);
        let tree: TreeRef = "acme/web".parse().expect("tree");
        let e = edges(&tree, &report());
        let mut t = Text::default();
        graph_text(&mut t, &tree, &e);
        assert_eq!(
            t.as_str(),
            "(hidden) -> acme/web  #5 Secret (open)\n\
             acme/docs -> acme/web  #4 Docs (done)\n\
             acme/web -> acme/api  #1 Add \"v2\" (open)\n\
             acme/web -> other/lib  Bump (no TODO)\n"
        );
        let mut t = Text::default();
        graph_dot(&mut t, &tree, &e);
        let dot = t.as_str();
        assert!(
            dot.starts_with(
                "digraph dependencies {\n  rankdir=LR;\n  \"acme/web\" [style=bold];\n"
            ),
            "{dot}"
        );
        assert!(
            dot.contains("  \"acme/web\" -> \"acme/api\" [label=\"#1 Add \\\"v2\\\" (open)\"];\n"),
            "{dot}"
        );
        assert!(dot.ends_with("}\n"));
        let mut t = Text::default();
        graph_text(&mut t, &tree, &[]);
        assert_eq!(t.as_str(), "acme/web has no dependencies\n");
    }
}
