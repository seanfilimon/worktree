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
    #[arg(long, conflicts_with = "stat")]
    pub name_only: bool,

    /// Show a diffstat instead of patches
    #[arg(long)]
    pub stat: bool,
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
        name_only: args.name_only,
        paths: ctx.tree_paths(&repo, &tree, &paths)?,
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
        } else if args.stat {
            render_stat(t, &files);
        } else {
            render_patches(t, &files);
        }
    })
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
    for file in files {
        t.line(format!("diff --wt a/{0} b/{0}", file.path).bold());
        match file.kind {
            ChangeKind::Added => t.line("new file".bold()),
            ChangeKind::Deleted => t.line("deleted file".bold()),
            ChangeKind::Modified => {}
        }
        if file.binary {
            t.line(format_args!(
                "Binary files a/{0} and b/{0} differ",
                file.path
            ));
            continue;
        }
        for line in file.patch.as_deref().unwrap_or_default().lines() {
            t.line(colorize_patch_line(line));
        }
    }
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
    let path_width = files
        .iter()
        .map(|f| f.path.chars().count())
        .max()
        .unwrap_or(0);
    let max_total = files.iter().map(|f| f.added + f.removed).max().unwrap_or(0);
    let count_width = max_total.to_string().len();
    let (mut added, mut removed) = (0, 0);
    for file in files {
        added += file.added;
        removed += file.removed;
        if file.binary {
            t.line(format_args!(
                " {:<path_width$} | {:>count_width$}",
                file.path, "Bin"
            ));
            continue;
        }
        let (plus, minus) = stat_bar(file.added, file.removed, max_total, STAT_BAR_WIDTH);
        t.line(format_args!(
            " {:<path_width$} | {:>count_width$} {}{}",
            file.path,
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
        }
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
