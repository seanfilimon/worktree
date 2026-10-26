//! `wt search` — search tenants, trees, branches and accounts on the
//! server; `wt search code` — search file contents (§14.3) or symbol
//! definitions (`--symbol`, §15.9) on the default branch, one branch or all
//! branches; `wt admin code-index` (implemented here, §15.9).

use std::collections::HashSet;

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;

use crate::commands::admin::{account_table, Account};
use crate::commands::server_common::{
    bytes, fields, nullable, render, when, Api, Cell, Table, TreeRef,
};
use crate::commands::tenant::{tenant_table, Tenant};
use crate::commands::trees::{Branch, Tree};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{short, subject};
use crate::output::{Output, Text};

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
    /// Search file contents or symbol definitions in trees (grep-like output)
    ///
    /// Searches every tree you may read, or the trees of one tenant
    /// (--tenant), or one tree (the global --tree TENANT/TREE), on the
    /// default branch unless --branch or --all-branches is given. Prints
    /// TENANT/TREE:PATH:LINE:TEXT per matching line (TENANT/TREE@BRANCH
    /// with a branch option); context lines use "-" instead of ":" and
    /// groups are separated by "--". With --symbol, QUERY matches the names
    /// of functions, methods, types and constants (Go, Rust, TypeScript,
    /// JavaScript, Python, Java, C/C++), best matches first: exact, then
    /// prefix, then substring. Paths you may not read (access policies,
    /// license rules) are never searched. Binary files, files over 1 MiB
    /// and chunked files are not indexed.
    #[command(
        after_help = "Examples:\n  wt search code 'fn main' --path '*.rs'\n  wt search code -E -i 'todo|fixme' --tree acme/web --context 1\n  wt search code --symbol parse --kind function,method\n  wt search code needle --all-branches"
    )]
    Code(CodeArgs),
}

/// Symbol kinds accepted by `--kind` (§15.9).
pub const SYMBOL_KINDS: [&str; 9] = [
    "function",
    "method",
    "class",
    "struct",
    "interface",
    "trait",
    "enum",
    "type",
    "const",
];

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

    /// Maximum number of matching lines or symbols (1-1000; server default 50)
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=1000))]
    pub limit: Option<u32>,

    /// Search symbol definitions whose names contain QUERY (case-insensitive) instead of lines
    #[arg(long, conflicts_with_all = ["regex", "context"])]
    pub symbol: bool,

    /// With --symbol: only these kinds (function, method, class, struct, interface, trait, enum, type, const; comma-separated or repeated)
    #[arg(long, value_name = "KIND", requires = "symbol", value_delimiter = ',', value_parser = SYMBOL_KINDS)]
    pub kind: Vec<String>,

    /// Search this branch instead of each tree's default branch
    #[arg(long, value_name = "B", conflicts_with = "all_branches")]
    pub branch: Option<String>,

    /// Search every branch tip (the server indexes up to 10 branches per tree by default)
    #[arg(long)]
    pub all_branches: bool,
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
    branch: String,
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
    push_branch_scope(&mut query, args);
    query
}

