//! Dynamic shell completion (CliErgonomics §2): the hooks printed by
//! `wt completions SHELL` and the candidates `wt` answers with when the
//! shell calls it back (`WT_COMPLETE=SHELL wt -- WORDS...`).

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

struct Sandbox {
    home: TempDir,
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn cmd(&self, cwd: &Path) -> Command {
        let mut cmd = Command::cargo_bin("wt").expect("wt binary");
        cmd.current_dir(cwd)
            .env("WT_HOME", self.home.path())
            .env("NO_COLOR", "1")
            .env_remove("WT_COMPLETE")
            .env_remove("WT_SERVER")
            .env_remove("WT_TOKEN")
            .env_remove("WT_LOG");
        cmd
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.cmd(self.path()).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "wt {args:?} failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// Ask for completions of the last of `words` (a `wt ...` command line)
    /// the way fish does, from `cwd`; returns the candidate values that are
    /// not options. The request must succeed silently.
    fn complete_in(&self, cwd: &Path, words: &[&str]) -> Vec<String> {
        let out = self
            .cmd(cwd)
            .env("WT_COMPLETE", "fish")
            .arg("--")
            .args(words)
            .output()
            .unwrap();
        assert!(out.status.success(), "{words:?}: {out:?}");
        assert!(out.stderr.is_empty(), "{words:?}: {out:?}");
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|l| l.split('\t').next().unwrap_or_default().to_string())
            .filter(|v| !v.starts_with('-'))
            .collect()
    }

    fn complete(&self, words: &[&str]) -> Vec<String> {
        self.complete_in(self.path(), words)
    }

    /// A worktree with branches `main`, `dev`, `feature/login` and the
    /// archived `old`, tags `v1` and `release/2.0`, remotes `origin` and
    /// `backup`, a remote-tracking `origin/feature/remote`, and a nested
    /// tree `lib` (in `libs/lib`) with a branch `lib-only`.
    fn populated() -> Self {
        let sb = Sandbox::new();
        sb.ok(&["init", "--name", "web"]);
        fs::write(sb.path().join("a.txt"), "a\n").unwrap();
        sb.ok(&["snapshot", "-m", "one"]);
        for branch in ["dev", "feature/login", "old"] {
            sb.ok(&["branch", "create", branch]);
        }
        sb.ok(&["branch", "archive", "old"]);
        sb.ok(&["tag", "v1"]);
        sb.ok(&["tag", "release/2.0"]);
        for (name, url) in [
            ("origin", "http://localhost:7420"),
            ("backup", "http://localhost:7421"),
        ] {
            sb.ok(&["remote", "add", name, url, "--tenant", "acme"]);
        }
        let refs = sb.path().join(".wt/trees/web/refs");
        let tip = fs::read_to_string(refs.join("branches/main")).unwrap();
        let remote_ref = refs.join("remotes/origin/feature/remote");
        fs::create_dir_all(remote_ref.parent().unwrap()).unwrap();
        fs::write(remote_ref, tip).unwrap();
        sb.ok(&["tree", "add", "lib", "libs/lib"]);
        sb.ok(&["--tree", "lib", "branch", "create", "lib-only"]);
        fs::create_dir_all(sb.path().join("src/deep")).unwrap();
        sb
    }

    fn deep(&self) -> PathBuf {
        self.path().join("src/deep")
    }
}

fn has(values: &[String], wanted: &[&str]) -> bool {
    wanted.iter().all(|w| values.iter().any(|v| v == w))
}

fn lacks(values: &[String], unwanted: &[&str]) -> bool {
    unwanted.iter().all(|u| values.iter().all(|v| v != u))
}

