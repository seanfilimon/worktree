//! `worktree-bg` binary entry point.

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Mutex;

use clap::{Parser, Subcommand};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};
use worktree_bg::service::{service_definition, ServicePlatform};
use worktree_bg::RunOutcome;

/// Log file name inside `$WT_HOME`.
const LOG_FILE: &str = "bg.log";
/// `bg.log` is rotated to `bg.log.old` at startup once it exceeds this size.
const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "worktree-bg", version, about = "W0rkTree background daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon until Ctrl-C or a `Shutdown` request.
    ///
    /// Logs go to $WT_HOME/bg.log (level from WT_LOG, default "info").
    Run {
        /// Also log to stderr (for running interactively).
        #[arg(long)]
        foreground: bool,
    },
    /// Print a login-service definition that starts the daemon automatically.
    Service {
        /// Service manager to target (default: this platform's).
        #[arg(long, value_enum)]
        platform: Option<ServicePlatform>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Run { foreground } => run(foreground),
        Command::Service { platform } => {
            let exe = match std::env::current_exe() {
                Ok(exe) => exe,
                Err(e) => {
                    eprintln!("worktree-bg: cannot locate own executable: {e}");
                    return ExitCode::FAILURE;
                }
            };
            print!(
                "{}",
                service_definition(platform.unwrap_or_else(ServicePlatform::current), &exe)
            );
            ExitCode::SUCCESS
        }
    }
}

fn run(foreground: bool) -> ExitCode {
    let home = worktree_sdk::config::wt_home();
    if let Err(e) = init_logging(&home, foreground) {
        eprintln!("worktree-bg: cannot open log in {}: {e}", home.display());
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("worktree-bg: cannot start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(worktree_bg::run(&home, shutdown_signal())) {
        Ok(RunOutcome::Stopped) => ExitCode::SUCCESS,
        Ok(RunOutcome::AlreadyRunning(info)) => {
            eprintln!(
                "worktree-bg is already running (pid {}, port {})",
                info.pid, info.port
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!(error = %e, "daemon failed");
            eprintln!("worktree-bg: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Completes on Ctrl-C (and SIGTERM on Unix). If no handler can be
/// installed, never completes: the daemon then stops only via IPC.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %e, "cannot listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

fn init_logging(home: &Path, foreground: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(home)?;
    let file = open_log(&home.join(LOG_FILE))?;
    let filter = EnvFilter::try_from_env("WT_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = fmt::layer().with_ansi(false).with_writer(Mutex::new(file));
    let stderr_layer = foreground.then(|| fmt::layer().with_writer(std::io::stderr));
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .init();
    Ok(())
}

fn open_log(path: &Path) -> std::io::Result<File> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        // Best effort: if rotation fails we keep appending to the old file.
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    OpenOptions::new().create(true).append(true).open(path)
}
