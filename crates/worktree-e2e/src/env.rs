//! Per-scenario environment: temp dir, optional server, simulated users.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;

use crate::api::Api;
use crate::bins;
use crate::cmd::{Cmd, CmdOutput};
use crate::fsutil;
use crate::git::Git;
use crate::json;
use crate::server::Server;

/// Strip the `\\?\` verbatim prefix Windows' canonicalize adds to drive
/// paths (many tools, including Go, mishandle it).
fn simplify(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    p
}

pub struct TestEnv {
    pub name: String,
    /// Long-form, canonical temp root (contains spaces on some machines —
    /// intentionally exercised).
    pub root: PathBuf,
    /// Where every command's stdout/stderr is recorded.
    pub logs: PathBuf,
    server: Option<Server>,
    tmp: Option<TempDir>,
}

impl TestEnv {
    /// Temp dir only (no server).
    #[track_caller]
    pub fn new(name: &str) -> Self {
        let tmp = tempfile::Builder::new()
            .prefix(&format!("wt-e2e-{name}-"))
            .tempdir()
            .expect("create temp dir");
        let root = simplify(
            std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf()),
        );
        let logs = root.join("logs");
        std::fs::create_dir_all(&logs).expect("create logs dir");
        eprintln!("e2e[{name}]: temp dir {}", root.display());
        Self {
            name: name.to_string(),
            root,
            logs,
            server: None,
            tmp: Some(tmp),
        }
    }

    /// Temp dir plus a freshly started server.
    #[track_caller]
    pub fn with_server(name: &str) -> Self {
        Self::with_server_args(name, &[])
    }

    /// Temp dir plus a server started with extra flags.
    #[track_caller]
    pub fn with_server_args(name: &str, extra_args: &[&str]) -> Self {
        let mut env = Self::new(name);
        let server = Server::start_with(&env.root.join("server"), extra_args);
        eprintln!("e2e[{name}]: server {}", server.url);
        env.server = Some(server);
        env
    }

    pub fn server(&self) -> &Server {
        self.server
            .as_ref()
            .expect("this TestEnv has no server (use TestEnv::with_server)")
    }

    /// sync workstream (§15.5): the server, mutably (stop / restart it).
    pub fn server_mut(&mut self) -> &mut Server {
        self.server
            .as_mut()
            .expect("this TestEnv has no server (use TestEnv::with_server)")
    }

    /// Server base URL.
    pub fn url(&self) -> String {
        self.server().url.clone()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// A simulated user with its own `WT_HOME` and working area
    /// (`<root>/<name>/`). Display name is the capitalized username.
    pub fn user(&self, name: &str) -> User {
        let base = self.root.join("users").join(name);
        let home = base.join("wt-home");
        let dir = self.root.join(name);
        std::fs::create_dir_all(&home).expect("create WT_HOME");
        std::fs::create_dir_all(&dir).expect("create user dir");
        let git_config = base.join("gitconfig");
        fsutil::write(&git_config, "[core]\n\tautocrlf = false\n");
        let mut display = name.to_string();
        if let Some(first) = display.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        User {
            name: name.to_string(),
            display,
            email: format!("{name}@example.com"),
            password: format!("Pw-{}-x9", crate::random_hex(16)),
            home,
            dir,
            logs: self.logs.join(name),
            server_url: self.server.as_ref().map(|s| s.url.clone()),
        }
    }

    /// Isolated git CLI helper.
    pub fn git(&self) -> Git {
        Git::new(&self.root.join("git-home"), &self.logs.join("git"))
    }

    /// The server admin as a simulated user (own `WT_HOME`, the server's
    /// admin password; not logged in yet — call [`User::login`]).
    pub fn admin_user(&self) -> User {
        let server = self.server();
        let mut admin = self.user(&server.admin_user);
        admin.password = server.admin_password.clone();
        admin.email = format!("{}@localhost", server.admin_user);
        admin
    }
}

