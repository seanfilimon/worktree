//! `wt diff` and the shared patch / diffstat renderers.

use std::path::PathBuf;

use clap::Args;
use colored::Colorize;
use worktree_sdk::{ChangeKind, DiffOptions, FileDiff};

use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{plural, stat_bar};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct DiffArgs {
    /// Up to two revisions followed by paths. With no revision the working
    /// tree is compared with HEAD; with one, with that revision; with two,
    /// the revisions are compared with each other.
    #[arg(value_name = "REV|PATH")]
    pub args: Vec<String>,

    /// Paths to restrict the diff to (everything after `--`)
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Show only the names of changed files
    #[arg(long, conflicts_with_all = ["stat", "name_status"])]
    pub name_only: bool,

    /// Show each changed file's status letter and name (A, M, D, or
    /// R<similarity> OLD NEW for renames)
    #[arg(long, conflicts_with = "stat")]
    pub name_status: bool,

    /// Show a diffstat instead of patches
    #[arg(long)]
    pub stat: bool,

    /// Detect renames: deleted/added files with identical content, or at
    /// least PCT percent similar lines (default 50), are shown as
    /// `renamed: OLD -> NEW`
    #[arg(
        short = 'M',
        long,
        value_name = "PCT",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "50",
        value_parser = parse_percent
    )]
    pub find_renames: Option<u8>,

    /// Show changed words inline (`[-removed-]{+added+}`) instead of whole
    /// changed lines
    #[arg(long, conflicts_with_all = ["name_only", "name_status", "stat"])]
    pub word_diff: bool,
}

/// `50`, `50%` -> 50 (0..=100).
fn parse_percent(value: &str) -> std::result::Result<u8, String> {
    let digits = value.trim().trim_end_matches('%');
    match digits.parse::<u8>() {
        Ok(n) if n <= 100 => Ok(n),
        _ => Err(format!(
            "expected a percentage between 0 and 100, got {value:?}"
        )),
    }
}

pub fn run(ctx: &Context, args: DiffArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let (revs, mut paths) = split_revs_and_paths(
        args.args,
        |arg| repo.resolve(&tree.name, arg).is_ok(),
        |arg| ctx.absolute(arg.as_ref()).exists(),
    )?;
    paths.extend(args.paths);
    let opts = DiffOptions {
        name_only: args.name_only || args.name_status,
        paths: ctx.tree_paths(&repo, &tree, &paths)?,
        find_renames: args.find_renames,
        word_diff: args.word_diff,
        ..DiffOptions::default()
    };
    let files = match revs.as_slice() {
        [] => repo.diff_working(&tree.name, None, &opts)?,
        [rev] => repo.diff_working(&tree.name, Some(rev), &opts)?,
        [from, to] => repo.diff_revs(&tree.name, from, to, &opts)?,
        _ => unreachable!("split_revs_and_paths yields at most two revisions"),
    };
    ctx.out.emit(&files, |t| {
        if args.name_only {
            for file in &files {
                t.line(&file.path);
            }
        } else if args.name_status {
            render_name_status(t, &files);
        } else if args.stat {
            render_stat(t, &files);
        } else if args.word_diff {
            render_word_patches(t, &files);
        } else {
            render_patches(t, &files);
        }
    })
}

/// `M\tpath`, `A\tpath`, `D\tpath`, `R087\told\tnew` lines.
pub fn render_name_status(t: &mut Text, files: &[FileDiff]) {
    for file in files {
        match &file.old_path {
            Some(old) => t.line(format_args!(
                "R{:03}\t{old}\t{}",
                file.similarity.unwrap_or(100),
                file.path
            )),
            None => t.line(format_args!("{}\t{}", status_letter(file.kind), file.path)),
        }
    }
}

fn status_letter(kind: ChangeKind) -> char {
    match kind {
        ChangeKind::Added => 'A',
        ChangeKind::Modified => 'M',
        ChangeKind::Deleted => 'D',
    }
}

/// `old -> new` for renames, else the path.
fn display_path(file: &FileDiff) -> String {
    match &file.old_path {
        Some(old) => format!("{old} => {}", file.path),
        None => file.path.clone(),
    }
}

