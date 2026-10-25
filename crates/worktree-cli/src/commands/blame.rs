//! `wt blame` — Show which snapshot last changed each line of a file
//! (docs/ENGINEERING_PLAN.md §14.4).

use std::path::PathBuf;

use chrono::Local;
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::{BlameLine, Snapshot};

use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::short;
use crate::output::Text;

/// Show which snapshot last changed each line of a file.
///
/// Walks the history of the revision (first parent first; lines brought in
/// by a merged branch follow that branch) and attributes each line to the
/// snapshot that introduced it. Renames are not followed. Default output:
/// short snapshot id, author, date, line number and the line. `--json`
/// prints `{"path","snapshot","lines":[...]}`.
#[derive(Debug, Args)]
pub struct BlameArgs {
    /// File to blame (relative to the current directory)
    #[arg(value_name = "PATH")]
    pub path: PathBuf,

    /// Blame the file as of this revision (default: HEAD)
    #[arg(long, value_name = "REV")]
    pub rev: Option<String>,

    /// Only these lines: `START,END`, `START,+COUNT`, `START,` or `START`
    /// (the last two run to the end of the file)
    #[arg(short = 'L', value_name = "START,END", value_parser = parse_range)]
    pub lines: Option<LineRange>,

    /// Machine-readable output: per line a `<snapshot id> <line>` header,
    /// `author`, `author-time` (Unix seconds), `created-at` (RFC 3339),
    /// `summary` and `filename` lines, then the text after a tab
    #[arg(long)]
    pub porcelain: bool,
}

/// A `-L` range: 1-based, inclusive; `end: None` = to the end of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: Option<usize>,
}

/// Parses `START,END`, `START,+COUNT`, `START,` and `START`.
fn parse_range(value: &str) -> std::result::Result<LineRange, String> {
    let bad = || format!("expected START,END, START,+COUNT or START (1-based), got {value:?}");
    let number = |s: &str| s.trim().parse::<usize>().map_err(|_| bad());
    let (start, end) = match value.split_once(',') {
        None => (number(value)?, None),
        Some((start, end)) => {
            let start = number(start)?;
            let end = end.trim();
            let end = if end.is_empty() {
                None
            } else if let Some(count) = end.strip_prefix('+') {
                let count = number(count)?;
                if count == 0 {
                    return Err(bad());
                }
                Some(start + count - 1)
            } else {
                Some(number(end)?)
            };
            (start, end)
        }
    };
    if start == 0 || end.is_some_and(|e| e < start) {
        return Err(bad());
    }
    Ok(LineRange { start, end })
}

#[derive(Serialize)]
struct BlameResult<'a> {
    path: &'a str,
    snapshot: &'a Snapshot,
    lines: &'a [BlameLine],
}

pub fn run(ctx: &Context, args: BlameArgs) -> Result<()> {
    if args.porcelain && ctx.out.is_json() {
        return Err(CliError::usage(
            "--porcelain cannot be combined with --json",
        ));
    }
    let (repo, tree) = ctx.open_tree()?;
    let path = match ctx
        .tree_paths(&repo, &tree, std::slice::from_ref(&args.path))?
        .pop()
    {
        Some(p) => p,
        None => {
            return Err(CliError::usage(format!(
                "{} is the tree directory; name a file",
                args.path.display()
            )))
        }
    };
    let snapshot = repo.resolve(&tree.name, args.rev.as_deref().unwrap_or("HEAD"))?;
    let range = args.lines.map(|r| r.start..=r.end.unwrap_or(usize::MAX));
    let lines = repo.blame_lines(&tree.name, &path, Some(&snapshot.id), range)?;

    let result = BlameResult {
        path: &path,
        snapshot: &snapshot,
        lines: &lines,
    };
    // --porcelain is never combined with --json (checked above).
    ctx.out.emit(&result, |t| {
        if args.porcelain {
            render_porcelain(t, &path, &lines);
        } else {
            render(t, &lines);
        }
    })
}

