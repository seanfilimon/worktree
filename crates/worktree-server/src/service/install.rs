use crate::error::ServerError;

/// Install the Worktree server as a system service / daemon.
///
/// Uses platform detection to choose the appropriate service manager:
/// - **Linux**: systemd unit file
/// - **macOS**: launchd plist
/// - **Windows**: Windows Service via `sc.exe`
pub fn install_service() -> Result<(), ServerError> {
    if cfg!(target_os = "linux") {
        install_systemd_service()
    } else if cfg!(target_os = "macos") {
        install_launchd_service()
    } else if cfg!(target_os = "windows") {
        install_windows_service()
    } else {
        Err(ServerError::Config(format!(
            "unsupported platform for service installation: {}",
            std::env::consts::OS
        )))
    }
}

/// Uninstall the Worktree server system service / daemon.
///
/// Mirrors `install_service` with platform-specific teardown logic.
pub fn uninstall_service() -> Result<(), ServerError> {
    if cfg!(target_os = "linux") {
        uninstall_systemd_service()
    } else if cfg!(target_os = "macos") {
        uninstall_launchd_service()
    } else if cfg!(target_os = "windows") {
        uninstall_windows_service()
    } else {
        Err(ServerError::Config(format!(
            "unsupported platform for service uninstallation: {}",
            std::env::consts::OS
        )))
    }
}

/// Returns the expected service name used across all platforms.
pub fn service_name() -> &'static str {
    "worktree-server"
}

// ---------------------------------------------------------------------------
// Platform-specific install helpers
// ---------------------------------------------------------------------------

fn install_systemd_service() -> Result<(), ServerError> {
    // Write a systemd unit file to /etc/systemd/system/worktree-server.service
    // then run `systemctl daemon-reload && systemctl enable worktree-server`.
    todo!("install systemd unit file for worktree-server")
}

fn install_launchd_service() -> Result<(), ServerError> {
    // Write a launchd plist to ~/Library/LaunchAgents/com.worktree.server.plist
    // then run `launchctl load <plist>`.
    todo!("install launchd plist for worktree-server")
}

fn install_windows_service() -> Result<(), ServerError> {
    // Register with the Windows Service Control Manager via `sc.exe create`.
    todo!("install Windows service for worktree-server")
}

// ---------------------------------------------------------------------------
// Platform-specific uninstall helpers
// ---------------------------------------------------------------------------

fn uninstall_systemd_service() -> Result<(), ServerError> {
    // Run `systemctl disable --now worktree-server` then remove the unit file.
    todo!("uninstall systemd unit file for worktree-server")
}

fn uninstall_launchd_service() -> Result<(), ServerError> {
    // Run `launchctl unload <plist>` then remove the plist file.
    todo!("uninstall launchd plist for worktree-server")
}

fn uninstall_windows_service() -> Result<(), ServerError> {
    // Run `sc.exe delete worktree-server`.
    todo!("uninstall Windows service for worktree-server")
}