#[test]
fn completion_scripts_are_dynamic_hooks() {
    let sb = Sandbox::new();
    for (shell, hook) in [
        ("bash", "complete -o nospace -o bashdefault"),
        ("zsh", "compdef _clap_dynamic_completer_wt wt"),
        ("fish", "complete --keep-order --exclusive --command wt"),
        (
            "powershell",
            "Register-ArgumentCompleter -Native -CommandName wt",
        ),
        ("elvish", "edit:completion:arg-completer[wt]"),
    ] {
        let script = sb.ok(&["completions", shell]);
        assert!(script.contains("WT_COMPLETE"), "{shell}: {script}");
        assert!(script.contains(hook), "{shell}: {script}");
    }
    // Installed in $fpath as _wt, the zsh file completes on its first call.
    let zsh = sb.ok(&["completions", "zsh"]);
    assert!(zsh.starts_with("#compdef wt"), "{zsh}");
    assert!(zsh.contains(r#""${funcstack[1]}" == "_wt""#), "{zsh}");
    sb.cmd(sb.path())
        .args(["completions", "tcsh"])
        .assert()
        .code(2);
}

#[test]
fn branches_exclude_archived_unless_restoring() {
    let sb = Sandbox::populated();
    let switch = sb.complete(&["wt", "switch", ""]);
    assert!(
        has(&switch, &["dev", "feature/login", "main"]),
        "{switch:?}"
    );
    assert!(lacks(&switch, &["old", "v1", "lib-only"]), "{switch:?}");
    assert_eq!(
        sb.complete(&["wt", "switch", "feature/lo"]),
        ["feature/login"]
    );
    let archive = sb.complete(&["wt", "branch", "archive", ""]);
    assert!(
        has(&archive, &["dev", "feature/login", "main"]),
        "{archive:?}"
    );
    assert!(lacks(&archive, &["old"]), "{archive:?}");
    assert_eq!(sb.complete(&["wt", "branch", "restore", ""]), ["old"]);
    for words in [
        &["wt", "push", "--branch", "fe"][..],
        &["wt", "pull", "--branch", "fe"],
        &["wt", "branch", "describe", "fe"],
    ] {
        assert_eq!(sb.complete(words), ["feature/login"], "{words:?}");
    }
}

#[test]
fn tags_remotes_and_trees() {
    let sb = Sandbox::populated();
    for sub in ["delete", "verify", "push"] {
        assert_eq!(
            sb.complete(&["wt", "tag", sub, ""]),
            ["release/2.0", "v1"],
            "{sub}"
        );
    }
    assert_eq!(sb.complete(&["wt", "tag", "push", "rel"]), ["release/2.0"]);
    for words in [
        &["wt", "push", "--remote", ""][..],
        &["wt", "pull", "--remote", ""],
        &["wt", "fetch", "--remote", ""],
        &["wt", "tag", "push", "v1", "--remote", ""],
        &["wt", "remote", "remove", ""],
    ] {
        assert_eq!(sb.complete(words), ["backup", "origin"], "{words:?}");
    }
    assert_eq!(sb.complete(&["wt", "fetch", "--remote", "o"]), ["origin"]);
    assert_eq!(sb.complete(&["wt", "--tree", ""]), ["web", "lib"]);
    assert_eq!(sb.complete(&["wt", "log", "--tree", "l"]), ["lib"]);
    assert_eq!(sb.complete(&["wt", "tree", "remove", "l"]), ["lib"]);
}

#[test]
fn revisions_are_branches_tags_and_remote_tracking_refs() {
    let sb = Sandbox::populated();
    let all = [
        "dev",
        "feature/login",
        "main",
        "release/2.0",
        "v1",
        "origin/feature/remote",
    ];
    for words in [
        &["wt", "merge", ""][..],
        &["wt", "revert", ""],
        &["wt", "restore", "--from", ""],
        &["wt", "log", ""],
        &["wt", "branch", "create", "x", "--from", ""],
        &["wt", "tag", "create", "x", ""],
    ] {
        let values = sb.complete(words);
        assert!(has(&values, &all), "{words:?}: {values:?}");
        assert!(lacks(&values, &["old"]), "{words:?}: {values:?}");
    }
    assert_eq!(sb.complete(&["wt", "revert", "re"]), ["release/2.0"]);
    // `wt diff` takes revisions and paths.
    let diff = sb.complete(&["wt", "diff", ""]);
    assert!(has(&diff, &all), "{diff:?}");
    assert!(has(&diff, &["a.txt"]), "{diff:?}");
    assert_eq!(sb.complete(&["wt", "diff", "feature/"]), ["feature/login"]);
}

#[test]
fn completion_follows_subdirectories_and_selected_tree() {
    let sb = Sandbox::populated();
    let deep = sb.deep();
    assert_eq!(
        sb.complete_in(&deep, &["wt", "switch", "feat"]),
        ["feature/login"]
    );
    assert_eq!(sb.complete_in(&deep, &["wt", "tag", "delete", "v"]), ["v1"]);
    // Inside the nested tree, or with --tree, its own branches.
    let lib_dir = sb.path().join("libs/lib");
    let lib = sb.complete_in(&lib_dir, &["wt", "switch", ""]);
    assert!(has(&lib, &["lib-only", "main"]), "{lib:?}");
    assert!(lacks(&lib, &["feature/login"]), "{lib:?}");
    assert_eq!(
        sb.complete(&["wt", "--tree", "lib", "switch", "lib"]),
        ["lib-only"]
    );
    assert_eq!(
        sb.complete(&["wt", "--tree=lib", "switch", "lib"]),
        ["lib-only"]
    );
    // -C points completion at another directory.
    let outside = tempfile::tempdir().unwrap();
    let dir = sb.path().to_string_lossy().into_owned();
    assert_eq!(
        sb.complete_in(outside.path(), &["wt", "-C", &dir, "switch", "feat"]),
        ["feature/login"]
    );
}

#[test]
fn nothing_but_commands_and_flags_outside_a_worktree() {
    let sb = Sandbox::new();
    for words in [
        &["wt", "switch", ""][..],
        &["wt", "merge", "m"],
        &["wt", "tag", "delete", ""],
        &["wt", "push", "--remote", ""],
        &["wt", "--tree", ""],
        &["wt", "branch", "restore", ""],
    ] {
        let values = sb.complete(words);
        let refs: Vec<&String> = values
            .iter()
            .filter(|v| !["resolve", "abort", "status", "help"].contains(&v.as_str()))
            .collect();
        assert!(refs.is_empty(), "{words:?}: {values:?}");
    }
    // A broken worktree is just as quiet.
    fs::create_dir_all(sb.path().join(".wt")).unwrap();
    fs::write(sb.path().join(".wt/config.toml"), "not toml [").unwrap();
    assert!(sb.complete(&["wt", "switch", ""]).is_empty());
}

#[test]
fn bash_protocol_and_aliases_are_current() {
    let sb = Sandbox::populated();
    sb.ok(&["config", "set", "alias.lgx", "log --oneline"]);
    let out = sb
        .cmd(&sb.deep())
        .env("WT_COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "2")
        .env("_CLAP_IFS", "\n")
        .args(["--", "wt", "switch", "fe"])
        .output()
        .unwrap();
    assert!(out.status.success() && out.stderr.is_empty(), "{out:?}");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "feature/login");
    // Aliases are completed as commands without regenerating the script.
    assert_eq!(sb.complete(&["wt", "lg"]), ["lgx"]);
}