/// `branch=B` or `branches=all` (§15.9).
fn push_branch_scope(query: &mut Vec<(&'static str, String)>, args: &CodeArgs) {
    if let Some(branch) = &args.branch {
        query.push(("branch", branch.clone()));
    }
    if args.all_branches {
        query.push(("branches", "all".into()));
    }
}

/// The REST query of a symbol search (§15.9).
fn symbol_query(args: &CodeArgs, tree: Option<&TreeRef>) -> Vec<(&'static str, String)> {
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
    if !args.kind.is_empty() {
        query.push(("kind", args.kind.join(",")));
    }
    if let Some(limit) = args.limit {
        query.push(("limit", limit.to_string()));
    }
    push_branch_scope(&mut query, args);
    query
}

fn run_code(ctx: &Context, args: CodeArgs) -> Result<()> {
    if args.symbol && args.query.trim().is_empty() {
        return Err(CliError::usage("QUERY must not be empty"));
    }
    if !args.symbol && !args.regex && args.query.chars().count() < 3 {
        return Err(CliError::usage(
            "QUERY must be at least 3 characters (or use --regex or --symbol)",
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
    let show_branch = args.branch.is_some() || args.all_branches;
    if args.symbol {
        let value = api.get_query(
            "/api/v1/search/symbols",
            &symbol_query(&args, tree.as_ref()),
        )?;
        let highlight = Highlight::new(&args.query, true);
        return render(&ctx.out, &value, |t, results: SymbolResults| {
            symbol_text(t, &results, &highlight, show_branch);
        });
    }
    let value = api.get_query("/api/v1/search/code", &code_query(&args, tree.as_ref()))?;
    let highlight = (!args.regex).then(|| Highlight::new(&args.query, args.ignore_case));
    render(&ctx.out, &value, |t, results: CodeResults| {
        code_text(t, &results, highlight.as_ref(), show_branch);
    })
}

/// `{"symbols":[SymbolHit],"truncated":bool,"searched_trees":n}` (§15.9).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SymbolResults {
    #[serde(deserialize_with = "nullable")]
    symbols: Vec<SymbolHit>,
    truncated: bool,
    searched_trees: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SymbolHit {
    tenant: String,
    tree: String,
    branch: String,
    path: String,
    line: u64,
    name: String,
    kind: String,
    container: String,
    signature: String,
}

/// `TENANT/TREE` or, when branches are shown, `TENANT/TREE@BRANCH`.
fn tree_label(tenant: &str, tree: &str, branch: &str, show_branch: bool) -> String {
    if show_branch && !branch.is_empty() {
        format!("{tenant}/{tree}@{branch}")
    } else {
        format!("{tenant}/{tree}")
    }
}

/// One line per symbol: `TENANT/TREE:PATH:LINE: KIND NAME  SIGNATURE`.
fn symbol_text(t: &mut Text, r: &SymbolResults, highlight: &Highlight, show_branch: bool) {
    if r.symbols.is_empty() {
        t.line("No symbols found.".dimmed());
    }
    let width = r.symbols.iter().map(|s| s.kind.len()).max().unwrap_or(0);
    for s in &r.symbols {
        let name = highlight.apply(&s.name);
        let qualified = if s.container.is_empty() {
            name
        } else {
            format!("{}.{name}", s.container)
        };
        t.line(format!(
            "{}{}{}{} {:<width$} {}  {}",
            tree_label(&s.tenant, &s.tree, &s.branch, show_branch).magenta(),
            format!(":{}", s.path).magenta(),
            ":".cyan(),
            format!("{}:", s.line).green(),
            s.kind.yellow().to_string(),
            qualified.bold(),
            s.signature.dimmed(),
            width = width
        ));
    }
    if r.truncated {
        t.line(
            format!(
                "(showing the best {} symbols in {} trees; narrow the query or raise --limit)",
                r.symbols.len(),
                r.searched_trees
            )
            .dimmed(),
        );
    }
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

fn code_text(t: &mut Text, r: &CodeResults, highlight: Option<&Highlight>, show_branch: bool) {
    if r.hits.is_empty() {
        t.line("No matches.".dimmed());
    }
    let with_context = r
        .hits
        .iter()
        .any(|h| !h.before.is_empty() || !h.after.is_empty());
    // Match lines are printed as matches, never as another hit's context.
    let matches: HashSet<(&str, &str, &str, &str, u64)> = r
        .hits
        .iter()
        .map(|h| {
            (
                h.tenant.as_str(),
                h.tree.as_str(),
                h.branch.as_str(),
                h.path.as_str(),
                h.line,
            )
        })
        .collect();
    // (tree, path, last printed line) of the previous group.
    let mut last: Option<(String, &str, u64)> = None;
    for hit in &r.hits {
        let tree = tree_label(&hit.tenant, &hit.tree, &hit.branch, show_branch);
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
            if matches.contains(&(
                hit.tenant.as_str(),
                hit.tree.as_str(),
                hit.branch.as_str(),
                hit.path.as_str(),
                n,
            )) {
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

// ---------------------------------------------------------------------------
// wt admin code-index (§15.9)

/// `wt admin code-index [rebuild [TENANT/TREE]]`.
#[derive(Debug, Args)]
pub struct CodeIndexArgs {
    #[command(subcommand)]
    pub command: Option<CodeIndexCommand>,
}

#[derive(Debug, Subcommand)]
pub enum CodeIndexCommand {
    /// Discard a tree's code search index and rebuild it from stored
    /// objects (every indexed tree when TREE is omitted)
    Rebuild {
        /// TENANT/TREE to rebuild
        #[arg(value_name = "TREE", id = "tree_ref")]
        tree: Option<TreeRef>,
    },
}

const CODE_INDEX: &str = "/api/v1/admin/code-index";

/// `codesearch.IndexStatus`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct IndexStatus {
    format_version: u64,
    persistent: bool,
    memory_bytes: u64,
    memory_limit: u64,
    loaded_segments: u64,
    evictions: u64,
    disk_bytes: u64,
    max_branches: u64,
    builds: u64,
    disk_loads: u64,
    #[serde(deserialize_with = "nullable")]
    trees: Vec<IndexTree>,
}

/// `codesearch.CodeIndexTree`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct IndexTree {
    tenant: String,
    tree: String,
    state: String,
    files: u64,
    symbols: u64,
    built_at: String,
    branch_count: u64,
    segments: u64,
    memory_bytes: u64,
    disk_bytes: u64,
    error: String,
}

/// `codesearch.RebuildResult`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RebuildResult {
    rebuilt: u64,
    #[serde(deserialize_with = "nullable")]
    trees: Vec<IndexTree>,
}

/// Runs `wt admin code-index` (called from `commands/admin.rs`).
pub fn run_code_index(api: &Api, out: &Output, args: CodeIndexArgs) -> Result<()> {
    match args.command {
        None => {
            let value = api.get(CODE_INDEX)?;
            render(out, &value, |t, s: IndexStatus| index_status_text(t, &s))
        }
        Some(CodeIndexCommand::Rebuild { tree }) => {
            let path = match &tree {
                Some(tree) => format!(
                    "{CODE_INDEX}/rebuild?tree={}",
                    worktree_sdk::remote::rest::seg(&tree.to_string())
                ),
                None => format!("{CODE_INDEX}/rebuild"),
            };
            let value = api.post(&path, &serde_json::json!({}))?;
            render(out, &value, |t, r: RebuildResult| {
                t.line(format_args!(
                    "Rebuilt the code index of {} {}",
                    r.rebuilt,
                    if r.rebuilt == 1 { "tree" } else { "trees" }
                ));
                index_tree_table(t, &r.trees);
            })
        }
    }
}

fn index_status_text(t: &mut Text, s: &IndexStatus) {
    let limit = if s.memory_limit == 0 {
        "unlimited".to_string()
    } else {
        bytes(s.memory_limit)
    };
    fields(
        t,
        &[
            (
                "format",
                format!(
                    "{} ({})",
                    s.format_version,
                    if s.persistent {
                        "on disk"
                    } else {
                        "in memory only"
                    }
                ),
            ),
            (
                "memory",
                format!(
                    "{} of {limit} ({} segments loaded, {} evicted)",
                    bytes(s.memory_bytes),
                    s.loaded_segments,
                    s.evictions
                ),
            ),
            ("disk", bytes(s.disk_bytes)),
            (
                "since start",
                format!("{} built, {} loaded from disk", s.builds, s.disk_loads),
            ),
            ("branches", format!("up to {} per tree", s.max_branches)),
        ],
    );
    t.blank();
    index_tree_table(t, &s.trees);
}

fn index_tree_table(t: &mut Text, trees: &[IndexTree]) {
    let mut table = Table::new(&[
        "TREE", "STATE", "BRANCHES", "FILES", "SYMBOLS", "MEMORY", "DISK", "BUILT",
    ]);
    for tr in trees {
        let state = match tr.state.as_str() {
            "ready" => Cell::from("ready").green(),
            "building" => Cell::from("building").yellow(),
            other if !tr.error.is_empty() => Cell::from(format!("{other}: {}", tr.error)).red(),
            other => Cell::from(other),
        };
        let branches = if tr.segments != tr.branch_count {
            format!("{} ({} segments)", tr.branch_count, tr.segments)
        } else {
            tr.branch_count.to_string()
        };
        table.row([
            Cell::from(format!("{}/{}", tr.tenant, tr.tree)).bold(),
            state,
            branches.into(),
            tr.files.to_string().into(),
            tr.symbols.to_string().into(),
            bytes(tr.memory_bytes).into(),
            bytes(tr.disk_bytes).into(),
            when(&tr.built_at).into(),
        ]);
    }
    table.render(t, "No tree has been indexed yet.");
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
        code_text(&mut t, &results, highlight, false);
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

    // --- search-v2 (§15.9) ---

    #[test]
    fn parses_symbol_and_branch_options() {
        let c = code(&[
            "parse",
            "--symbol",
            "--kind",
            "function,method",
            "--kind",
            "const",
        ]);
        assert!(c.symbol);
        assert_eq!(c.kind, vec!["function", "method", "const"]);
        assert_eq!(
            symbol_query(&c, None),
            vec![
                ("q", "parse".to_string()),
                ("kind", "function,method,const".into())
            ]
        );
        let c = code(&[
            "Parse", "--symbol", "--tenant", "acme", "--path", "src", "-n", "5", "--branch", "dev",
        ]);
        let tree: TreeRef = "acme/web".parse().unwrap();
        assert_eq!(
            symbol_query(&c, Some(&tree)),
            vec![
                ("q", "Parse".to_string()),
                ("tree", "acme/web".into()),
                ("tenant", "acme".into()),
                ("path", "src".into()),
                ("limit", "5".into()),
                ("branch", "dev".into()),
            ]
        );
        let c = code(&["needle", "--all-branches"]);
        assert_eq!(
            code_query(&c, None),
            vec![
                ("q", "needle".to_string()),
                ("case", "true".into()),
                ("context", "0".into()),
                ("branches", "all".into())
            ]
        );
        for bad in [
            &["search", "code", "x", "--kind", "function"][..],
            &["search", "code", "x", "--symbol", "--kind", "widget"],
            &["search", "code", "x", "--symbol", "--regex"],
            &["search", "code", "x", "--symbol", "--context", "1"],
            &["search", "code", "x", "--branch", "dev", "--all-branches"],
        ] {
            let kind = parse_err(bad);
            assert!(
                matches!(
                    kind,
                    ErrorKind::MissingRequiredArgument
                        | ErrorKind::InvalidValue
                        | ErrorKind::ArgumentConflict
                ),
                "{bad:?}: {kind:?}"
            );
        }
    }

    #[test]
    fn renders_symbols() {
        colored::control::set_override(false);
        let results: SymbolResults = serde_json::from_value(json!({
            "symbols": [
                {"tenant": "acme", "tree": "web", "branch": "main", "snapshot_id": "s", "path": "src/lib.rs", "line": 3,
                 "name": "parse", "kind": "function", "signature": "pub fn parse(s: &str)", "language": "rust", "match": "exact"},
                {"tenant": "acme", "tree": "web", "branch": "dev", "snapshot_id": "s", "path": "a.go", "line": 9,
                 "name": "Parse", "kind": "method", "container": "Parser", "signature": "func (p *Parser) Parse()", "language": "go", "match": "exact"}
            ],
            "truncated": true, "searched_trees": 2
        }))
        .expect("symbols");
        let h = Highlight::new("parse", true);
        let mut t = Text::default();
        symbol_text(&mut t, &results, &h, false);
        assert_eq!(
            t.as_str(),
            "acme/web:src/lib.rs:3: function parse  pub fn parse(s: &str)\n\
             acme/web:a.go:9: method   Parser.Parse  func (p *Parser) Parse()\n\
             (showing the best 2 symbols in 2 trees; narrow the query or raise --limit)\n"
        );
        let mut t = Text::default();
        symbol_text(&mut t, &results, &h, true);
        assert!(
            t.as_str().starts_with("acme/web@main:src/lib.rs:3:"),
            "{}",
            t.as_str()
        );
        assert!(
            t.as_str().contains("acme/web@dev:a.go:9:"),
            "{}",
            t.as_str()
        );
        let mut t = Text::default();
        symbol_text(&mut t, &SymbolResults::default(), &h, false);
        assert_eq!(t.as_str(), "No symbols found.\n");

        // Code hits show the branch when a branch option was given.
        let v = json!({"hits": [hit("web", "a.txt", 1, "x", &[], &[])], "truncated": false, "searched_trees": 1});
        let results: CodeResults = serde_json::from_value(v).unwrap();
        let mut t = Text::default();
        code_text(&mut t, &results, None, true);
        assert_eq!(t.as_str(), "acme/web@main:a.txt:1:x\n");
    }

    #[test]
    fn parses_and_renders_code_index_admin() {
        use crate::commands::admin::{AdminArgs, AdminCommand};
        let Command::Admin(AdminArgs {
            command: AdminCommand::CodeIndex(a),
        }) = parse(&["admin", "code-index"])
        else {
            panic!()
        };
        assert!(a.command.is_none());
        let Command::Admin(AdminArgs {
            command: AdminCommand::CodeIndex(a),
        }) = parse(&["admin", "code-index", "rebuild", "acme/web"])
        else {
            panic!()
        };
        let Some(CodeIndexCommand::Rebuild { tree: Some(tree) }) = a.command else {
            panic!()
        };
        assert_eq!(tree.to_string(), "acme/web");
        assert!(matches!(
            parse(&["admin", "code-index", "rebuild"]),
            Command::Admin(AdminArgs {
                command: AdminCommand::CodeIndex(CodeIndexArgs {
                    command: Some(CodeIndexCommand::Rebuild { tree: None })
                })
            })
        ));
        assert_eq!(
            parse_err(&["admin", "code-index", "rebuild", "notatree"]),
            ErrorKind::ValueValidation
        );

        colored::control::set_override(false);
        let status: IndexStatus = serde_json::from_value(json!({
            "format_version": 1, "persistent": true, "memory_bytes": 2048, "memory_limit": 268435456,
            "loaded_segments": 1, "evictions": 0, "disk_bytes": 4096, "max_branches": 10, "builds": 0,
            "disk_loads": 1,
            "trees": [{"tree_id": "t1", "tenant": "acme", "tree": "web", "default_branch": "main", "state": "ready",
                       "files": 12, "bytes": 100, "symbols": 40, "trigrams": 9, "built_at": "", "branch_count": 2,
                       "segments": 1, "memory_bytes": 2048, "disk_bytes": 4096, "branches": null}]
        }))
        .unwrap();
        let mut t = Text::default();
        index_status_text(&mut t, &status);
        let out = t.as_str();
        for want in [
            "1 (on disk)",
            "2.0 KiB of 256.0 MiB (1 segments loaded, 0 evicted)",
            "0 built, 1 loaded from disk",
            "up to 10 per tree",
            "acme/web",
            "2 (1 segments)",
        ] {
            assert!(out.contains(want), "missing {want:?} in\n{out}");
        }
        let mut t = Text::default();
        index_tree_table(&mut t, &[]);
        assert_eq!(t.as_str(), "No tree has been indexed yet.\n");
    }
}
