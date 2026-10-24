//! Driving the real `git` CLI with an isolated, deterministic config.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cmd::{Cmd, CmdOutput};

/// Base timestamp for synthetic commits (2023-11-14T22:13:20Z).
const BASE_TS: u64 = 1_700_000_000;

pub struct Git {
    log_dir: PathBuf,
    home: PathBuf,
    config: PathBuf,
    tick: AtomicU64,
}

impl Git {
    /// Isolated git environment rooted at `home` (global config written there).
    pub fn new(home: &Path, log_dir: &Path) -> Self {
        std::fs::create_dir_all(home).unwrap_or_else(|e| panic!("create {}: {e}", home.display()));
        let config = home.join("gitconfig");
        crate::fsutil::write(
            &config,
            "[core]\n\tautocrlf = false\n\tsafecrlf = false\n\tfilemode = false\n\tlongpaths = true\n\
             [init]\n\tdefaultBranch = main\n\
             [user]\n\tname = Git Author\n\temail = git.author@example.com\n\
             [commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n\
             [advice]\n\tdetachedHead = false\n",
        );
        Self {
            log_dir: log_dir.to_path_buf(),
            home: home.to_path_buf(),
            config,
            tick: AtomicU64::new(0),
        }
    }

    /// Whether a `git` executable is on PATH.
    pub fn available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// `git <args>` in `dir` (not yet run).
    pub fn cmd(&self, dir: &Path) -> Cmd {
        Cmd::new("git", &self.log_dir)
            .cwd(dir)
            .actor("git")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &self.config)
            .env("HOME", &self.home)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
    }

    /// Run `git <args>` in `dir`, requiring success.
    #[track_caller]
    pub fn run(&self, dir: &Path, args: &[&str]) -> CmdOutput {
        self.cmd(dir).args(args).ok()
    }

    /// Trimmed stdout of `git <args>`.
    #[track_caller]
    pub fn out(&self, dir: &Path, args: &[&str]) -> String {
        self.run(dir, args).stdout.trim().to_string()
    }

    /// Next deterministic timestamp in git's raw date format.
    fn next_date(&self) -> String {
        let n = self.tick.fetch_add(1, Ordering::SeqCst) + 1;
        format!("{} +0000", BASE_TS + n * 3600)
    }

    /// Run a committing git command (commit / merge / tag -a) with a fixed
    /// author and deterministic, strictly increasing dates.
    #[track_caller]
    pub fn committing(&self, dir: &Path, args: &[&str], author: (&str, &str)) -> CmdOutput {
        let date = self.next_date();
        self.cmd(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", author.0)
            .env("GIT_AUTHOR_EMAIL", author.1)
            .env("GIT_COMMITTER_NAME", author.0)
            .env("GIT_COMMITTER_EMAIL", author.1)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .ok()
    }

    /// `git add -A && git commit -m msg` as `author`.
    #[track_caller]
    pub fn commit_all(&self, dir: &Path, msg: &str, author: (&str, &str)) -> CmdOutput {
        self.run(dir, &["add", "-A"]);
        self.committing(dir, &["commit", "-q", "-m", msg], author)
    }
}
