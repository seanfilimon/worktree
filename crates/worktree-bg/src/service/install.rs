use crate::error::BgError;

/// Install the Worktree bg daemon as a system service.
///
/// Uses platform detection to choose the appropriate service manager:
/// - **Linux**: systemd unit file
/// - **macOS**: launchd plist
/// - **Windows**: Windows Service via `sc.exe`
pub fn install_service() -> Result<(), BgError> {
    if cfg!(target_os = "linux") {
        install_systemd_service()
    } else if cfg!(target_os = "macos") {
        install_launchd_service()
    } else if cfg!(target_os = "windows") {
        install_windows_service()
    } else {
        Err(BgError::Config(format!(
            "unsupported platform for service installation: {}",
            std::env::consts::OS
        )))
    }
}

/// Uninstall the Worktree bg daemon system service.
///
/// Mirrors `install_service` with platform-specific teardown logic.
pub fn uninstall_service() -> Result<(), BgError> {
    if cfg!(target_os = "linux") {
        uninstall_systemd_service()
    } else if cfg!(target_os = "macos") {
        uninstall_launchd_service()
    } else if cfg!(target_os = "windows") {
        uninstall_windows_service()
    } else {
        Err(BgError::Config(format!(
            "unsupported platform for service uninstallation: {}",
            std::env::consts::OS
        )))
    }
}

/// Returns the expected service name used across all platforms.
pub fn service_name() -> &'static str {
    "worktree-bg"
}

// ---------------------------------------------------------------------------
// Platform-specific install helpers
// ---------------------------------------------------------------------------

fn install_systemd_service() -> Result<(), BgError> {
    // Write a systemd unit file to /etc/systemd/system/worktree-bg.service
    // then run `systemctl daemon-reload && systemctl enable worktree-bg`.
    todo!("install systemd unit file for worktree-bg")
}

fn install_launchd_service() -> Result<(), BgError> {
    // Write a launchd plist to ~/Library/LaunchAgents/com.worktree.bg.plist
    // then run `launchctl load <plist>`.
    todo!("install launchd plist for worktree-bg")
}

fn install_windows_service() -> Result<(), BgError> {
    // Register with the Windows Service Control Manager via `sc.exe create`.
    todo!("install Windows service for worktree-bg")
}

// ---------------------------------------------------------------------------
// Platform-specific uninstall helpers
// ---------------------------------------------------------------------------

fn uninstall_systemd_service() -> Result<(), BgError> {
    // Run `systemctl disable --now worktree-bg` then remove the unit file.
    todo!("uninstall systemd unit file for worktree-bg")
}

fn uninstall_launchd_service() -> Result<(), BgError> {
    // Run `launchctl unload <plist>` then remove the plist file.
    todo!("uninstall launchd plist for worktree-bg")
}

fn uninstall_windows_service() -> Result<(), BgError> {
    // Run `sc.exe delete worktree-bg`.
    todo!("uninstall Windows service for worktree-bg")
}
