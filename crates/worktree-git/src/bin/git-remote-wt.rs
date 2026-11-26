//! `git-remote-wt`: lets stock Git clone, fetch and push
//! `wt::https://<host>/<tenant>/<tree>`. Git starts this program itself;
//! see `worktree_git::remote_helper`.

use std::process::ExitCode;

/// Diagnostics go to stderr and are off unless `WT_LOG` (an `EnvFilter`
/// directive such as `debug` or `worktree_sdk=trace`) is set, as in `wt`.
fn init_tracing() {
    if let Ok(filter) = tracing_subscriber::EnvFilter::try_from_env("WT_LOG") {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }
}

fn main() -> ExitCode {
    init_tracing();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match worktree_git::remote_helper::run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}: {e}", worktree_git::remote_helper::BINARY_NAME);
            ExitCode::FAILURE
        }
    }
}
