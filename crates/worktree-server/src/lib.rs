pub mod api;
pub mod auth;
pub mod config;
pub mod engine;
pub mod error;
pub mod git;
pub mod service;
pub mod storage;
pub mod sync;
pub mod watcher;

use crate::api::handlers::{
    handle_branch, handle_init, handle_snapshot, handle_staged, handle_status, BranchRequest,
    InitRequest, SnapshotRequest, StagedRequest, StatusRequest,
};
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};

pub async fn run() -> Result<(), error::ServerError> {
    let root = std::env::current_dir().map_err(error::ServerError::Io)?;
    tokio::task::spawn_blocking(move || {
        if let Err(e) = watcher_loop_blocking(root) {
            tracing::warn!("watcher task exited: {e}");
        }
    });

    let app = Router::new()
        .route("/health", get(route_health))
        .route("/init", post(route_init))
        .route("/status", post(route_status))
        .route("/snapshot", post(route_snapshot))
        .route("/staged", post(route_staged))
        .route("/branch", post(route_branch));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:9876")
        .await
        .map_err(error::ServerError::Io)?;
    tracing::info!("listening on http://127.0.0.1:9876");
    axum::serve(listener, app)
        .await
        .map_err(error::ServerError::Io)?;
    Ok(())
}

// ── Axum route handlers ───────────────────────────────────────────────────────

async fn route_health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

async fn route_init(Json(req): Json<InitRequest>) -> impl IntoResponse {
    match handle_init(req).await {
        Ok(r) => (StatusCode::OK, Json(serde_json::to_value(r).unwrap())).into_response(),
        Err(e) => server_err(e),
    }
}

async fn route_status(Json(req): Json<StatusRequest>) -> impl IntoResponse {
    match handle_status(req).await {
        Ok(r) => (StatusCode::OK, Json(serde_json::to_value(r).unwrap())).into_response(),
        Err(e) => server_err(e),
    }
}

async fn route_snapshot(Json(req): Json<SnapshotRequest>) -> impl IntoResponse {
    match handle_snapshot(req).await {
        Ok(r) => (StatusCode::OK, Json(serde_json::to_value(r).unwrap())).into_response(),
        Err(e) => server_err(e),
    }
}

async fn route_branch(Json(req): Json<BranchRequest>) -> impl IntoResponse {
    match handle_branch(req).await {
        Ok(r) => (StatusCode::OK, Json(serde_json::to_value(r).unwrap())).into_response(),
        Err(e) => server_err(e),
    }
}

async fn route_staged(Json(req): Json<StagedRequest>) -> impl IntoResponse {
    match handle_staged(req).await {
        Ok(r) => (StatusCode::OK, Json(serde_json::to_value(r).unwrap())).into_response(),
        Err(e) => server_err(e),
    }
}

fn server_err(e: error::ServerError) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
    )
        .into_response()
}

// ── Watcher loop (blocking thread) ───────────────────────────────────────────

fn watcher_loop_blocking(root: std::path::PathBuf) -> Result<(), error::ServerError> {
    use crate::engine::{auto_commit::AutoCommitEngine, event::classify_event};
    use crate::watcher::{
        debounce::{DebouncedEvent, EventKind},
        fs::FileSystemWatcher,
    };

    let engine = worktree_sdk::WorktreeEngine::open(&root)
        .map_err(|e| error::ServerError::Engine(e.to_string()))?;

    let mut watcher = FileSystemWatcher::new()?;
    watcher.watch(&root)?;

    let mut debouncer = crate::watcher::debounce::Debouncer::new(500);
    let commit_engine = AutoCommitEngine::new();

    loop {
        match watcher.receiver.recv() {
            Ok(Ok(raw)) => {
                let kind = match raw.kind {
                    notify::EventKind::Create(_) => EventKind::Created,
                    notify::EventKind::Modify(_) => EventKind::Modified,
                    notify::EventKind::Remove(_) => EventKind::Deleted,
                    _ => EventKind::Modified,
                };
                for path in raw.paths {
                    debouncer.push(DebouncedEvent::now(path, kind));
                }
                let ready = debouncer.flush();
                if !ready.is_empty() {
                    let semantic: Vec<_> = ready.iter().map(classify_event).collect();
                    if let Some(msg) = commit_engine.evaluate(&semantic) {
                        match worktree_sdk::engine::snapshot::create_snapshot(&engine, None, &msg) {
                            Ok(snap) => {
                                tracing::info!(
                                    "bgprocess: auto-snapshot {} - {msg}",
                                    &snap.id[..8]
                                );
                                if let Err(e) =
                                    worktree_sdk::engine::sync::push_staged(&engine, &snap.id)
                                {
                                    tracing::warn!(
                                        "bgprocess: staged sync failed after auto-snapshot {}: {e}",
                                        &snap.id[..8]
                                    );
                                }
                            }
                            Err(worktree_sdk::SdkError::NoChanges) => {}
                            Err(e) => tracing::warn!("bgprocess: snapshot failed: {e}"),
                        }
                    }
                }
            }
            Ok(Err(e)) => tracing::warn!("watcher error: {e}"),
            Err(_) => break,
        }
    }
    Ok(())
}
