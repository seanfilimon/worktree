use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use worktree_bg::error::BgError;

#[derive(Parser)]
#[command(
    name = "worktree-bg",
    about = "W0rkTree background daemon — file watching, auto-snapshots, sync",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the daemon in the foreground (blocks until shutdown)
    Run {
        /// Worktree root (defaults to the current directory's worktree)
        #[arg(long)]
        worktree: Option<PathBuf>,
        /// Write logs to this file instead of stderr (used by `start`: the
        /// detached daemon has no usable std handles by design)
        #[arg(long)]
        log_file: Option<PathBuf>,
    },
    /// Start the daemon detached in the background
    Start {
        #[arg(long)]
        worktree: Option<PathBuf>,
    },
    /// Ask the running daemon to shut down
    Stop {
        #[arg(long)]
        worktree: Option<PathBuf>,
    },
    /// Query the running daemon
    Status {
        #[arg(long)]
        worktree: Option<PathBuf>,
    },
    /// Register the daemon with the OS service manager (auto-start)
    Install {
        #[arg(long)]
        worktree: Option<PathBuf>,
    },
    /// Remove the OS service registration
    Uninstall {
        #[arg(long)]
        worktree: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    let level = std::env::var("WT_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(level)
        .with_target(false);
    if let Cmd::Run {
        log_file: Some(path),
        ..
    } = &cli.command
    {
        match std::fs::File::create(path) {
            Ok(file) => builder
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .init(),
            Err(e) => {
                eprintln!("worktree-bg: cannot open log file {}: {e}", path.display());
                std::process::exit(1);
            }
        }
    } else {
        builder.init();
    }

    if let Err(e) = run(cli) {
        eprintln!("worktree-bg: {e}");
        std::process::exit(1);
    }
}

fn resolve_root(worktree: Option<PathBuf>) -> Result<PathBuf, BgError> {
    let start = worktree.unwrap_or_else(|| PathBuf::from("."));
    let engine = worktree_engine::WorktreeEngine::open(&start)
        .map_err(|e| BgError::Config(format!("{e} (looked from {})", start.display())))?;
    Ok(engine.root().to_path_buf())
}

fn run(cli: Cli) -> Result<(), BgError> {
    match cli.command {
        Cmd::Run { worktree, .. } => {
            let root = resolve_root(worktree)?;
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(worktree_bg::service::daemon::run(root))
        }
        Cmd::Start { worktree } => start_detached(&resolve_root(worktree)?),
        Cmd::Stop { worktree } => stop(&resolve_root(worktree)?),
        Cmd::Status { worktree } => status(&resolve_root(worktree)?),
        Cmd::Install { worktree } => {
            worktree_bg::service::install::install(&resolve_root(worktree)?)
        }
        Cmd::Uninstall { worktree } => {
            worktree_bg::service::install::uninstall(&resolve_root(worktree)?)
        }
    }
}

/// Spawn `worktree-bg run` detached, log to `.wt/cache/bgprocess.log`, and
/// wait until the IPC endpoint answers.
///
/// The daemon must not inherit **any** handle from this process: it outlives
/// the whole launch chain, and an inherited pipe handle (e.g. a script
/// capturing `wt server start`'s output) would keep that pipe open forever —
/// hanging the caller. Hence the daemon logs to a file it opens itself, and
/// on Windows it is spawned with `bInheritHandles = FALSE`.
fn start_detached(root: &Path) -> Result<(), BgError> {
    // Refuse a double start.
    if try_call(root, worktree_ipc::Command::DaemonInfo).is_ok() {
        println!("daemon already running for {}", root.display());
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let log_path = root.join(".wt").join("cache").join("bgprocess.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let pid = spawn_daemon(&exe, root, &log_path)?;

    // Wait for the endpoint to come up.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match try_call(root, worktree_ipc::Command::DaemonInfo) {
            Ok(_) => break,
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                return Err(BgError::Engine(format!(
                    "daemon did not come up within 10s (pid {pid}, log at {}): {e}",
                    log_path.display()
                )))
            }
        }
    }

    println!("daemon started (pid {pid}) for {}", root.display());
    Ok(())
}

/// Windows: raw `CreateProcessW` with `bInheritHandles = FALSE` so the
/// daemon starts with a clean handle table (std's `Command` always inherits
/// marked handles, which is exactly the leak we must prevent).
#[cfg(windows)]
#[allow(unsafe_code)] // FFI: std::process cannot express bInheritHandles=FALSE
fn spawn_daemon(exe: &Path, root: &Path, log_file: &Path) -> Result<u32, BgError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS,
        PROCESS_INFORMATION, STARTUPINFOW,
    };

    fn quoted(path: &Path) -> String {
        format!("\"{}\"", path.display())
    }

    let command_line = format!(
        "{} run --worktree {} --log-file {}",
        quoted(exe),
        quoted(root),
        quoted(log_file)
    );
    let mut wide: Vec<u16> = std::ffi::OsString::from(&command_line)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let created = unsafe {
        CreateProcessW(
            std::ptr::null(), // application resolved from the command line
            wide.as_mut_ptr(),
            std::ptr::null(), // default process security
            std::ptr::null(), // default thread security
            0,                // bInheritHandles = FALSE — the whole point
            DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP,
            std::ptr::null(), // inherit environment (WT_* variables)
            std::ptr::null(), // inherit working directory
            &startup,
            &mut process,
        )
    };
    if created == 0 {
        return Err(BgError::Io(std::io::Error::last_os_error()));
    }
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    Ok(process.dwProcessId)
}

/// Unix: a normal detached spawn. File descriptors other than stdio are
/// close-on-exec in Rust, and stdio is explicitly null, so nothing leaks.
#[cfg(not(windows))]
fn spawn_daemon(exe: &Path, root: &Path, log_file: &Path) -> Result<u32, BgError> {
    let child = std::process::Command::new(exe)
        .arg("run")
        .arg("--worktree")
        .arg(root)
        .arg("--log-file")
        .arg(log_file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(child.id())
}

fn stop(root: &Path) -> Result<(), BgError> {
    match try_call(root, worktree_ipc::Command::DaemonShutdown) {
        Ok(_) => {
            println!("daemon stopping");
            Ok(())
        }
        Err(worktree_ipc::IpcError::DaemonUnavailable) => {
            println!("no daemon running for {}", root.display());
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn status(root: &Path) -> Result<(), BgError> {
    match try_call(root, worktree_ipc::Command::DaemonInfo) {
        Ok(info) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&info).unwrap_or_default()
            );
            Ok(())
        }
        Err(worktree_ipc::IpcError::DaemonUnavailable) => {
            println!("stopped");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn try_call(
    root: &Path,
    command: worktree_ipc::Command,
) -> Result<serde_json::Value, worktree_ipc::IpcError> {
    let endpoint = worktree_ipc::endpoint::endpoint_for(root);
    let mut client = worktree_ipc::IpcClient::connect(&endpoint)?;
    let request = worktree_ipc::Request::new(command, serde_json::Value::Null);
    let response = client.call(&request)?;
    Ok(response.data)
}