/// A TOML basic string literal.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `scheme://host[:port]` without a trailing slash (the key format of
/// `credentials.toml`; the e2e server URL is already in this form).
fn normalize_server_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.contains("://") {
        url.to_ascii_lowercase()
    } else {
        format!("http://{}", url.to_ascii_lowercase())
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let failed = std::thread::panicking();
        if failed {
            eprintln!(
                "e2e[{}] FAILED — temp dir: {} (command logs in {})",
                self.name,
                self.root.display(),
                self.logs.display()
            );
            if let Some(s) = &self.server {
                eprintln!("{}", s.logs());
            }
        }
        if let Some(mut s) = self.server.take() {
            s.stop();
        }
        // Keep the temp dir for post-mortem on failure (unless WT_E2E_KEEP=0)
        // or always with WT_E2E_KEEP=1.
        let keep = match std::env::var("WT_E2E_KEEP").as_deref() {
            Ok("1") => true,
            Ok("0") => false,
            _ => failed,
        };
        if let Some(tmp) = self.tmp.take() {
            if keep {
                let p = tmp.keep();
                eprintln!("e2e[{}]: kept {}", self.name, p.display());
            }
            // Otherwise TempDir's drop removes it (errors ignored).
        }
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub name: String,
    /// "Alice" for "alice" (used as `user.name`).
    pub display: String,
    pub email: String,
    pub password: String,
    /// `WT_HOME` for this user.
    pub home: PathBuf,
    /// Working area for this user's checkouts.
    pub dir: PathBuf,
    logs: PathBuf,
    server_url: Option<String>,
}

impl User {
    /// `wt` in the user's working area.
    pub fn wt(&self) -> Cmd {
        self.wt_in(&self.dir)
    }

    /// `wt` in `dir` with this user's isolated environment.
    pub fn wt_in(&self, dir: &Path) -> Cmd {
        let mut c = Cmd::new(bins::wt_bin(), &self.logs)
            .cwd(dir)
            .actor(&self.name)
            .env("WT_HOME", &self.home)
            .env("HOME", &self.home)
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .env("RUST_BACKTRACE", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                self.home.parent().unwrap_or(&self.home).join("gitconfig"),
            )
            .env_remove("WT_PASSWORD")
            .env_remove("WT_TOKEN")
            .env_remove("WT_SERVER");
        if let Some(bg) = bins::try_bg_bin() {
            c = c.env("WT_BG_BIN", &bg);
            if let Some(bg_dir) = bg.parent() {
                let mut paths = vec![bg_dir.to_path_buf()];
                if let Some(p) = std::env::var_os("PATH") {
                    paths.extend(std::env::split_paths(&p));
                }
                if let Ok(joined) = std::env::join_paths(paths) {
                    c = c.env("PATH", joined);
                }
            }
        }
        c
    }

    /// Server URL of the env this user belongs to.
    pub fn server(&self) -> &str {
        self.server_url
            .as_deref()
            .expect("user has no server (use TestEnv::with_server)")
    }

    /// `wt register <server> -u <name> -e <email> --password-stdin`.
    #[track_caller]
    pub fn register(&self) -> CmdOutput {
        self.wt()
            .args([
                "register",
                self.server(),
                "-u",
                &self.name,
                "-e",
                &self.email,
                "--password-stdin",
            ])
            .stdin(format!("{}\n", self.password))
            .ok()
    }

    /// `wt login <server> -u <name> --password-stdin`.
    #[track_caller]
    pub fn login(&self) -> CmdOutput {
        self.wt()
            .args(["login", self.server(), "-u", &self.name, "--password-stdin"])
            .stdin(format!("{}\n", self.password))
            .ok()
    }

    /// REST client authenticated as this user (fresh session token).
    #[track_caller]
    pub fn api(&self) -> Api {
        Api::login(self.server(), &self.name, &self.password)
    }

    /// `wt init <dir>/<name> --name <name>` + identity config.
    #[track_caller]
    pub fn init(&self, name: &str) -> Repo {
        let path = self.dir.join(name);
        self.wt().arg("init").arg(&path).args(["--name", name]).ok();
        let repo = self.repo(&path, name);
        repo.set_identity();
        repo
    }

    /// `wt clone <server> <tenant>/<tree> <dir>/<dest>` + identity config.
    #[track_caller]
    pub fn clone_tree(&self, tenant: &str, tree: &str, dest: &str) -> Repo {
        let path = self.dir.join(dest);
        self.wt()
            .args(["clone", self.server(), &format!("{tenant}/{tree}")])
            .arg(&path)
            .ok();
        let repo = self.repo(&path, tree);
        repo.set_identity();
        repo
    }

