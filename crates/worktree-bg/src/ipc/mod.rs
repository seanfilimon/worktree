//! IPC server (BgProcess.md §14): serves `wt` CLI requests.
//!
//! One task per connection; requests are dispatched by command name.
//! Mutating commands are serialized through the daemon's single-writer
//! lock; read-only commands run concurrently (§14.4).

mod handlers;

use crate::service::daemon::DaemonContext;
use std::sync::Arc;
use worktree_ipc::{transport::tokio_frame, Command, Request, Response};

/// Serve one CLI connection until it closes.
pub async fn handle_connection(
    ctx: Arc<DaemonContext>,
    stream: interprocess::local_socket::tokio::Stream,
) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    loop {
        let request: Request = match tokio_frame::read(&mut reader).await {
            Ok(request) => request,
            Err(worktree_ipc::IpcError::ConnectionClosed) => break,
            Err(e) => {
                tracing::debug!("IPC read failed: {e}");
                break;
            }
        };
        let response = dispatch(ctx.clone(), request).await;
        if let Err(e) = tokio_frame::write(&mut writer, &response).await {
            tracing::debug!("IPC write failed: {e}");
            break;
        }
    }
}

/// Route a request to its handler.
pub async fn dispatch(ctx: Arc<DaemonContext>, request: Request) -> Response {
    let Some(command) = Command::from_name(&request.command) else {
        return Response::error(
            request.id,
            &format!("unknown command '{}'", request.command),
        );
    };

    let result = if command.is_mutating() {
        let guard_ctx = ctx.clone();
        let _writer = guard_ctx.writer_lock.lock().await;
        execute(ctx, command, request.args).await
    } else {
        execute(ctx, command, request.args).await
    };

    match result {
        Ok(data) => Response::ok(request.id, data),
        Err(e) => Response::error(request.id, &e.to_string()),
    }
}

async fn execute(
    ctx: Arc<DaemonContext>,
    command: Command,
    args: serde_json::Value,
) -> Result<serde_json::Value, crate::error::BgError> {
    use crate::error::BgError;

    // Shutdown is handled inline — it must not run on the blocking pool.
    if command == Command::DaemonShutdown {
        ctx.request_shutdown();
        return Ok(serde_json::json!({ "stopping": true }));
    }
    if command == Command::DaemonInfo {
        return Ok(handlers::daemon_info(&ctx));
    }

    let root = ctx.root.clone();
    tokio::task::spawn_blocking(move || handlers::execute_blocking(&root, command, args))
        .await
        .map_err(|e| BgError::Engine(format!("handler panicked: {e}")))?
}