/// Git-style disambiguation of `wt diff` positionals: leading arguments
/// that name revisions (at most two) are revisions; the rest must be
/// existing paths. Use `--` to name paths that do not exist.
fn split_revs_and_paths(
    args: Vec<String>,
    is_rev: impl Fn(&str) -> bool,
    exists: impl Fn(&str) -> bool,
) -> Result<(Vec<String>, Vec<PathBuf>)> {
    let mut revs = Vec::new();
    let mut paths = Vec::new();
    for arg in args {
        if paths.is_empty() && revs.len() < 2 && is_rev(&arg) {
            revs.push(arg);
        } else if exists(&arg) {
            paths.push(PathBuf::from(arg));
        } else {
            return Err(CliError::failure(format!(
                "unknown revision or path not in the working tree: '{arg}'"
            ))
            .with_hint(
                "use `--` to separate paths from revisions: `wt diff [REV [REV2]] -- PATH...`",
            ));
        }
    }
    Ok((revs, paths))
}

/// Colored unified diffs.
pub fn render_patches(t: &mut Text, files: &[FileDiff]) {
    render_files(t, files, |line| colorize_patch_line(line).to_string());
}

/// Colored word diffs (patches produced with `DiffOptions::word_diff`).
pub fn render_word_patches(t: &mut Text, files: &[FileDiff]) {
    render_files(t, files, colorize_word_line);
}

fn render_files(t: &mut Text, files: &[FileDiff], colorize: impl Fn(&str) -> String) {
    for file in files {
        let old = file.old_path.as_deref().unwrap_or(&file.path);
        t.line(format!("diff --wt a/{old} b/{}", file.path).bold());
        if let Some(old) = &file.old_path {
            t.line(
                format!(
                    "renamed: {old} -> {} (similarity {}%)",
                    file.path,
                    file.similarity.unwrap_or(100)
                )
                .bold(),
            );
        } else {
            match file.kind {
                ChangeKind::Added => t.line("new file".bold()),
                ChangeKind::Deleted => t.line("deleted file".bold()),
                ChangeKind::Modified => {}
            }
        }
        if file.binary {
            t.line(format_args!(
                "Binary files a/{old} and b/{} differ",
                file.path
            ));
            continue;
        }
        for line in file.patch.as_deref().unwrap_or_default().lines() {
            t.line(colorize(line));
        }
    }
}

