//! Locating, building and starting the Go `worktree-server` (the same way
//! the e2e harness does: `crates/worktree-e2e/src/{bins,server}.rs`).

use std::fs::File;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Readiness line printed by the server on startup.
const READY_LINE: &str = "worktree-server listening on";
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// Repository root (two levels above this crate's manifest).
pub fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest.to_path_buf())
}

fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Directories searched for a prebuilt server: the directory of this
/// executable, then `$CARGO_TARGET_DIR` / `target` debug and release.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        dirs.push(dir);
    }
    let mut targets = Vec::new();
    if let Some(t) = std::env::var_os("CARGO_TARGET_DIR") {
        let t = PathBuf::from(t);
        targets.push(if t.is_absolute() {
            t
        } else {
            workspace_root().join(t)
        });
    }
    targets.push(workspace_root().join("target"));
    for t in targets {
        for profile in ["debug", "release"] {
            let d = t.join(profile);
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    }
    dirs
}

/// The server binary: `explicit`, else `WT_SERVER_BIN`, else a prebuilt
/// `worktree-server` next to this binary or in the target directory, else
/// `go build` into the directory of this binary.
pub fn locate(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return if p.is_file() {
            Ok(p.to_path_buf())
        } else {
            Err(format!("--server-bin {} does not exist", p.display()))
        };
    }
    if let Some(p) = std::env::var_os("WT_SERVER_BIN") {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!("WT_SERVER_BIN={} does not exist", p.display()))
        };
    }
    let file = exe_name("worktree-server");
    for d in candidate_dirs() {
        let p = d.join(&file);
        if p.is_file() {
            return Ok(p);
        }
    }
    build()
}

fn build() -> Result<PathBuf, String> {
    let out_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| workspace_root().join("target").join("debug"));
    let out = out_dir.join(exe_name("worktree-server"));
    let src = workspace_root().join("services").join("server-go");
    eprintln!("building {} (go build) ...", out.display());
    let output = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(&out)
        .arg(".")
        .current_dir(&src)
        .env("CGO_ENABLED", "0")
        .output()
        .map_err(|e| {
            format!(
                "failed to run `go build` in {} (is Go installed?): {e}",
                src.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "`go build -o {} .` in {} failed ({}):\n{}{}",
            out.display(),
            src.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(out)
}

/// A server process started by the bench; killed on drop (only this child).
pub struct Server {
    child: Option<Child>,
    pub url: String,
    pub admin_user: String,
    pub admin_password: String,
    pub data_dir: PathBuf,
    pub stderr_path: PathBuf,
}

fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind 127.0.0.1:0: {e}"))?;
    l.local_addr()
        .map(|a| a.port())
        .map_err(|e| format!("local_addr: {e}"))
}

fn read_lossy(p: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).into_owned()
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

impl Server {
    /// Start `bin` with its data under `dir` on a free localhost port, with
    /// the flags the e2e harness uses plus `--rate-limit 0` (a single bench
    /// user would otherwise hit the per-principal limit) and `extra`.
    pub fn start(bin: &Path, dir: &Path, extra: &[String]) -> Result<Server, String> {
        let admin_password = format!("Adm-{}-9z", uuid::Uuid::new_v4().simple());
        let mut last = String::new();
        for attempt in 1..=3 {
            match Self::try_start(bin, dir, attempt, &admin_password, extra) {
                Ok(s) => return Ok(s),
                Err((retry, e)) => {
                    last = e;
                    if !retry {
                        break;
                    }
                }
            }
        }
        Err(format!("failed to start {}: {last}", bin.display()))
    }

    fn try_start(
        bin: &Path,
        dir: &Path,
        attempt: u32,
        admin_password: &str,
        extra: &[String],
    ) -> Result<Server, (bool, String)> {
        let port = free_port().map_err(|e| (true, e))?;
        let addr = format!("127.0.0.1:{port}");
        let data_dir = dir.join(format!("server-data-{attempt}"));
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| (false, format!("create {}: {e}", data_dir.display())))?;
        let stdout_path = dir.join(format!("server-{attempt}.stdout.log"));
        let stderr_path = dir.join(format!("server-{attempt}.stderr.log"));
        let out = File::create(&stdout_path).map_err(|e| (false, e.to_string()))?;
        let err = File::create(&stderr_path).map_err(|e| (false, e.to_string()))?;
        let child = Command::new(bin)
            .arg("--addr")
            .arg(&addr)
            .arg("--data")
            .arg(&data_dir)
            .arg("--admin-user")
            .arg("admin")
            .arg("--admin-password")
            .arg(admin_password)
            .arg("--rate-limit")
            .arg("0")
            .args(extra)
            .current_dir(dir)
            .env_remove("WT_ADDR")
            .env_remove("WT_DATA")
            .env_remove("WT_ADMIN_USER")
            .env_remove("WT_ADMIN_PASSWORD")
            .env_remove("WT_ADMIN_DIR")
            .env_remove("WT_ALLOW_REGISTRATION")
            .env_remove("WT_RATE_LIMIT")
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|e| (false, format!("spawn {}: {e}", bin.display())))?;
        let mut server = Server {
            child: Some(child),
            url: format!("http://{addr}"),
            admin_user: "admin".into(),
            admin_password: admin_password.to_string(),
            data_dir,
            stderr_path,
        };
        let started = Instant::now();
        loop {
            if let Some(child) = server.child.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    server.child = None;
                    let logs = read_lossy(&server.stderr_path);
                    let lower = logs.to_lowercase();
                    let retry = lower.contains("address already in use")
                        || lower.contains("only one usage of each socket address")
                        || lower.contains("bind:");
                    return Err((
                        retry,
                        format!(
                            "server exited during startup ({status})\n{}",
                            tail(&logs, 40)
                        ),
                    ));
                }
            }
            if read_lossy(&stdout_path).contains(READY_LINE) {
                return Ok(server);
            }
            if started.elapsed() > START_TIMEOUT {
                return Err((
                    false,
                    format!(
                        "server not ready after {START_TIMEOUT:?} on {addr}\n{}",
                        tail(&read_lossy(&server.stderr_path), 40)
                    ),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Last lines of the server's stderr (for error reports).
    pub fn log_tail(&self) -> String {
        tail(&read_lossy(&self.stderr_path), 40)
    }

    /// Kill the child (idempotent).
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}
