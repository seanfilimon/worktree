use super::ServerAction;
use crate::output::format;
use std::path::PathBuf;
use worktree_sdk::Client;

pub async fn execute(action: ServerAction) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::open_current()?;
    match action {
        ServerAction::Start => {
            if client.is_daemon_backed() {
                format::print_info("Daemon is already running.");
                return Ok(());
            }
            format::print_info("Starting worktree background daemon...");

            // No pipes to the launcher: the detached daemon outlives it and a
            // pipe handle inherited down the spawn chain would keep our read
            // side open forever (observed hang on Windows). The launcher logs
            // to `.wt/cache/bgprocess.log`; readiness is polled over IPC.
            let daemon = daemon_binary()?;
            let status = std::process::Command::new(&daemon)
                .arg("start")
                .arg("--worktree")
                .arg(client.root())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;

            let log_hint = client.wt_dir().join("cache").join("bgprocess.log");
            if !status.success() {
                return Err(format!("failed to start daemon (see {})", log_hint.display()).into());
            }

            // Confirm the daemon answers before declaring success.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let info = loop {
                match Client::open(client.root()).and_then(|c| c.daemon_info()) {
                    Ok(info) => break info,
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        return Err(format!(
                            "daemon did not become ready: {e} (see {})",
                            log_hint.display()
                        )
                        .into())
                    }
                }
            };

            format::print_success(&format!("Daemon started (pid {}).", info.pid));
            format::print_info("Auto-snapshot is active; `wt` commands now go through the daemon.");
        }
        ServerAction::Stop => {
            if client.daemon_stop()? {
                format::print_success("Daemon stopping.");
            } else {
                format::print_info("No daemon is running for this worktree.");
            }
        }
        ServerAction::Status => match client.daemon_info() {
            Ok(info) => {
                format::print_header("Background Daemon");
                format::print_kv("Status", "running");
                format::print_kv("PID", &info.pid.to_string());
                format::print_kv("Version", &info.version);
                format::print_kv("Worktree", &info.root);
                format::print_kv("Uptime", &format!("{}s", info.uptime_secs));
                format::print_kv("Auto-snapshots", &info.snapshots_created.to_string());
                format::print_kv(
                    "Watcher",
                    if info.watcher_active {
                        "active"
                    } else {
                        "inactive"
                    },
                );
            }
            Err(worktree_sdk::SdkError::DaemonUnavailable) => {
                format::print_header("Background Daemon");
                format::print_kv("Status", "stopped");
                format::print_info("Start it with `wt server start`.");
            }
            Err(e) => return Err(e.into()),
        },
    }
    Ok(())
}

/// Locate the `worktree-bg` binary: next to the `wt` executable first, then
/// on PATH.
fn daemon_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let name = if cfg!(windows) {
        "worktree-bg.exe"
    } else {
        "worktree-bg"
    };
    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            let sibling = dir.join(name);
            if sibling.is_file() {
                return Ok(sibling);
            }
        }
    }
    // Fall back to PATH resolution by the OS.
    Ok(PathBuf::from(name))
}