/// `a1b2c3d4 (Ada Lovelace 2026-10-25 12) text`, columns aligned.
fn render(t: &mut Text, lines: &[BlameLine]) {
    let author_width = lines
        .iter()
        .map(|l| author_name(&l.author).chars().count())
        .max()
        .unwrap_or(0);
    let number_width = lines.last().map(|l| l.line.to_string().len()).unwrap_or(1);
    for l in lines {
        let date = l.created_at.with_timezone(&Local).format("%Y-%m-%d");
        t.line(format_args!(
            "{} ({:<aw$} {} {:>nw$}) {}",
            short(&l.snapshot_id).yellow(),
            author_name(&l.author),
            date,
            l.line,
            l.text,
            aw = author_width,
            nw = number_width
        ));
    }
}

/// One block per line (like `git blame --line-porcelain`).
fn render_porcelain(t: &mut Text, path: &str, lines: &[BlameLine]) {
    for l in lines {
        t.line(format_args!("{} {}", l.snapshot_id, l.line));
        t.line(format_args!("author {}", l.author));
        t.line(format_args!("author-time {}", l.created_at.timestamp()));
        t.line(format_args!("created-at {}", l.created_at.to_rfc3339()));
        t.line(format_args!("summary {}", l.message));
        t.line(format_args!("filename {path}"));
        t.line(format_args!("\t{}", l.text));
    }
}

/// `Ada Lovelace <ada@example.com>` -> `Ada Lovelace`.
fn author_name(author: &str) -> &str {
    match author.find('<') {
        Some(i) if i > 0 => author[..i].trim_end(),
        _ => author,
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;

    fn range(start: usize, end: Option<usize>) -> LineRange {
        LineRange { start, end }
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("3,7").unwrap(), range(3, Some(7)));
        assert_eq!(parse_range("3,+2").unwrap(), range(3, Some(4)));
        assert_eq!(parse_range("3,").unwrap(), range(3, None));
        assert_eq!(parse_range("5").unwrap(), range(5, None));
        assert_eq!(parse_range(" 2 , 2 ").unwrap(), range(2, Some(2)));
        for bad in ["0,3", "4,3", "a,b", "3,+0", "", ",5", "1,-2"] {
            assert!(parse_range(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn author_names() {
        assert_eq!(
            author_name("Ada Lovelace <ada@example.com>"),
            "Ada Lovelace"
        );
        assert_eq!(author_name("ada"), "ada");
        assert_eq!(author_name("<anon@example.com>"), "<anon@example.com>");
    }

    fn line(n: usize, id: &str, author: &str, text: &str) -> BlameLine {
        BlameLine {
            line: n,
            text: text.into(),
            snapshot_id: id.into(),
            author: author.into(),
            message: "msg".into(),
            created_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        }
    }

    #[test]
    fn human_and_porcelain_rendering() {
        colored::control::set_override(false);
        let lines: Vec<BlameLine> = (1..=10)
            .map(|n| {
                let author = if n % 2 == 0 {
                    "Bo <b@x>"
                } else {
                    "Alice <a@x>"
                };
                line(n, "0123456789abcdef", author, &format!("t{n}"))
            })
            .collect();
        let mut t = Text::default();
        render(&mut t, &lines);
        let out = t.as_str();
        let first = out.lines().next().unwrap();
        assert!(first.starts_with("01234567 (Alice "), "{first}");
        assert!(first.ends_with("  1) t1"), "{first}");
        let second = out.lines().nth(1).unwrap();
        assert!(second.starts_with("01234567 (Bo    "), "{second}");
        assert!(out.lines().nth(9).unwrap().ends_with(" 10) t10"), "{out}");

        let mut t = Text::default();
        render_porcelain(&mut t, "a.txt", &lines[..1]);
        assert_eq!(
            t.as_str(),
            "0123456789abcdef 1\nauthor Alice <a@x>\nauthor-time 1700000000\n\
             created-at 2023-11-14T22:13:20+00:00\nsummary msg\nfilename a.txt\n\tt1\n"
        );
    }
}