/// Word-diff line: headers as in patches, `[-x-]` red, `{+x+}` green.
fn colorize_word_line(line: &str) -> String {
    if line.starts_with("+++ ") || line.starts_with("--- ") {
        return line.bold().to_string();
    }
    if line.starts_with("@@") {
        return line.cyan().to_string();
    }
    word_segments(line)
        .into_iter()
        .map(|(mark, text)| match mark {
            WordMark::Removed => text.red().to_string(),
            WordMark::Added => text.green().to_string(),
            WordMark::Plain => text.to_string(),
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordMark {
    Plain,
    Removed,
    Added,
}

/// Splits a word-diff line into plain text and `[-...-]` / `{+...+}`
/// segments (markers included). Unterminated markers are plain text.
fn word_segments(line: &str) -> Vec<(WordMark, &str)> {
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        let next = [
            ("[-", "-]", WordMark::Removed),
            ("{+", "+}", WordMark::Added),
        ]
        .into_iter()
        .filter_map(|(open, close, mark)| rest.find(open).map(|i| (i, open, close, mark)))
        .min_by_key(|(i, ..)| *i);
        let Some((start, open, close, mark)) = next else {
            break;
        };
        let body_start = start + open.len();
        let Some(len) = rest[body_start..].find(close) else {
            break;
        };
        let end = body_start + len + close.len();
        if start > 0 {
            out.push((WordMark::Plain, &rest[..start]));
        }
        out.push((mark, &rest[start..end]));
        rest = &rest[end..];
    }
    if !rest.is_empty() {
        out.push((WordMark::Plain, rest));
    }
    out
}

fn colorize_patch_line(line: &str) -> colored::ColoredString {
    if line.starts_with("+++") || line.starts_with("---") {
        line.bold()
    } else if line.starts_with('+') {
        line.green()
    } else if line.starts_with('-') {
        line.red()
    } else if line.starts_with("@@") {
        line.cyan()
    } else if line.starts_with('\\') {
        line.dimmed()
    } else {
        line.normal()
    }
}

/// Maximum width of the `+++---` bar in `--stat` output.
const STAT_BAR_WIDTH: usize = 40;

/// ` path | 12 +++++-------` lines plus a summary.
pub fn render_stat(t: &mut Text, files: &[FileDiff]) {
    let paths: Vec<String> = files.iter().map(display_path).collect();
    let path_width = paths.iter().map(|p| p.chars().count()).max().unwrap_or(0);
    let max_total = files.iter().map(|f| f.added + f.removed).max().unwrap_or(0);
    let count_width = max_total.to_string().len();
    let (mut added, mut removed) = (0, 0);
    for (file, path) in files.iter().zip(&paths) {
        added += file.added;
        removed += file.removed;
        if file.binary {
            t.line(format_args!(
                " {:<path_width$} | {:>count_width$}",
                path, "Bin"
            ));
            continue;
        }
        let (plus, minus) = stat_bar(file.added, file.removed, max_total, STAT_BAR_WIDTH);
        t.line(format_args!(
            " {:<path_width$} | {:>count_width$} {}{}",
            path,
            file.added + file.removed,
            "+".repeat(plus).green(),
            "-".repeat(minus).red()
        ));
    }
    t.line(format_args!(
        " {} changed, {}(+), {}(-)",
        plural(files.len(), "file", "files"),
        plural(added, "insertion", "insertions"),
        plural(removed, "deletion", "deletions")
    ));
}

#[cfg(test)]
mod tests {
    use worktree_sdk::store::hash_bytes;

    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn split(args: &[&str]) -> Result<(Vec<String>, Vec<PathBuf>)> {
        let revs = ["HEAD", "HEAD~1", "main", "v1"];
        let files = ["src", "README.md", "main.rs"];
        split_revs_and_paths(strings(args), |a| revs.contains(&a), |a| files.contains(&a))
    }

    #[test]
    fn splits_revisions_from_paths() {
        assert_eq!(split(&[]).expect("ok"), (vec![], vec![]));
        assert_eq!(split(&["HEAD"]).expect("ok"), (strings(&["HEAD"]), vec![]));
        assert_eq!(
            split(&["HEAD~1", "HEAD", "src", "README.md"]).expect("ok"),
            (
                strings(&["HEAD~1", "HEAD"]),
                vec![PathBuf::from("src"), PathBuf::from("README.md")]
            )
        );
        assert_eq!(
            split(&["src"]).expect("ok"),
            (vec![], vec![PathBuf::from("src")])
        );
    }

    #[test]
    fn at_most_two_revisions() {
        // A third revision-like argument is only accepted if it is a path.
        let err = split(&["HEAD", "main", "v1"]).expect_err("third rev");
        assert!(err.to_string().contains("'v1'"));
    }

    #[test]
    fn revisions_after_paths_are_paths() {
        let err = split(&["src", "HEAD"]).expect_err("rev after path");
        assert!(err.to_string().contains("'HEAD'"));
    }

    #[test]
    fn unknown_argument_suggests_double_dash() {
        let err = split(&["nope"]).expect_err("unknown");
        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("unknown revision or path"));
    }

    fn file(path: &str, kind: ChangeKind, added: usize, removed: usize, binary: bool) -> FileDiff {
        FileDiff {
            path: path.into(),
            kind,
            old_hash: None,
            new_hash: Some(hash_bytes(path.as_bytes())),
            binary,
            patch: (!binary)
                .then(|| format!("--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-old\n+new\n")),
            added,
            removed,
            old_path: None,
            similarity: None,
        }
    }

    fn renamed(old: &str, new: &str, similarity: u8) -> FileDiff {
        FileDiff {
            old_path: Some(old.into()),
            similarity: Some(similarity),
            patch: None,
            ..file(new, ChangeKind::Modified, 0, 0, false)
        }
    }

    #[test]
    fn name_status_output() {
        let files = [
            file("a.txt", ChangeKind::Modified, 1, 1, false),
            file("b.txt", ChangeKind::Added, 1, 0, false),
            file("c.txt", ChangeKind::Deleted, 0, 1, false),
            renamed("old.rs", "new.rs", 87),
        ];
        let mut t = Text::default();
        render_name_status(&mut t, &files);
        assert_eq!(
            t.as_str(),
            "M\ta.txt\nA\tb.txt\nD\tc.txt\nR087\told.rs\tnew.rs\n"
        );
    }

    #[test]
    fn renames_in_patches_and_stat() {
        colored::control::set_override(false);
        let files = [renamed("src/old.rs", "src/new.rs", 100)];
        let mut t = Text::default();
        render_patches(&mut t, &files);
        assert_eq!(
            t.as_str(),
            "diff --wt a/src/old.rs b/src/new.rs\nrenamed: src/old.rs -> src/new.rs (similarity 100%)\n"
        );
        let mut t = Text::default();
        render_stat(&mut t, &files);
        assert!(
            t.as_str().starts_with(" src/old.rs => src/new.rs | 0 \n"),
            "{}",
            t.as_str()
        );
    }

    #[test]
    fn word_lines_split_by_marker() {
        use WordMark::*;
        assert_eq!(
            word_segments("the [-quick-]{+slow+} fox"),
            [
                (Plain, "the "),
                (Removed, "[-quick-]"),
                (Added, "{+slow+}"),
                (Plain, " fox")
            ]
        );
        assert_eq!(word_segments("{+all+}"), [(Added, "{+all+}")]);
        assert_eq!(word_segments("plain [- open"), [(Plain, "plain [- open")]);
        assert!(word_segments("").is_empty());
        colored::control::set_override(false);
        assert_eq!(
            colorize_word_line("the [-quick-]{+slow+} fox"),
            "the [-quick-]{+slow+} fox"
        );
    }

    #[test]
    fn percent_values() {
        assert_eq!(parse_percent("50"), Ok(50));
        assert_eq!(parse_percent("75%"), Ok(75));
        assert_eq!(parse_percent("100"), Ok(100));
        assert!(parse_percent("101").is_err());
        assert!(parse_percent("x").is_err());
    }

    #[derive(Debug, clap::Parser)]
    struct Harness {
        #[command(flatten)]
        diff: DiffArgs,
    }

    fn parse(args: &[&str]) -> std::result::Result<DiffArgs, clap::Error> {
        use clap::Parser;
        let mut full = vec!["diff"];
        full.extend_from_slice(args);
        Harness::try_parse_from(full).map(|h| h.diff)
    }

    #[test]
    fn parses_new_flags() {
        let a = parse(&["--find-renames"]).unwrap();
        assert_eq!(a.find_renames, Some(50));
        let a = parse(&["--find-renames=90%", "HEAD"]).unwrap();
        assert_eq!(a.find_renames, Some(90));
        assert_eq!(a.args, ["HEAD"]);
        // Without `=`, the next argument stays a revision.
        let a = parse(&["-M", "HEAD"]).unwrap();
        assert_eq!(
            (a.find_renames, a.args.as_slice()),
            (Some(50), &["HEAD".to_string()][..])
        );
        assert_eq!(parse(&[]).unwrap().find_renames, None);
        assert!(parse(&["--word-diff"]).unwrap().word_diff);
        assert!(parse(&["--name-status"]).unwrap().name_status);
        assert!(parse(&["--name-status", "--name-only"]).is_err());
        assert!(parse(&["--name-status", "--stat"]).is_err());
        assert!(parse(&["--word-diff", "--stat"]).is_err());
    }

    #[test]
    fn stat_output() {
        colored::control::set_override(false);
        let files = [
            file("a.txt", ChangeKind::Modified, 3, 1, false),
            file("long/path.rs", ChangeKind::Added, 10, 0, false),
            file("img.png", ChangeKind::Modified, 0, 0, true),
        ];
        let mut t = Text::default();
        render_stat(&mut t, &files);
        let out = t.as_str();
        assert!(out.contains(" a.txt        |  4 +++-\n"), "{out}");
        assert!(out.contains(" long/path.rs | 10 ++++++++++\n"), "{out}");
        assert!(out.contains(" img.png      | Bin\n"), "{out}");
        assert!(
            out.ends_with(" 3 files changed, 13 insertions(+), 1 deletion(-)\n"),
            "{out}"
        );
    }

    #[test]
    fn patch_output() {
        colored::control::set_override(false);
        let files = [
            file("a.txt", ChangeKind::Modified, 1, 1, false),
            file("new.bin", ChangeKind::Added, 0, 0, true),
        ];
        let mut t = Text::default();
        render_patches(&mut t, &files);
        let out = t.as_str();
        assert!(
            out.starts_with(
                "diff --wt a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n"
            ),
            "{out}"
        );
        assert!(out.contains("diff --wt a/new.bin b/new.bin\nnew file\nBinary files a/new.bin and b/new.bin differ\n"), "{out}");
    }

    #[test]
    fn patch_line_colors() {
        assert_eq!(colorize_patch_line("+x"), "+x".green());
        assert_eq!(colorize_patch_line("-x"), "-x".red());
        assert_eq!(colorize_patch_line("@@ -1 +1 @@"), "@@ -1 +1 @@".cyan());
        assert_eq!(colorize_patch_line("+++ b/x"), "+++ b/x".bold());
        assert_eq!(colorize_patch_line(" ctx"), " ctx".normal());
    }
}
