//! `wt search` — search tenants, trees, branches and accounts on the
//! server; `wt search code` — search file contents (§14.3).

use std::collections::HashSet;

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;

use crate::commands::admin::{account_table, Account};
use crate::commands::server_common::{nullable, render, when, Api, Cell, Table, TreeRef};
use crate::commands::tenant::{tenant_table, Tenant};
use crate::commands::trees::{Branch, Tree};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{short, subject};
use crate::output::Text;

/// `wt search QUERY` searches names; `wt search code QUERY` searches the
/// contents of files on the default branch of every tree you may read.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub struct SearchArgs {
    #[command(subcommand)]
    pub command: Option<SearchCommand>,

    /// Text to look for (case-insensitive substring)
    #[arg(required = true)]
    pub query: Option<String>,

    /// Maximum hits per kind (server default: 20)
    #[arg(short = 'n', value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub max_count: Option<u32>,
}

#[derive(Debug, Subcommand)]
pub enum SearchCommand {
    /// Search file contents on the default branch of trees (grep-like output)
    ///
    /// Searches every tree you may read, or the trees of one tenant
    /// (--tenant), or one tree (the global --tree TENANT/TREE). Prints
    /// TENANT/TREE:PATH:LINE:TEXT per matching line; context lines use "-"
    /// instead of ":" and groups are separated by "--". Paths you may not
    /// read (access policies, license rules) are never searched. Binary
    /// files, files over 1 MiB and chunked files are not indexed.
    #[command(
        after_help = "Examples:\n  wt search code 'fn main' --path '*.rs'\n  wt search code -E -i 'todo|fixme' --tree acme/web --context 1"
    )]
    Code(CodeArgs),
}

#[derive(Debug, Args)]
pub struct CodeArgs {
    /// Text to look for (at least 3 characters), or with --regex an RE2 regular expression
    pub query: String,

    /// Search only the trees of this tenant
    #[arg(long, value_name = "T")]
    pub tenant: Option<String>,

    /// Search only files matching this glob ("*.go" matches at any depth; "src/**/*.rs"; a directory covers its files)
    #[arg(long, value_name = "GLOB")]
    pub path: Option<String>,

    /// Treat QUERY as a regular expression (RE2 syntax, matched per line)
    #[arg(short = 'E', long)]
    pub regex: bool,

    /// Ignore case (Unicode case folding)
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Lines of context before and after each match (0-10; default 0)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(0..=10))]
    pub context: Option<u8>,

    /// Maximum number of matching lines (1-1000; server default 50)
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=1000))]
    pub limit: Option<u32>,
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
    match args.command {
        Some(SearchCommand::Code(code)) => run_code(ctx, code),
        None => run_names(ctx, args.query.unwrap_or_default(), args.max_count),
    }
}

