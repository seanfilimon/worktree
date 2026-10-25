//! `wt ci` — CI check results: report a status, show checks of a snapshot or branch.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Deserialize;
use serde_json::json;

use crate::commands::server_common::{
    nullable, or_dash, render, tree_or_default, when, Api, Cell, Table, TreeRef,
};
use crate::context::Context;
use crate::error::Result;
use crate::output::format::short;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct CiArgs {
    #[command(subcommand)]
    pub command: CiCommand,
}

#[derive(Debug, Subcommand)]
pub enum CiCommand {
    /// Report the result of a CI check for a snapshot (needs ci:report)
    ///
    /// A newer report for the same snapshot and context replaces the older
    /// one. Branch protection with --require-ci blocks merges until the
    /// required contexts report success on the source tip.
    Report {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Snapshot id (or a branch / tag, resolved to its snapshot)
        #[arg(long, value_name = "ID")]
        snapshot: String,
        /// Check name, e.g. "ci/build"
        #[arg(long, value_name = "NAME")]
        context: String,
        /// Check state
        #[arg(long, value_name = "STATE")]
        state: CiState,
        /// Short description of the result
        #[arg(long, value_name = "TEXT")]
        description: Option<String>,
        /// Link to the CI run (http or https)
        #[arg(long, value_name = "URL")]
        url: Option<String>,
    },
    /// Show the combined CI state and checks of a snapshot or branch
    Status {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Snapshot id (or a branch / tag)
        #[arg(long, value_name = "ID", conflicts_with = "branch")]
        snapshot: Option<String>,
        /// Branch whose tip to show (default: the tree's default branch)
        #[arg(long, value_name = "B")]
        branch: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CiState {
    Pending,
    Success,
    Failure,
    Error,
}

impl CiState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Error => "error",
        }
    }
}

/// `domain.CIStatus`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Check {
    snapshot_id: String,
    context: String,
    state: String,
    description: String,
    target_url: String,
    reporter: String,
    updated_at: String,
}

/// `ci.Status`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Status {
    snapshot: String,
    state: String,
    #[serde(deserialize_with = "nullable")]
    checks: Vec<Check>,
}

pub fn run(ctx: &Context, args: CiArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        CiCommand::Report {
            tree,
            snapshot,
            context,
            state,
            description,
            url,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let mut body =
                json!({ "snapshot": snapshot, "context": context, "state": state.as_str() });
            if let Some(d) = description {
                body["description"] = json!(d);
            }
            if let Some(u) = url {
                body["target_url"] = json!(u);
            }
            let value = api.post(&path(&tree), &body)?;
            render(out, &value, |t, c: Check| {
                t.line(format_args!(
                    "Reported {} {} for {} on {tree}",
                    c.context.bold(),
                    state_cell(&c.state).colored(),
                    short(&c.snapshot_id).yellow()
                ));
            })
        }
        CiCommand::Status {
            tree,
            snapshot,
            branch,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let mut query = Vec::new();
            if let Some(s) = snapshot {
                query.push(("snapshot", s));
            }
            if let Some(b) = branch {
                query.push(("branch", b));
            }
            let value = api.get_query(&path(&tree), &query)?;
            render(out, &value, |t, s: Status| status(t, &s))
        }
    }
}

fn path(tree: &TreeRef) -> String {
    format!("{}/ci/status", tree.api())
}

fn state_cell(state: &str) -> Cell {
    let cell = Cell::from(if state.is_empty() { "none" } else { state });
    match state {
        "success" => cell.green(),
        "pending" => cell.yellow(),
        "failure" | "error" => cell.red(),
        _ => cell,
    }
}

fn status(t: &mut Text, s: &Status) {
    if s.snapshot.is_empty() {
        t.line("The branch has no snapshots.");
        return;
    }
    t.line(format_args!(
        "{} {}",
        short(&s.snapshot).yellow(),
        state_cell(&s.state).colored()
    ));
    let mut table = Table::new(&[
        "CONTEXT",
        "STATE",
        "DESCRIPTION",
        "REPORTER",
        "UPDATED",
        "URL",
    ]);
    for c in &s.checks {
        table.row([
            Cell::from(c.context.as_str()).bold(),
            state_cell(&c.state),
            or_dash(&c.description).into(),
            c.reporter.as_str().into(),
            when(&c.updated_at).into(),
            or_dash(&c.target_url).into(),
        ]);
    }
    table.render(t, "No checks reported.");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn ci(args: &[&str]) -> CiCommand {
        let mut full = vec!["ci"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Ci(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        let CiCommand::Report {
            tree,
            snapshot,
            context,
            state,
            description,
            url,
        } = ci(&[
            "report",
            "acme/web",
            "--snapshot",
            "abc",
            "--context",
            "ci/build",
            "--state",
            "failure",
            "--description",
            "2 failed",
            "--url",
            "https://ci/1",
        ])
        else {
            panic!()
        };
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert_eq!((snapshot.as_str(), context.as_str()), ("abc", "ci/build"));
        assert_eq!(state, CiState::Failure);
        assert_eq!(description.as_deref(), Some("2 failed"));
        assert_eq!(url.as_deref(), Some("https://ci/1"));
        assert!(matches!(
            ci(&["status"]),
            CiCommand::Status {
                tree: None,
                snapshot: None,
                branch: None
            }
        ));
        assert!(matches!(
            ci(&["status", "acme/web", "--branch", "main"]),
            CiCommand::Status {
                tree: Some(_),
                branch: Some(_),
                ..
            }
        ));
        assert_eq!(
            parse_err(&["ci", "report", "--snapshot", "a", "--context", "c"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&[
                "ci",
                "report",
                "--snapshot",
                "a",
                "--context",
                "c",
                "--state",
                "green"
            ]),
            ErrorKind::InvalidValue
        );
        assert_eq!(
            parse_err(&["ci", "status", "--snapshot", "a", "--branch", "b"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(CiState::Error.as_str(), "error");
    }

    #[test]
    fn renders_status() {
        colored::control::set_override(false);
        let s: Status = serde_json::from_value(json!({
            "snapshot": "0123456789abcdef", "state": "pending",
            "checks": [
                {"context": "build", "state": "success", "reporter": "bot", "updated_at": ""},
                {"context": "test", "state": "pending", "description": "running", "reporter": "bot",
                 "target_url": "https://ci/2", "updated_at": ""}
            ]
        }))
        .expect("status");
        let mut t = Text::default();
        status(&mut t, &s);
        assert_eq!(
            t.as_str(),
            "01234567 pending\n\
             CONTEXT  STATE    DESCRIPTION  REPORTER  UPDATED  URL\n\
             build    success  -            bot       -        -\n\
             test     pending  running      bot       -        https://ci/2\n"
        );
        let mut t = Text::default();
        status(&mut t, &Status::default());
        assert_eq!(t.as_str(), "The branch has no snapshots.\n");
        let empty: Status =
            serde_json::from_value(json!({"snapshot": "ab", "state": "", "checks": null}))
                .expect("empty");
        let mut t = Text::default();
        status(&mut t, &empty);
        assert_eq!(t.as_str(), "ab none\nNo checks reported.\n");
    }
}
