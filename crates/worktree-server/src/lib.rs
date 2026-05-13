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

    let mut sessions = HashMap::new();
    let demo_user = worktree_protocol::core::id::AccountId::new();
    sessions.insert(
        "dev-secret".to_string(),
        crate::auth::session::Session::new(
            demo_user,
            "dev-secret",
            chrono::Utc::now() + chrono::Duration::days(365),
        ),
    );

    let mut enforcer = PermissionEnforcer::new();
    use worktree_protocol::iam::permission::Permission;
    use worktree_protocol::iam::scope::Scope;
    enforcer.grant(demo_user, Permission::TreeCreate, Scope::Global);
    enforcer.grant(demo_user, Permission::TreeRead, Scope::Global);
    enforcer.grant(demo_user, Permission::SnapshotCreate, Scope::Global);
    enforcer.grant(demo_user, Permission::StagedCreate, Scope::Global);
    enforcer.grant(demo_user, Permission::BranchCreate, Scope::Global);
    enforcer.grant(demo_user, Permission::BranchRead, Scope::Global);

    let state = Arc::new(AppState {
        enforcer: Arc::new(RwLock::new(enforcer)),
        sessions: Arc::new(RwLock::new(sessions)),
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
    PushDirty(String, Vec<std::path::PathBuf>),
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

    loop {
        let token = std::env::var("WT_SERVER_AUTH_TOKEN").unwrap_or_else(|_| {
            if let Ok(engine) = worktree_sdk::WorktreeEngine::open(&root) {
                let auth_file = engine.wt_dir().join("cache").join("auth_token");
                if let Ok(t) = std::fs::read_to_string(auth_file) {
                    return t.trim().to_string();
                }
            }
            "dev-secret".to_string()
        });

        let mut tenant = std::env::var("WT_TENANT").unwrap_or_else(|_| "default".to_string());

        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() == 3 {
            let s = parts[1].replace('-', "+").replace('_', "/");
            let mut out = Vec::new();
            let mut buf = 0u32;
            let mut bits = 0;
            for c in s.chars() {
                if c == '=' {
                    continue;
                }
                let val = if c.is_ascii_uppercase() {
                    c as u32 - 65
                } else if c.is_ascii_lowercase() {
                    c as u32 - 71
                } else if c.is_ascii_digit() {
                    c as u32 + 4
                } else if c == '+' {
                    62
                } else if c == '/' {
                    63
                } else {
                    continue;
                };
                buf = (buf << 6) | val;
                bits += 6;
                if bits >= 8 {
                    bits -= 8;
                    out.push((buf >> bits) as u8);
                }
            }
            let payload_str = String::from_utf8_lossy(&out);
            if let Some(idx) = payload_str.find(r#""tenant":""#) {
                let rem = &payload_str[idx + 10..];
                if let Some(end_idx) = rem.find('"') {
                    tenant = rem[..end_idx].to_string();
                }
            }
        }

        let current_ws_url = format!("{}?tenant={}", ws_url, tenant);

        let mut request = match current_ws_url.clone().into_client_request() {
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

        tracing::info!(
            "Connecting to WebSocket: {} with token length {}",
            current_ws_url,
            token.len()
        );

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
    use crate::engine::{
        auto_commit::AutoCommitEngine, event::classify_event, event::SemanticEvent,
    };
    use crate::watcher::{
        debounce::{DebouncedEvent, EventKind},
        fs::FileSystemWatcher,
    };
    use std::collections::HashSet;
    use std::time::{Duration, Instant};
    use worktree_protocol::core::id::SnapshotId;

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
                PushQueueEvent::PushDirty(snap_id, dirty_paths) => {
                    if let Err(e) = worktree_sdk::engine::sync::push_staged_dirty(
                        &push_engine,
                        &snap_id,
                        &dirty_paths,
                    ) {
                        tracing::warn!(
                            "bgprocess: staged dirty sync failed for {}: {}",
                            &snap_id[..8],
                            e
                        );
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

    let mut auto_commit_interval = Duration::from_secs(3600); // Default to 1 hour
    if let Ok(config_str) = worktree_sdk::engine::config::read_config(&engine) {
        if let Ok(config) = toml::from_str::<
            worktree_protocol::config::worktree_config::WorktreeConfig,
        >(&config_str)
        {
            auto_commit_interval = Duration::from_secs(config.sync.auto_commit_interval_secs);
        }
    }

    let mut session_start = Instant::now();
    let mut active_snapshot_id = SnapshotId::new().to_string();
    let mut accumulated_events: Vec<SemanticEvent> = Vec::new();
    let mut dirty_paths: HashSet<std::path::PathBuf> = HashSet::new();

    loop {
        // Use a timeout so we can check the auto-commit interval
        match watcher.receiver.recv_timeout(Duration::from_millis(500)) {
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
                    for evt in &semantic {
                        accumulated_events.push(evt.clone());
                        if let SemanticEvent::CodeChange { paths, .. } = evt {
                            for p in paths {
                                dirty_paths.insert(p.clone());
                            }
                        }
                    }

                    // Push the dirty paths to the active staged snapshot on the server
                    if !dirty_paths.is_empty() {
                        let _ = push_tx.send(PushQueueEvent::PushDirty(
                            active_snapshot_id.clone(),
                            dirty_paths.iter().cloned().collect(),
                        ));
                    }
                }
            }
            Ok(Err(e)) => tracing::warn!("watcher error: {e}"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Check if the hourly interval has passed
                if session_start.elapsed() >= auto_commit_interval {
                    if !accumulated_events.is_empty() {
                        if let Some(msg) = commit_engine.evaluate(&accumulated_events) {
                            match worktree_sdk::engine::snapshot::create_snapshot(
                                &engine, None, &msg,
                            ) {
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
                    // Reset the session
                    session_start = Instant::now();
                    active_snapshot_id = SnapshotId::new().to_string();
                    accumulated_events.clear();
                    dirty_paths.clear();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}
