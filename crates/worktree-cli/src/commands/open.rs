//! `wt open [mr N | snapshot REV | file PATH] [--print]` — open the current
//! tree (or one of its merge requests, snapshots or files) in the admin
//! panel served by the tree's server:
//!
//! - tree:      `<server>/admin/trees/<tenant>/<tree>`
//! - mr N:      `.../merge-requests/<N>`
//! - snapshot:  `.../snapshots/<id>` (REV resolved locally when possible)
//! - file PATH: `.../files?path=<tree-relative path>&branch=<current branch>`
//!
//! The browser is `$BROWSER` when set, else `cmd /c start` (Windows),
//! `open` (macOS) or `xdg-open` (elsewhere). `--print` only prints the URL.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use clap::{Args, Subcommand};
use serde::Serialize;
use worktree_sdk::remote::rest::{path_segs, seg};
use worktree_sdk::remote::ORIGIN;

use crate::commands::remote::normalize_server;
use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct OpenArgs {
    #[command(subcommand)]
    pub target: Option<OpenTarget>,

    /// Print the URL instead of opening a browser
    #[arg(long, global = true)]
    pub print: bool,

    /// Remote whose server and tenant to use
    #[arg(long, value_name = "NAME", global = true, default_value = ORIGIN)]
    pub remote: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum OpenTarget {
    /// The tree overview (the default)
    Tree,
    /// A merge request of the tree
    Mr {
        /// Merge request number
        number: u64,
    },
    /// A snapshot (HEAD, a branch, a tag or an id prefix; resolved locally)
    Snapshot {
        /// Revision to open
        #[arg(value_name = "REV")]
        rev: String,
    },
    /// A file or directory in the Files tab, on the current branch
    File {
        /// Path (relative to the current directory)
        path: PathBuf,
    },
}

/// A resolved admin-panel page of one server tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Tree,
    MergeRequest(u64),
    Snapshot(String),
    /// Tree-relative '/'-separated path ("" = the tree root) and branch.
    File {
        path: String,
        branch: Option<String>,
    },
}

/// The admin-panel URL of `page` in `tenant/tree` on `server`.
pub fn admin_url(server: &str, tenant: &str, tree: &str, page: &Page) -> String {
    let base = format!(
        "{}/admin/trees/{}/{}",
        normalize_server(server),
        seg(tenant),
        seg(tree)
    );
    match page {
        Page::Tree => base,
        Page::MergeRequest(n) => format!("{base}/merge-requests/{n}"),
        Page::Snapshot(id) => format!("{base}/snapshots/{}", seg(id)),
        Page::File { path, branch } => {
            let mut query = Vec::new();
            if !path.is_empty() {
                query.push(format!("path={}", path_segs(path)));
            }
            if let Some(branch) = branch.as_deref().filter(|b| !b.is_empty()) {
                query.push(format!("branch={}", path_segs(branch)));
            }
            if query.is_empty() {
                format!("{base}/files")
            } else {
                format!("{base}/files?{}", query.join("&"))
            }
        }
    }
}

/// How to launch a browser for a URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    /// Pass `args` verbatim (Windows `cmd` quoting) instead of escaping.
    pub raw: bool,
}

/// The launcher for `url` on `os` (`std::env::consts::OS`); `browser` is
/// `$BROWSER`.
pub fn launcher(os: &str, url: &str, browser: Option<&str>) -> Launch {
    let plain = |program: &str| Launch {
        program: program.to_string(),
        args: vec![url.to_string()],
        raw: false,
    };
    if let Some(browser) = browser.map(str::trim).filter(|b| !b.is_empty()) {
        return plain(browser);
    }
    match os {
        // `start` treats its first quoted argument as the window title,
        // hence the empty "". Quoting keeps `&` in query strings literal.
        "windows" => Launch {
            program: "cmd".into(),
            args: vec![
                "/C".into(),
                "start".into(),
                "\"\"".into(),
                format!("\"{}\"", url.replace('"', "%22")),
            ],
            raw: true,
        },
        "macos" => plain("open"),
        _ => plain("xdg-open"),
    }
}

fn spawn(launch: &Launch) -> std::io::Result<()> {
    let mut command = Command::new(&launch.program);
    if launch.raw {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            for arg in &launch.args {
                command.raw_arg(arg);
            }
        }
        #[cfg(not(windows))]
        command.args(&launch.args);
    } else {
        command.args(&launch.args);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[derive(Serialize)]
struct Opened<'a> {
    url: &'a str,
    opened: bool,
}

