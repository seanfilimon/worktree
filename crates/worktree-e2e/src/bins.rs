//! Locating (and for the Go server, building) the binaries under test.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Repository root (two levels above this crate's manifest).
pub fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest.to_path_buf())
}

/// `name` + the platform executable suffix (`.exe` on Windows).
pub fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// The cargo profile directory the running test binary was built into
/// (`target/<profile>` — the test exe lives in `target/<profile>/deps`).
pub fn current_profile_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let deps = exe.parent()?;
    let profile = if deps.file_name().map(|n| n == "deps").unwrap_or(false) {
        deps.parent()?
    } else {
        deps
    };
    Some(profile.to_path_buf())
}

/// Directories searched for `wt` / `worktree-bg`, most specific first.
pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(p) = current_profile_dir() {
        dirs.push(p);
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

fn locate(env_var: &str, name: &str) -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os(env_var) {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!("{env_var}={} does not exist", p.display()))
        };
    }
    let file = exe_name(name);
    let dirs = candidate_dirs();
    for d in &dirs {
        let p = d.join(&file);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(format!(
        "could not find `{file}`; set {env_var} or build it first. Searched:\n  {}",
        dirs.iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join("\n  ")
    ))
}

/// Path to the `wt` CLI binary (panics with build instructions if missing).
pub fn wt_bin() -> PathBuf {
    static BIN: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BIN.get_or_init(|| locate("WT_BIN", "wt"))
        .clone()
        .unwrap_or_else(|e| panic!("{e}\n(hint: cargo build -p worktree-cli)"))
}

/// Path to `worktree-bg`, if it can be found.
pub fn try_bg_bin() -> Option<PathBuf> {
    static BIN: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BIN.get_or_init(|| locate("WT_BG_BIN", "worktree-bg"))
        .clone()
        .ok()
}

/// Path to `worktree-bg` (panics with build instructions if missing).
pub fn bg_bin() -> PathBuf {
    locate("WT_BG_BIN", "worktree-bg")
        .unwrap_or_else(|e| panic!("{e}\n(hint: cargo build -p worktree-bg)"))
}

/// Path to the Go server binary: `WT_SERVER_BIN`, else built once per test
/// process (and serialized across processes with a file lock) via
/// `go build -o <profile dir>/worktree-server(.exe) .`.
pub fn server_bin() -> PathBuf {
    static BIN: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(p) = std::env::var_os("WT_SERVER_BIN") {
            let p = PathBuf::from(p);
            return if p.is_file() {
                Ok(p)
            } else {
                Err(format!("WT_SERVER_BIN={} does not exist", p.display()))
            };
        }
        build_server()
    })
    .clone()
    .unwrap_or_else(|e| panic!("{e}"))
}

fn build_server() -> Result<PathBuf, String> {
    let out_dir = current_profile_dir()
        .filter(|d| d.is_dir())
        .unwrap_or_else(|| workspace_root().join("target").join("debug"));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    let out = out_dir.join(exe_name("worktree-server"));
    let src = workspace_root().join("services").join("server-go");

    // Serialize concurrent builds from several test processes.
    let lock_path = out_dir.join("worktree-server.build.lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("open {}: {e}", lock_path.display()))?;
    lock.lock()
        .map_err(|e| format!("lock {}: {e}", lock_path.display()))?;

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
    let _ = lock.unlock();
    if !output.status.success() {
        return Err(format!(
            "`go build -o {} .` in {} failed ({}):\n{}\n{}",
            out.display(),
            src.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(out)
}