    /// Wrap an existing worktree at `path` whose root tree is `name`.
    pub fn repo(&self, path: &Path, name: &str) -> Repo {
        Repo {
            user: self.clone(),
            dir: path.to_path_buf(),
            name: name.to_string(),
            tree: None,
        }
    }

    /// `wt daemon start`; the guard stops (and if needed kills) it on drop.
    #[track_caller]
    pub fn daemon_start(&self) -> DaemonGuard {
        let guard = DaemonGuard {
            user: self.clone(),
            stopped: false,
        };
        self.wt()
            .args(["daemon", "start"])
            .timeout(Duration::from_secs(60))
            .ok();
        guard
    }

    pub fn bg_json_path(&self) -> PathBuf {
        self.home.join("bg.json")
    }

    /// `$WT_HOME/credentials.toml`.
    pub fn credentials_path(&self) -> PathBuf {
        self.home.join("credentials.toml")
    }

    /// Save `token` as this user's credential for the env's server
    /// (replacing `credentials.toml`), exactly as `wt login` stores it. Used
    /// to act as a service account or with a scoped API token.
    #[track_caller]
    pub fn save_token(&self, username: &str, token: &str) {
        let server = normalize_server_url(self.server());
        fsutil::write(
            &self.credentials_path(),
            format!(
                "[servers.{}]\nusername = {}\ntoken = {}\n",
                toml_string(&server),
                toml_string(username),
                toml_string(token)
            ),
        );
    }

    /// The token saved in `credentials.toml` for the env's server, if any.
    pub fn saved_token(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.credentials_path()).ok()?;
        let header = format!(
            "[servers.{}]",
            toml_string(&normalize_server_url(self.server()))
        );
        let mut in_section = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_section = line == header;
            } else if in_section {
                if let Some(rest) = line.strip_prefix("token") {
                    let value = rest.trim_start().strip_prefix('=')?.trim();
                    return Some(value.trim_matches(|c| c == '"' || c == '\'').to_string());
                }
            }
        }
        None
    }

    /// `wt login <server> -u <name> --password-stdin <extra...>` (not
    /// asserted; MFA scenarios check success/failure themselves).
    pub fn login_with(&self, extra: &[&str]) -> CmdOutput {
        self.wt()
            .args(["login", self.server(), "-u", &self.name, "--password-stdin"])
            .args(extra)
            .stdin(format!("{}\n", self.password))
            .run()
    }
}

/// Stops the user's daemon on drop (`wt daemon stop`, then kill by pid
/// from `$WT_HOME/bg.json` if it is still there).
pub struct DaemonGuard {
    user: User,
    stopped: bool,
}

impl DaemonGuard {
    /// Explicit `wt daemon stop` (asserted to succeed).
    #[track_caller]
    pub fn stop(&mut self) -> CmdOutput {
        self.stopped = true;
        self.user
            .wt()
            .args(["daemon", "stop"])
            .timeout(Duration::from_secs(60))
            .ok()
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self
                .user
                .wt()
                .args(["daemon", "stop"])
                .timeout(Duration::from_secs(20))
                .try_run();
        }
        let info = self.user.bg_json_path();
        if let Ok(text) = std::fs::read_to_string(&info) {
            // Give a clean shutdown a moment to remove bg.json, then kill.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while info.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            if info.exists() {
                if let Some(pid) = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.get("pid").and_then(Value::as_u64))
                {
                    eprintln!("e2e: killing leftover daemon pid {pid}");
                    crate::kill_pid(pid as u32);
                }
            }
        }
    }
}

/// A worktree checkout owned by a user. `tree` (if set) is passed as
/// `--tree` to every command.
#[derive(Debug, Clone)]
pub struct Repo {
    pub user: User,
    pub dir: PathBuf,
    /// Root tree name (worktree name).
    pub name: String,
    pub tree: Option<String>,
}

impl Repo {
    /// `wt [--tree T]` in the worktree root.
    pub fn wt(&self) -> Cmd {
        self.wt_at(&self.dir)
    }

