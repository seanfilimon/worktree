//! `wt search` — search tenants, trees, branches and accounts on the server.

use clap::Args;
use colored::Colorize;
use serde::Deserialize;

use crate::commands::admin::{account_table, Account};
use crate::commands::server_common::{nullable, render, when, Api, Cell, Table};
use crate::commands::tenant::{tenant_table, Tenant};
use crate::commands::trees::{Branch, Tree};
use crate::context::Context;
use crate::error::Result;
use crate::output::format::{short, subject};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Text to look for (case-insensitive substring)
    pub query: String,

    /// Maximum hits per kind (server default: 20)
    #[arg(short = 'n', value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub max_count: Option<u32>,
}

/// `SearchResults` of §11.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SearchResults {
    #[serde(deserialize_with = "nullable")]
    tenants: Vec<Tenant>,
    #[serde(deserialize_with = "nullable")]
    trees: Vec<TreeHit>,
    #[serde(deserialize_with = "nullable")]
    branches: Vec<BranchHit>,
    #[serde(deserialize_with = "nullable")]
    accounts: Vec<Account>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TreeHit {
    tree: Tree,
    tenant_slug: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BranchHit {
    tenant_slug: String,
    tree: String,
    branch: Branch,
}

pub fn run(ctx: &Context, args: SearchArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let query = [
        ("q", args.query),
        (
            "limit",
            args.max_count.map(|n| n.to_string()).unwrap_or_default(),
        ),
    ];
    let value = api.get_query("/api/v1/search", &query)?;
    render(&ctx.out, &value, |t, results: SearchResults| {
        results_text(t, &results);
    })
}

fn results_text(t: &mut Text, r: &SearchResults) {
    if r.tenants.is_empty() && r.trees.is_empty() && r.branches.is_empty() && r.accounts.is_empty()
    {
        t.line("No matches.".dimmed());
        return;
    }
    let mut first = true;
    let mut section = |t: &mut Text, title: &str| {
        if !first {
            t.blank();
        }
        first = false;
        t.line(title.bold().underline());
    };
    if !r.tenants.is_empty() {
        section(t, "Tenants");
        tenant_table(t, &r.tenants);
    }
    if !r.trees.is_empty() {
        section(t, "Trees");
        let mut table = Table::new(&["TREE", "VISIBILITY", "DESCRIPTION"]);
        for hit in &r.trees {
            table.row([
                Cell::from(format!("{}/{}", hit.tenant_slug, hit.tree.name)).bold(),
                hit.tree.visibility.as_str().into(),
                subject(&hit.tree.description).into(),
            ]);
        }
        table.render(t, "");
    }
    if !r.branches.is_empty() {
        section(t, "Branches");
        let mut table = Table::new(&["TREE", "BRANCH", "TIP", "UPDATED"]);
        for hit in &r.branches {
            table.row([
                Cell::from(format!("{}/{}", hit.tenant_slug, hit.tree)).bold(),
                Cell::from(hit.branch.name.as_str()).green(),
                Cell::from(short(&hit.branch.tip)).yellow(),
                when(&hit.branch.updated_at).into(),
            ]);
        }
        table.render(t, "");
    }
    if !r.accounts.is_empty() {
        section(t, "Accounts");
        account_table(t, &r.accounts);
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use serde_json::json;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    #[test]
    fn parses_query_and_limit() {
        let Command::Search(a) = parse(&["search", "web", "-n", "5"]) else {
            panic!()
        };
        assert_eq!((a.query.as_str(), a.max_count), ("web", Some(5)));
        let Command::Search(a) = parse(&["search", "two words"]) else {
            panic!()
        };
        assert_eq!((a.query.as_str(), a.max_count), ("two words", None));
        assert_eq!(parse_err(&["search"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(
            parse_err(&["search", "x", "-n", "0"]),
            ErrorKind::ValueValidation
        );
    }

    #[test]
    fn renders_sections() {
        colored::control::set_override(false);
        let mut t = Text::default();
        results_text(&mut t, &SearchResults::default());
        assert_eq!(t.as_str(), "No matches.\n");

        let results: SearchResults = serde_json::from_value(json!({
            "tenants": null,
            "trees": [{"tenant_slug": "acme", "tree": {"name": "web", "visibility": "private", "description": "Site"}}],
            "branches": [{"tenant_slug": "acme", "tree": "web", "branch": {"name": "main", "tip": "0123456789", "updated_at": ""}}],
            "accounts": []
        }))
        .expect("results");
        let mut t = Text::default();
        results_text(&mut t, &results);
        assert_eq!(
            t.as_str(),
            "Trees\nTREE      VISIBILITY  DESCRIPTION\nacme/web  private     Site\n\nBranches\nTREE      BRANCH  TIP       UPDATED\nacme/web  main    01234567  -\n"
        );
    }
}
