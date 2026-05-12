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
    handle_branch, handle_init, handle_snapshot, handle_staged, handle_status, route_staged_ws,
    BranchRequest, InitRequest, SnapshotRequest, StagedRequest, StatusRequest,
};
use crate::auth::enforcer::PermissionEnforcer;
use crate::auth::session::Session;
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct AppState {
    pub enforcer: Arc<RwLock<PermissionEnforcer>>,
    pub sessions: Arc<RwLock<HashMap<String, Session>>>,
    pub staged_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
}

pub async fn run() -> Result<(), error::ServerError> {
    let root = std::env::current_dir().map_err(error::ServerError::Io)?;
    let root_clone = root.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = watcher_loop_blocking(root_clone) {
            tracing::warn!("watcher task exited: {e}");
        }
    });

    let ws_root = root.clone();
    tokio::spawn(async move {
        ws_staged_loop(ws_root).await;
    });

    let (staged_tx, _) = tokio::sync::broadcast::channel(100);

    let state = Arc::new(AppState {
        enforcer: Arc::new(RwLock::new(PermissionEnforcer::new())),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        staged_tx,
    });

    let app = Router::new()
        .route("/health", get(route_health))
        .route("/init", post(route_init))
        .route("/status", post(route_status))
        .route("/snapshot", post(route_snapshot))
        .route("/staged", post(route_staged))
        .route("/branch", post(route_branch))
        .route("/staged/ws", get(route_staged_ws))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ))
        .with_state(state);

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

async fn route_staged(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(req): Json<StagedRequest>,
) -> impl IntoResponse {
    let snapshot = req.snapshot.clone();
    match handle_staged(req).await {
        Ok(r) => {
            if let Ok(val) = serde_json::to_value(&snapshot) {
                let _ = state.staged_tx.send(val);
            }
            (StatusCode::OK, Json(serde_json::to_value(r).unwrap())).into_response()
        }
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

enum PushQueueEvent {
    Push(String),
    Resume,
}

async fn ws_staged_loop(root: std::path::PathBuf) {
    use futures_util::StreamExt;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let base_url =
        std::env::var("WT_SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let ws_url = if base_url.starts_with("https") {
        base_url.replacen("https", "wss", 1) + "/staged/ws"
    } else {
        base_url.replacen("http", "ws", 1) + "/staged/ws"
    };

    let token = std::env::var("WT_SERVER_AUTH_TOKEN").unwrap_or_else(|_| {
        if let Ok(engine) = worktree_sdk::WorktreeEngine::open(&root) {
            let auth_file = engine.wt_dir().join("cache").join("auth_token");
            if let Ok(t) = std::fs::read_to_string(auth_file) {
                return t.trim().to_string();
            }
        }
        "dev-secret".to_string()
    });

    loop {
        let mut request = match ws_url.clone().into_client_request() {
            Ok(req) => req,
            Err(e) => {
                tracing::error!("Invalid WebSocket URL: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );

        tracing::info!("Connecting to WebSocket: {}", ws_url);

        let ws_stream = match connect_async(request).await {
            Ok((stream, _)) => stream,
            Err(e) => {
                tracing::warn!("WebSocket connection failed: {}. Retrying...", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        tracing::info!("WebSocket connected, listening for staged snapshots.");
        let (_, mut read) = ws_stream.split();

        while let Some(msg) = read.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    tracing::debug!("WS message: {}", text);
                    if let Ok(engine) = worktree_sdk::WorktreeEngine::open(&root) {
                        let staged_file = engine.wt_dir().join("cache").join("staged_index.json");

                        if let Ok(incoming) = serde_json::from_str::<serde_json::Value>(&text) {
                            if incoming.get("snapshot_id").is_some() || incoming.get("id").is_some()
                            {
                                let mut snapshots = Vec::new();
                                if let Ok(content) = std::fs::read_to_string(&staged_file) {
                                    if let Ok(existing) =
                                        serde_json::from_str::<Vec<serde_json::Value>>(&content)
                                    {
                                        snapshots = existing;
                                    }
                                }
                                snapshots.push(incoming);

                                if let Ok(out) = serde_json::to_string_pretty(&snapshots) {
                                    let _ = std::fs::create_dir_all(staged_file.parent().unwrap());
                                    let _ = std::fs::write(&staged_file, out);
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("WebSocket error: {}", e);
                    break;
                }
            }
        }

        tracing::warn!("WebSocket disconnected. Reconnecting in 5s...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

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

    let (push_tx, push_rx) = std::sync::mpsc::channel::<PushQueueEvent>();
    let push_root = root.clone();
    std::thread::spawn(move || {
        let push_engine = match worktree_sdk::WorktreeEngine::open(&push_root) {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("push thread failed to open engine: {e}");
                return;
            }
        };
        for event in push_rx {
            match event {
                PushQueueEvent::Push(snap_id) => {
                    let mut attempts = 0;
                    loop {
                        match worktree_sdk::engine::sync::push_staged(&push_engine, &snap_id) {
                            Ok(_) => break,
                            Err(e) => {
                                attempts += 1;
                                tracing::warn!(
                                    "bgprocess: staged sync failed for snapshot {}: {} (attempt {})",
                                    &snap_id[..8], e, attempts
                                );
                                std::thread::sleep(std::time::Duration::from_secs(2));
                            }
                        }
                    }
                }
                PushQueueEvent::Resume => {
                    tracing::info!("bgprocess: sync resumed, backfilling...");
                    if let Err(e) = worktree_sdk::engine::sync::push_unpushed(&push_engine) {
                        tracing::warn!("bgprocess: backfill failed: {e}");
                    }
                }
            }
        }
    });

    loop {
        match watcher.receiver.recv() {
            Ok(Ok(raw)) => {
                let mut sync_resumed = false;
                for path in raw.paths {
                    let kind = match &raw.kind {
                        notify::EventKind::Create(_) => EventKind::Created,
                        notify::EventKind::Modify(_) => EventKind::Modified,
                        notify::EventKind::Remove(_) => EventKind::Deleted,
                        _ => EventKind::Modified,
                    };
                    if path.ends_with(
                        std::path::Path::new(".wt")
                            .join("cache")
                            .join("sync_paused"),
                    ) && matches!(kind, EventKind::Deleted)
                    {
                        sync_resumed = true;
                    }
                    debouncer.push(DebouncedEvent::now(path, kind));
                }
                if sync_resumed {
                    let _ = push_tx.send(PushQueueEvent::Resume);
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
                                let _ = push_tx.send(PushQueueEvent::Push(snap.id.clone()));
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
