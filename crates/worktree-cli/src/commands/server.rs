use super::ServerAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: ServerAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ServerAction::Start => {
            match WorktreeEngine::open(Path::new(".")) {
                Ok(engine) => {
                    let pid_file = engine.wt_dir().join("cache").join("bgprocess.pid");
                    if pid_file.exists() {
                        let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
                        format::print_warning(&format!(
                            "Server may already be running (PID {})",
                            pid.trim()
                        ));
                        return Ok(());
                    }

                    let binary = find_server_binary();
                    if !binary.exists() {
                        format::print_error(&format!(
                            "worktree-server binary not found at {}",
                            binary.display()
                        ));
                        return Ok(());
                    }

                    let cache_dir = engine.wt_dir().join("cache");
                    std::fs::create_dir_all(&cache_dir)?;

                    let log_path = cache_dir.join("server.log");
                    let log_file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                        .map_err(|e| format!("cannot open log file: {e}"))?;

                    let child = std::process::Command::new(&binary)
                        .stdout(std::process::Stdio::from(
                            log_file
                                .try_clone()
                                .map_err(|e| format!("log clone: {e}"))?,
                        ))
                        .stderr(std::process::Stdio::from(log_file))
                        .spawn()
                        .map_err(|e| format!("failed to spawn server: {e}"))?;

                    let pid = child.id();
                    // Detach: dropping Child does not kill the process on any platform
                    std::mem::forget(child);

                    std::fs::write(&pid_file, pid.to_string())?;

                    format::print_success(&format!("Server started (PID {})", pid));
                    format::print_kv("Address", "http://127.0.0.1:9876");
                    format::print_kv("Log", &log_path.display().to_string());
                    format::print_info("Watch logs: wt server logs  |  Stop: wt server stop");
                }
                Err(e) => {
                    format::print_error(&format!("Cannot start: {}", e));
                }
            }
        }

        ServerAction::Stop => match WorktreeEngine::open(Path::new(".")) {
            Ok(engine) => {
                let pid_file = engine.wt_dir().join("cache").join("bgprocess.pid");
                if !pid_file.exists() {
                    format::print_info("No server is currently running.");
                    return Ok(());
                }

                let pid = std::fs::read_to_string(&pid_file)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                std::fs::remove_file(&pid_file)?;

                kill_process(&pid);
                format::print_success(&format!("Server stopped (PID {})", pid));
            }
            Err(e) => {
                format::print_error(&format!("Cannot stop: {}", e));
            }
        },

        ServerAction::Status => match WorktreeEngine::open(Path::new(".")) {
            Ok(engine) => {
                let state = worktree_sdk::engine::status::load_state(&engine)?;
                format::print_header("Server Status");
                format::print_kv("Worktree", &state.name);

                let pid_file = engine.wt_dir().join("cache").join("bgprocess.pid");
                if pid_file.exists() {
                    let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
                    format::print_kv("Status", "running");
                    format::print_kv("PID", pid.trim());
                    format::print_kv("Address", "http://127.0.0.1:9876");
                } else {
                    format::print_kv("Status", "stopped");
                }

                let config_content = worktree_sdk::engine::config::read_config(&engine)?;
                if config_content.contains("auto = true") {
                    format::print_kv("Auto-sync", "enabled");
                } else {
                    format::print_kv("Auto-sync", "disabled");
                }

                format::print_kv("Trees", &state.trees.len().to_string());
                let total_snapshots: usize = state.trees.iter().map(|t| t.snapshots.len()).sum();
                format::print_kv("Total snapshots", &total_snapshots.to_string());
            }
            Err(e) => {
                format::print_error(&format!("Not in a worktree: {}", e));
            }
        },

        ServerAction::Logs => match WorktreeEngine::open(Path::new(".")) {
            Ok(engine) => {
                let log_path = engine.wt_dir().join("cache").join("server.log");
                if !log_path.exists() {
                    format::print_info("No server log yet. Start the server first.");
                    return Ok(());
                }
                format::print_info(&format!(
                    "Streaming {} (Ctrl-C to stop)",
                    log_path.display()
                ));
                stream_log(&log_path)?;
            }
            Err(e) => {
                format::print_error(&format!("Not in a worktree: {}", e));
            }
        },
    }
    Ok(())
}

fn stream_log(log_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};

    let file = std::fs::File::open(log_path)?;
    let mut reader = BufReader::new(file);

    // Print existing content
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        print!("{line}");
        line.clear();
    }

    // Poll for new lines (Ctrl-C interrupts the process)
    loop {
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            // Re-seek to current position to detect new content
            let pos = reader.stream_position()?;
            reader.seek(SeekFrom::Start(pos))?;
        } else {
            print!("{line}");
            line.clear();
        }
    }
}

fn find_server_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe()
        .unwrap_or_default()
        .parent()
        .unwrap_or(Path::new("."))
        .join("worktree-server");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    path
}

fn kill_process(pid: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", pid])
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", pid])
            .output();
    }
}