fn run_names(ctx: &Context, query: String, max_count: Option<u32>) -> Result<()> {
    let api = Api::connect(ctx)?;
    let query = [
        ("q", query),
        (
            "limit",
            max_count.map(|n| n.to_string()).unwrap_or_default(),
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

// ---------------------------------------------------------------------------
// wt search code

/// `{"hits":[CodeSearchHit],"truncated":bool,"searched_trees":n}` (§14.3).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CodeResults {
    #[serde(deserialize_with = "nullable")]
    hits: Vec<CodeHit>,
    truncated: bool,
    searched_trees: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CodeHit {
    tenant: String,
    tree: String,
    path: String,
    line: u64,
    text: String,
    #[serde(deserialize_with = "nullable")]
    before: Vec<String>,
    #[serde(deserialize_with = "nullable")]
    after: Vec<String>,
}

/// The REST query of a code search.
fn code_query(args: &CodeArgs, tree: Option<&TreeRef>) -> Vec<(&'static str, String)> {
    let mut query = vec![("q", args.query.clone())];
    if let Some(tree) = tree {
        query.push(("tree", tree.to_string()));
    }
    if let Some(tenant) = &args.tenant {
        query.push(("tenant", tenant.clone()));
    }
    if let Some(path) = &args.path {
        query.push(("path", path.clone()));
    }
    if args.regex {
        query.push(("regex", "true".into()));
    }
    // grep semantics: case-sensitive unless -i.
    query.push(("case", (!args.ignore_case).to_string()));
    query.push(("context", args.context.unwrap_or(0).to_string()));
    if let Some(limit) = args.limit {
        query.push(("limit", limit.to_string()));
    }
    query
}

fn run_code(ctx: &Context, args: CodeArgs) -> Result<()> {
    if !args.regex && args.query.chars().count() < 3 {
        return Err(CliError::usage(
            "QUERY must be at least 3 characters (or use --regex)",
        ));
    }
    let tree = ctx
        .tree_flag()
        .map(|spec| {
            spec.parse::<TreeRef>().map_err(|e| {
                CliError::usage(format!("--tree: {e}"))
                    .with_hint("wt search code takes --tree TENANT/TREE")
            })
        })
        .transpose()?;
    if let (Some(tree), Some(tenant)) = (&tree, &args.tenant) {
        if &tree.tenant != tenant {
            return Err(CliError::usage(format!(
                "--tree {tree} is not in --tenant {tenant}"
            )));
        }
    }
    let api = Api::connect(ctx)?;
    let value = api.get_query("/api/v1/search/code", &code_query(&args, tree.as_ref()))?;
    let highlight = (!args.regex).then(|| Highlight::new(&args.query, args.ignore_case));
    render(&ctx.out, &value, |t, results: CodeResults| {
        code_text(t, &results, highlight.as_ref());
    })
}

/// Marks occurrences of a literal query in output lines.
struct Highlight {
    needle: String,
    fold: bool,
}

impl Highlight {
    fn new(needle: &str, ignore_case: bool) -> Self {
        Self {
            needle: needle.to_string(),
            // ASCII folding keeps byte offsets; other case-insensitive
            // queries are shown without highlighting.
            fold: ignore_case,
        }
    }

    fn apply(&self, text: &str) -> String {
        self.apply_with(text, |m| m.red().bold().to_string())
    }

    fn apply_with(&self, text: &str, mark: impl Fn(&str) -> String) -> String {
        if self.needle.is_empty() || (self.fold && !self.needle.is_ascii()) {
            return text.to_string();
        }
        let hay = if self.fold {
            text.to_ascii_lowercase()
        } else {
            text.to_string()
        };
        let needle = if self.fold {
            self.needle.to_ascii_lowercase()
        } else {
            self.needle.clone()
        };
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        while let Some(found) = hay[at..].find(&needle) {
            let start = at + found;
            let end = start + needle.len();
            out.push_str(&text[at..start]);
            out.push_str(&mark(&text[start..end]));
            at = end;
        }
        out.push_str(&text[at..]);
        out
    }
}

fn code_text(t: &mut Text, r: &CodeResults, highlight: Option<&Highlight>) {
    if r.hits.is_empty() {
        t.line("No matches.".dimmed());
    }
    let with_context = r
        .hits
        .iter()
        .any(|h| !h.before.is_empty() || !h.after.is_empty());
    // Match lines are printed as matches, never as another hit's context.
    let matches: HashSet<(&str, &str, &str, u64)> = r
        .hits
        .iter()
        .map(|h| (h.tenant.as_str(), h.tree.as_str(), h.path.as_str(), h.line))
        .collect();
    // (tree, path, last printed line) of the previous group.
    let mut last: Option<(String, &str, u64)> = None;
    for hit in &r.hits {
        let tree = format!("{}/{}", hit.tenant, hit.tree);
        let first = hit.line.saturating_sub(hit.before.len() as u64);
        let printed = match &last {
            Some((lt, lp, ll)) if *lt == tree && *lp == hit.path => *ll,
            _ => 0,
        };
        if with_context && last.is_some() && (printed == 0 || first > printed + 1) {
            t.line("--".cyan());
        }
        let prefix = format!("{}{}", tree.magenta(), format!(":{}", hit.path).magenta());
        let context_line = |t: &mut Text, n: u64, text: &str| {
            t.line(
                format!(
                    "{prefix}{}{}{}",
                    "-".cyan(),
                    n.to_string().green(),
                    "-".cyan()
                ) + text,
            );
        };
        for (i, text) in hit.before.iter().enumerate() {
            let n = first + i as u64;
            if n > printed {
                context_line(t, n, text);
            }
        }
        if hit.line > printed {
            let text = highlight.map_or_else(|| hit.text.clone(), |h| h.apply(&hit.text));
            t.line(format!(
                "{prefix}{}{}{}{text}",
                ":".cyan(),
                hit.line.to_string().green(),
                ":".cyan()
            ));
        }
        let mut end = hit.line.max(printed);
        for (i, text) in hit.after.iter().enumerate() {
            let n = hit.line + 1 + i as u64;
            if matches.contains(&(hit.tenant.as_str(), hit.tree.as_str(), hit.path.as_str(), n)) {
                break; // the next hit prints it
            }
            if n > end {
                context_line(t, n, text);
                end = n;
            }
        }
        last = Some((tree, hit.path.as_str(), end));
    }
    if r.truncated {
        t.line(
            format!(
                "(showing the first {} matches in {} trees; narrow the query or raise --limit)",
                r.hits.len(),
                r.searched_trees
            )
            .dimmed(),
        );
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use serde_json::{json, Value};

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    #[test]
    fn parses_query_and_limit() {
        let Command::Search(a) = parse(&["search", "web", "-n", "5"]) else {
            panic!()
        };
        assert!(a.command.is_none());
        assert_eq!((a.query.as_deref(), a.max_count), (Some("web"), Some(5)));
        let Command::Search(a) = parse(&["search", "two words"]) else {
            panic!()
        };
        assert_eq!((a.query.as_deref(), a.max_count), (Some("two words"), None));
        assert_eq!(parse_err(&["search"]), ErrorKind::MissingRequiredArgument);
        assert_eq!(
            parse_err(&["search", "x", "-n", "0"]),
            ErrorKind::ValueValidation
        );
    }

    fn code(args: &[&str]) -> CodeArgs {
        let mut argv = vec!["search", "code"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Search(SearchArgs {
                command: Some(SearchCommand::Code(c)),
                query: None,
                ..
            }) => c,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_code_search() {
        let c = code(&["fn main"]);
        assert_eq!(c.query, "fn main");
        assert!(!c.regex && !c.ignore_case);
        assert_eq!((c.context, c.limit), (None, None));
        assert_eq!(
            code_query(&c, None),
            vec![
                ("q", "fn main".to_string()),
                ("case", "true".into()),
                ("context", "0".into())
            ]
        );

        let c = code(&[
            "-E",
            "-i",
            "todo|fixme",
            "--tenant",
            "acme",
            "--path",
            "*.rs",
            "--context",
            "3",
            "-n",
            "7",
        ]);
        assert!(c.regex && c.ignore_case);
        let tree: TreeRef = "acme/web".parse().unwrap();
        assert_eq!(
            code_query(&c, Some(&tree)),
            vec![
                ("q", "todo|fixme".to_string()),
                ("tree", "acme/web".into()),
                ("tenant", "acme".into()),
                ("path", "*.rs".into()),
                ("regex", "true".into()),
                ("case", "false".into()),
                ("context", "3".into()),
                ("limit", "7".into()),
            ]
        );
        let c = code(&["x", "--regex", "--limit", "1000", "--ignore-case"]);
        assert!(c.regex && c.ignore_case && c.limit == Some(1000));

        // The global --tree flag selects the tree.
        let cli = <crate::cli::Cli as clap::Parser>::try_parse_from([
            "wt", "search", "code", "abc", "--tree", "acme/web",
        ])
        .unwrap();
        assert_eq!(cli.global.tree.as_deref(), Some("acme/web"));

        assert_eq!(
            parse_err(&["search", "code"]),
            ErrorKind::MissingRequiredArgument
        );
        for bad in [
            &["search", "code", "abc", "--context", "11"][..],
            &["search", "code", "abc", "-n", "0"],
            &["search", "code", "abc", "--limit", "1001"],
        ] {
            assert_eq!(parse_err(bad), ErrorKind::ValueValidation, "{bad:?}");
        }
        // Name-search arguments do not mix with the subcommand.
        assert_eq!(
            parse_err(&["search", "web", "code"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["search", "-n", "5", "code", "abc"]),
            ErrorKind::ArgumentConflict
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

    fn hit(
        tree: &str,
        path: &str,
        line: u64,
        text: &str,
        before: &[&str],
        after: &[&str],
    ) -> Value {
        json!({
            "tenant": "acme", "tree": tree, "branch": "main", "snapshot_id": "s", "path": path,
            "line": line, "text": text, "before": before, "after": after,
        })
    }

    fn render_code(v: Value, highlight: Option<&Highlight>) -> String {
        colored::control::set_override(false);
        let results: CodeResults = serde_json::from_value(v).expect("results");
        let mut t = Text::default();
        code_text(&mut t, &results, highlight);
        t.as_str().to_string()
    }

    #[test]
    fn renders_grep_like_lines() {
        let out = render_code(
            json!({"hits": [hit("web", "src/a.rs", 3, "fn main() {", &[], &[]),
                            hit("api", "b.go", 10, "func main() {", &[], &[])],
                   "truncated": false, "searched_trees": 2}),
            None,
        );
        assert_eq!(
            out,
            "acme/web:src/a.rs:3:fn main() {\nacme/api:b.go:10:func main() {\n"
        );
        assert_eq!(
            render_code(
                json!({"hits": null, "truncated": false, "searched_trees": 0}),
                None
            ),
            "No matches.\n"
        );
    }

    #[test]
    fn renders_context_groups_without_repeating_lines() {
        let out = render_code(
            json!({"hits": [
                hit("web", "a.txt", 3, "m3", &["l1", "l2"], &["l4", "m5"]),
                hit("web", "a.txt", 5, "m5", &["m3", "l4"], &["l6"]),
                hit("web", "a.txt", 20, "m20", &["l19"], &[]),
                hit("web", "b.txt", 1, "m1", &[], &["l2"]),
            ], "truncated": true, "searched_trees": 1}),
            None,
        );
        assert_eq!(
            out,
            "acme/web:a.txt-1-l1\nacme/web:a.txt-2-l2\nacme/web:a.txt:3:m3\nacme/web:a.txt-4-l4\n\
             acme/web:a.txt:5:m5\nacme/web:a.txt-6-l6\n--\nacme/web:a.txt-19-l19\nacme/web:a.txt:20:m20\n\
             --\nacme/web:b.txt:1:m1\nacme/web:b.txt-2-l2\n\
             (showing the first 4 matches in 1 trees; narrow the query or raise --limit)\n"
        );
    }

    #[test]
    fn highlights_literal_matches() {
        let mark = |m: &str| format!("[{m}]");
        let h = Highlight::new("Main", true);
        assert_eq!(h.apply_with("main and MAIN!", mark), "[main] and [MAIN]!");
        let h = Highlight::new("Main", false);
        assert_eq!(h.apply_with("main Main", mark), "main [Main]");
        assert_eq!(h.apply_with("ééMain", mark), "éé[Main]");
        // Non-ASCII case-insensitive needles are not highlighted.
        let h = Highlight::new("Été", true);
        assert_eq!(h.apply_with("été", mark), "été");
    }
}
