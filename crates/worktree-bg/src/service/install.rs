//! OS service registration so the daemon starts at login.
//!
//! Per-worktree, user-level (no admin required):
//! - **Linux**: systemd user unit (`~/.config/systemd/user/`)
//! - **macOS**: launchd agent (`~/Library/LaunchAgents/`)
//! - **Windows**: Scheduled Task (`schtasks /SC ONLOGON`)

use crate::error::BgError;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Stable per-worktree service identifier.
fn service_id(worktree: &Path) -> String {
    format!("wt-bg-{}", worktree_ipc::endpoint::worktree_hash(worktree))
}

fn daemon_exe() -> Result<PathBuf, BgError> {
    std::env::current_exe().map_err(BgError::Io)
}

fn run_checked(program: &str, args: &[&str], manual_hint: &str) -> Result<(), BgError> {
    let output = Command::new(program).args(args).output().map_err(|e| {
        BgError::Config(format!(
            "failed to invoke {program}: {e}. Run manually: {manual_hint}"
        ))
    })?;
    if !output.status.success() {
        return Err(BgError::Config(format!(
            "{program} failed: {}. Run manually: {manual_hint}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// Register the daemon for the given worktree with the OS service manager.
pub fn install(worktree: &Path) -> Result<(), BgError> {
    if cfg!(target_os = "windows") {
        install_windows(worktree)
    } else if cfg!(target_os = "linux") {
        install_systemd(worktree)
    } else if cfg!(target_os = "macos") {
        install_launchd(worktree)
    } else {
        Err(BgError::Config(format!(
            "unsupported platform for service installation: {}",
            std::env::consts::OS
        )))
    }
}

/// Remove the daemon's OS service registration.
pub fn uninstall(worktree: &Path) -> Result<(), BgError> {
    if cfg!(target_os = "windows") {
        uninstall_windows(worktree)
    } else if cfg!(target_os = "linux") {
        uninstall_systemd(worktree)
    } else if cfg!(target_os = "macos") {
        uninstall_launchd(worktree)
    } else {
        Err(BgError::Config(format!(
            "unsupported platform for service uninstallation: {}",
            std::env::consts::OS
        )))
    }
}

// --- Windows: user-level scheduled task ------------------------------------

fn install_windows(worktree: &Path) -> Result<(), BgError> {
    let id = service_id(worktree);
    let exe = daemon_exe()?;
    let action = format!(
        "\"{}\" run --worktree \"{}\"",
        exe.display(),
        worktree.display()
    );
    run_checked(
        "schtasks",
        &[
            "/Create", "/F", "/TN", &id, "/TR", &action, "/SC", "ONLOGON",
        ],
        &format!("schtasks /Create /TN {id} /TR '{action}' /SC ONLOGON"),
    )?;
    println!("installed scheduled task '{id}' (runs at logon)");
    Ok(())
}

fn uninstall_windows(worktree: &Path) -> Result<(), BgError> {
    let id = service_id(worktree);
    run_checked(
        "schtasks",
        &["/Delete", "/F", "/TN", &id],
        &format!("schtasks /Delete /F /TN {id}"),
    )?;
    println!("removed scheduled task '{id}'");
    Ok(())
}

// --- Linux: systemd user unit ------------------------------------------------

fn systemd_unit_path(id: &str) -> Result<PathBuf, BgError> {
    let config = dirs_config()?;
    Ok(config
        .join("systemd")
        .join("user")
        .join(format!("{id}.service")))
}

fn dirs_config() -> Result<PathBuf, BgError> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| BgError::Config("cannot resolve user config directory".into()))
}

fn install_systemd(worktree: &Path) -> Result<(), BgError> {
    let id = service_id(worktree);
    let exe = daemon_exe()?;
    let unit = format!(
        "[Unit]\nDescription=W0rkTree daemon for {root}\n\n\
         [Service]\nExecStart={exe} run --worktree {root}\nRestart=on-failure\n\n\
         [Install]\nWantedBy=default.target\n",
        root = worktree.display(),
        exe = exe.display(),
    );
    let path = systemd_unit_path(&id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, unit)?;
    run_checked(
        "systemctl",
        &["--user", "daemon-reload"],
        "systemctl --user daemon-reload",
    )?;
    run_checked(
        "systemctl",
        &["--user", "enable", "--now", &id],
        &format!("systemctl --user enable --now {id}"),
    )?;
    println!("installed systemd user unit '{id}'");
    Ok(())
}

fn uninstall_systemd(worktree: &Path) -> Result<(), BgError> {
    let id = service_id(worktree);
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", &id])
        .output();
    let path = systemd_unit_path(&id)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    println!("removed systemd user unit '{id}'");
    Ok(())
}

// --- macOS: launchd agent -----------------------------------------------------

fn launchd_plist_path(id: &str) -> Result<PathBuf, BgError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| BgError::Config("cannot resolve HOME".into()))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("dev.w0rktree.{id}.plist")))
}

fn install_launchd(worktree: &Path) -> Result<(), BgError> {
    let id = service_id(worktree);
    let exe = daemon_exe()?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>dev.w0rktree.{id}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>run</string>
        <string>--worktree</string>
        <string>{root}</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
</dict>
</plist>
"#,
        exe = exe.display(),
        root = worktree.display(),
    );
    let path = launchd_plist_path(&id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, plist)?;
    run_checked(
        "launchctl",
        &["load", &path.display().to_string()],
        &format!("launchctl load {}", path.display()),
    )?;
    println!("installed launchd agent 'dev.w0rktree.{id}'");
    Ok(())
}

fn uninstall_launchd(worktree: &Path) -> Result<(), BgError> {
    let id = service_id(worktree);
    let path = launchd_plist_path(&id)?;
    if path.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", &path.display().to_string()])
            .output();
        std::fs::remove_file(&path)?;
    }
    println!("removed launchd agent 'dev.w0rktree.{id}'");
    Ok(())
}
