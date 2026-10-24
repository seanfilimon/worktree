//! Login-service definitions for starting the daemon automatically.
//!
//! `worktree-bg service` prints the definition for the current platform (a
//! systemd user unit, a launchd agent plist, or a Task Scheduler command);
//! installing it is left to the user, so nothing here touches the system.

use std::fmt::Write as _;
use std::path::Path;

/// Service manager to generate a definition for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ServicePlatform {
    /// systemd user unit (Linux).
    Systemd,
    /// launchd agent (macOS).
    Launchd,
    /// Task Scheduler logon task (Windows).
    Windows,
}

impl ServicePlatform {
    /// The platform this binary was built for.
    pub fn current() -> Self {
        if cfg!(windows) {
            ServicePlatform::Windows
        } else if cfg!(target_os = "macos") {
            ServicePlatform::Launchd
        } else {
            ServicePlatform::Systemd
        }
    }
}

/// launchd label / systemd unit name / scheduled task name.
pub const SERVICE_NAME: &str = "worktree-bg";
const LAUNCHD_LABEL: &str = "dev.w0rktree.bg";

/// Service definition (with install instructions as comments) that runs
/// `<exe> run` at login and restarts it if it crashes.
pub fn service_definition(platform: ServicePlatform, exe: &Path) -> String {
    let exe = exe.to_string_lossy();
    match platform {
        ServicePlatform::Systemd => systemd_unit(&exe),
        ServicePlatform::Launchd => launchd_plist(&exe),
        ServicePlatform::Windows => windows_task(&exe),
    }
}

fn systemd_unit(exe: &str) -> String {
    let quoted = exe
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# Save as ~/.config/systemd/user/{SERVICE_NAME}.service, then run:"
    );
    let _ = writeln!(
        s,
        "#   systemctl --user daemon-reload && systemctl --user enable --now {SERVICE_NAME}"
    );
    s.push_str("[Unit]\nDescription=W0rkTree background daemon (auto-snapshots)\n\n");
    let _ = writeln!(
        s,
        "[Service]\nExecStart=\"{quoted}\" run\nRestart=on-failure\nRestartSec=5\n"
    );
    s.push_str("[Install]\nWantedBy=default.target\n");
    s
}

fn launchd_plist(exe: &str) -> String {
    let escaped = exe
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- Save as ~/Library/LaunchAgents/{LAUNCHD_LABEL}.plist, then run:
       launchctl load -w ~/Library/LaunchAgents/{LAUNCHD_LABEL}.plist -->
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{escaped}</string>
    <string>run</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
</dict>
</plist>
"#
    )
}

fn windows_task(exe: &str) -> String {
    // schtasks takes /TR as one argument; the inner quotes protect spaces.
    let escaped = exe.replace('"', "");
    format!(
        "REM Run in a terminal to start {SERVICE_NAME} at every logon:\n\
         schtasks /Create /F /SC ONLOGON /RL LIMITED /TN \"W0rkTree\\{SERVICE_NAME}\" /TR \"\\\"{escaped}\\\" run\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_unit_runs_daemon_and_quotes_path() {
        let unit = service_definition(ServicePlatform::Systemd, Path::new("/opt/w t/worktree-bg"));
        assert!(
            unit.contains("ExecStart=\"/opt/w t/worktree-bg\" run"),
            "{unit}"
        );
        assert!(unit.contains("[Install]\nWantedBy=default.target"));
        assert!(unit.contains("Restart=on-failure"));
    }

    #[test]
    fn systemd_unit_escapes_specifiers() {
        let unit = service_definition(ServicePlatform::Systemd, Path::new("/opt/100%/bg"));
        assert!(unit.contains("ExecStart=\"/opt/100%%/bg\" run"), "{unit}");
    }

    #[test]
    fn launchd_plist_escapes_xml() {
        let plist = service_definition(
            ServicePlatform::Launchd,
            Path::new("/Apps/R&D <x>/worktree-bg"),
        );
        assert!(
            plist.contains("<string>/Apps/R&amp;D &lt;x&gt;/worktree-bg</string>"),
            "{plist}"
        );
        assert!(plist.contains("<string>run</string>"));
        assert!(plist.contains(LAUNCHD_LABEL));
    }

    #[test]
    fn windows_task_quotes_executable() {
        let cmd = service_definition(
            ServicePlatform::Windows,
            Path::new(r"C:\Program Files\W0rkTree\worktree-bg.exe"),
        );
        assert!(
            cmd.contains(r#"/TR "\"C:\Program Files\W0rkTree\worktree-bg.exe\" run""#),
            "{cmd}"
        );
        assert!(cmd.contains("/SC ONLOGON"));
    }
}