pub fn run(ctx: &Context, args: OpenArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = repo.config().remote.get(&args.remote).ok_or_else(|| {
        CliError::failure(format!("remote '{}' is not configured", args.remote)).with_hint(
            "add one with `wt remote add origin <url> --tenant <tenant>`, or pass --remote NAME",
        )
    })?;
    let server_tree = if tree.remote_name.is_empty() {
        tree.name.clone()
    } else {
        tree.remote_name.clone()
    };
    let page = match args.target.unwrap_or(OpenTarget::Tree) {
        OpenTarget::Tree => Page::Tree,
        OpenTarget::Mr { number } => Page::MergeRequest(number),
        OpenTarget::Snapshot { rev } => match repo.resolve(&tree.name, &rev) {
            Ok(snapshot) => Page::Snapshot(snapshot.id),
            // Not fetched yet: a full or long id may still exist on the server.
            Err(_) if rev.len() >= 8 && rev.chars().all(|c| c.is_ascii_hexdigit()) => {
                Page::Snapshot(rev)
            }
            Err(e) => return Err(e.into()),
        },
        OpenTarget::File { path } => {
            let paths = ctx.tree_paths(&repo, &tree, &[path])?;
            Page::File {
                path: paths.into_iter().next().unwrap_or_default(),
                branch: Some(tree.current_branch.clone()),
            }
        }
    };
    let url = admin_url(&remote.url, &remote.tenant, &server_tree, &page);
    if args.print {
        return ctx.out.emit(
            &Opened {
                url: &url,
                opened: false,
            },
            |t| t.line(&url),
        );
    }
    let browser = std::env::var("BROWSER").ok();
    let launch = launcher(std::env::consts::OS, &url, browser.as_deref());
    spawn(&launch).map_err(|e| {
        CliError::failure(format!("cannot start '{}': {e}", launch.program))
            .with_hint(format!("open it yourself: {url} (or use --print)"))
    })?;
    ctx.out.emit(
        &Opened {
            url: &url,
            opened: true,
        },
        |t| t.line(format_args!("Opening {url}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command as Cli;
    use crate::commands::server_common::testing::{parse, parse_err};

    const SERVER: &str = "http://localhost:7420/";

    #[test]
    fn builds_admin_urls() {
        let url = |page: &Page| admin_url(SERVER, "acme", "web", page);
        assert_eq!(
            url(&Page::Tree),
            "http://localhost:7420/admin/trees/acme/web"
        );
        assert_eq!(
            url(&Page::MergeRequest(12)),
            "http://localhost:7420/admin/trees/acme/web/merge-requests/12"
        );
        assert_eq!(
            url(&Page::Snapshot("0123abcd".into())),
            "http://localhost:7420/admin/trees/acme/web/snapshots/0123abcd"
        );
        assert_eq!(
            url(&Page::File {
                path: "src/my file&x.rs".into(),
                branch: Some("feature/login".into()),
            }),
            "http://localhost:7420/admin/trees/acme/web/files?path=src/my%20file%26x.rs&branch=feature/login"
        );
        assert_eq!(
            url(&Page::File {
                path: String::new(),
                branch: None
            }),
            "http://localhost:7420/admin/trees/acme/web/files"
        );
        // Names are percent-encoded; schemeless servers get http://.
        assert_eq!(
            admin_url("example.com:7420", "my org", "a/b", &Page::Tree),
            "http://example.com:7420/admin/trees/my%20org/a%2Fb"
        );
        assert_eq!(
            admin_url("https://wt.example.com", "acme", "web", &Page::Tree),
            "https://wt.example.com/admin/trees/acme/web"
        );
    }

    #[test]
    fn picks_the_platform_launcher() {
        let url = "http://s/admin/trees/a/b/files?path=x&branch=main";
        let win = launcher("windows", url, None);
        assert_eq!(win.program, "cmd");
        assert!(win.raw);
        assert_eq!(win.args[..3], ["/C", "start", "\"\""]);
        assert_eq!(win.args[3], format!("\"{url}\""));
        assert_eq!(
            launcher("macos", url, None),
            Launch {
                program: "open".into(),
                args: vec![url.into()],
                raw: false
            }
        );
        assert_eq!(launcher("linux", url, None).program, "xdg-open");
        assert_eq!(launcher("freebsd", url, Some("  ")).program, "xdg-open");
        let custom = launcher("windows", url, Some("firefox"));
        assert_eq!((custom.program.as_str(), custom.raw), ("firefox", false));
        assert_eq!(custom.args, [url]);
    }

    #[test]
    fn parses_targets_and_flags() {
        let Cli::Open(a) = parse(&["open"]) else {
            panic!()
        };
        assert_eq!(
            (a.target, a.print, a.remote.as_str()),
            (None, false, "origin")
        );
        let Cli::Open(a) = parse(&["open", "mr", "7", "--print"]) else {
            panic!()
        };
        assert_eq!(a.target, Some(OpenTarget::Mr { number: 7 }));
        assert!(a.print);
        let Cli::Open(a) = parse(&["open", "--print", "snapshot", "HEAD~1", "--remote", "up"])
        else {
            panic!()
        };
        assert_eq!(
            a.target,
            Some(OpenTarget::Snapshot {
                rev: "HEAD~1".into()
            })
        );
        assert_eq!(a.remote, "up");
        let Cli::Open(a) = parse(&["open", "file", "src/lib.rs"]) else {
            panic!()
        };
        assert_eq!(
            a.target,
            Some(OpenTarget::File {
                path: "src/lib.rs".into()
            })
        );
        assert_eq!(
            parse_err(&["open", "mr", "x"]),
            clap::error::ErrorKind::ValueValidation
        );
    }
}