    /// `wt [--tree T]` in `dir`.
    pub fn wt_at(&self, dir: &Path) -> Cmd {
        let c = self.user.wt_in(dir);
        match &self.tree {
            Some(t) => c.args(["--tree", t]),
            None => c,
        }
    }

    /// Same checkout, commands scoped to nested tree `tree`.
    pub fn nested(&self, tree: &str) -> Repo {
        Repo {
            tree: Some(tree.to_string()),
            ..self.clone()
        }
    }

    /// Name of the tree commands operate on.
    pub fn tree_name(&self) -> &str {
        self.tree.as_deref().unwrap_or(&self.name)
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        let mut p = self.dir.clone();
        for part in rel.split('/') {
            p.push(part);
        }
        p
    }

    #[track_caller]
    pub fn write(&self, rel: &str, content: impl AsRef<[u8]>) {
        fsutil::write(&self.path(rel), content);
    }

    #[track_caller]
    pub fn read(&self, rel: &str) -> String {
        fsutil::read_string(&self.path(rel))
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    #[track_caller]
    pub fn remove(&self, rel: &str) {
        fsutil::remove(&self.path(rel));
    }

    /// `wt config set user.name/user.email`.
    #[track_caller]
    pub fn set_identity(&self) {
        self.user
            .wt_in(&self.dir)
            .args(["config", "set", "user.name", &self.user.display])
            .ok();
        self.user
            .wt_in(&self.dir)
            .args(["config", "set", "user.email", &self.user.email])
            .ok();
    }

    /// `wt remote add <name> <url> --tenant <tenant>`.
    #[track_caller]
    pub fn add_remote(&self, name: &str, url: &str, tenant: &str) {
        self.wt()
            .args(["remote", "add", name, url, "--tenant", tenant])
            .ok();
    }

    /// `wt snapshot -m msg` (must succeed).
    #[track_caller]
    pub fn snapshot(&self, msg: &str) -> CmdOutput {
        self.wt().args(["snapshot", "-m", msg]).ok()
    }

    /// Write files then snapshot; returns the new head id.
    #[track_caller]
    pub fn commit(&self, files: &[(&str, &str)], msg: &str) -> String {
        for (p, c) in files {
            self.write(p, c);
        }
        self.snapshot(msg);
        self.head_id()
    }

    #[track_caller]
    pub fn push(&self) -> CmdOutput {
        self.wt().arg("push").ok()
    }

    #[track_caller]
    pub fn pull(&self) -> CmdOutput {
        self.wt().arg("pull").ok()
    }

    /// `wt --json log <args>` -> snapshot records, newest first.
    #[track_caller]
    pub fn log(&self, args: &[&str]) -> Vec<Value> {
        let v = self.wt().arg("log").args(args).json();
        json::snapshots(&v)
    }

    /// Head snapshot record (panics if the branch is empty).
    #[track_caller]
    pub fn head(&self) -> Value {
        let log = self.log(&["-n", "1"]);
        assert!(
            !log.is_empty(),
            "expected a head snapshot in {}",
            self.dir.display()
        );
        log[0].clone()
    }

    #[track_caller]
    pub fn head_id(&self) -> String {
        json::str_field(&self.head(), "id")
    }

    /// `wt --json status`.
    #[track_caller]
    pub fn status_json(&self) -> Value {
        self.wt().arg("status").json()
    }

    /// The status report for this repo's tree.
    #[track_caller]
    pub fn status(&self) -> Value {
        let v = self.status_json();
        json::status_report(&v, Some(self.tree_name())).clone()
    }

    /// (path, kind) changes of this repo's tree, sorted by path.
    #[track_caller]
    pub fn changes(&self) -> Vec<(String, String)> {
        let mut c = json::changes(&self.status());
        c.sort();
        c
    }

    #[track_caller]
    pub fn assert_clean(&self) {
        let c = self.changes();
        assert!(
            c.is_empty(),
            "expected a clean working tree in {}, got changes {c:?}",
            self.dir.display()
        );
    }

    /// Current branch per `wt status`.
    #[track_caller]
    pub fn current_branch(&self) -> String {
        json::str_field(&self.status(), "branch")
    }

    /// User files in the checkout (excluding `.wt/` etc.).
    pub fn files(&self) -> BTreeMap<String, Vec<u8>> {
        fsutil::dir_files(&self.dir)
    }
}
