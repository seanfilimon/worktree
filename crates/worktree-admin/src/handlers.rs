//! HTTP handlers for the Worktree admin panel API

use crate::{
    error::{AdminError, Result},
    state::AdminState,
    AdminResponse, RepositoryInfo, ServerStats, ServerStatus,
};
use axum::{
    extract::{Path, State},
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use uuid::Uuid;

/// Authentication middleware
pub async fn auth_middleware<B>(
    State(state): State<AdminState>,
    request: Request<B>,
    next: Next<B>,
) -> Result<Response> {
    // Skip auth if not enabled
    if !state.auth_enabled() {
        return Ok(next.run(request).await);
    }

    // Extract API key from Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| AdminError::Authentication("Missing Authorization header".to_string()))?;

    // Expect format: "Bearer <api_key>"
    let api_key = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AdminError::Authentication("Invalid Authorization format".to_string()))?;

    // Validate the API key
    state.validate_api_key(api_key)?;

    Ok(next.run(request).await)
}

/// Health check endpoint
pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now(),
        })),
    )
}

/// Get server status
pub async fn server_status(State(state): State<AdminState>) -> Result<Json<ServerStatus>> {
    state.increment_requests().await;

    let connection = state.server_connection.read().await;

    let status = ServerStatus {
        id: connection.server_id.unwrap_or_else(Uuid::new_v4),
        name: "worktree-server".to_string(),
        running: connection.connected,
        uptime_seconds: connection.uptime_seconds().unwrap_or(0),
        active_connections: 0, // TODO: Get from actual server
        tracked_repositories: 0, // TODO: Get from actual server
        last_updated: chrono::Utc::now(),
    };

    Ok(Json(status))
}

/// Get admin panel metrics
pub async fn metrics(State(state): State<AdminState>) -> Result<Json<serde_json::Value>> {
    state.increment_requests().await;

    let metrics = state.get_metrics().await;
    let connection = state.server_connection.read().await;

    Ok(Json(json!({
        "admin_panel": {
            "uptime_seconds": metrics.uptime_seconds(),
            "total_requests": metrics.total_requests,
            "total_errors": metrics.total_errors,
            "error_rate": metrics.error_rate(),
        },
        "server_connection": {
            "connected": connection.connected,
            "connection_attempts": connection.connection_attempts,
            "failed_connections": connection.failed_connections,
            "last_connected": connection.last_connected,
        }
    })))
}

/// Start the worktree server
pub async fn start_server(State(state): State<AdminState>) -> Result<Json<AdminResponse>> {
    state.increment_requests().await;

    // TODO: Implement actual server start logic
    tracing::info!("Starting worktree server at {}", state.server_endpoint());

    let mut connection = state.server_connection.write().await;
    connection.mark_connected(Uuid::new_v4());

    Ok(Json(AdminResponse::success("Server started successfully")))
}

/// Stop the worktree server
pub async fn stop_server(State(state): State<AdminState>) -> Result<Json<AdminResponse>> {
    state.increment_requests().await;

    // TODO: Implement actual server stop logic
    tracing::info!("Stopping worktree server");

    let mut connection = state.server_connection.write().await;
    connection.mark_disconnected();

    Ok(Json(AdminResponse::success("Server stopped successfully")))
}

/// Restart the worktree server
pub async fn restart_server(State(state): State<AdminState>) -> Result<Json<AdminResponse>> {
    state.increment_requests().await;

    // TODO: Implement actual server restart logic
    tracing::info!("Restarting worktree server");

    let mut connection = state.server_connection.write().await;
    connection.mark_disconnected();
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    connection.mark_connected(Uuid::new_v4());

    Ok(Json(AdminResponse::success("Server restarted successfully")))
}

/// List all repositories
pub async fn list_repositories(
    State(state): State<AdminState>,
) -> Result<Json<Vec<RepositoryInfo>>> {
    state.increment_requests().await;

    // TODO: Implement actual repository listing from server
    tracing::debug!("Listing repositories");

    // Return mock data for now
    let repos = vec![
        RepositoryInfo {
            id: Uuid::new_v4(),
            path: "/example/repo1".to_string(),
            branch_count: 3,
            commit_count: 42,
            last_activity: chrono::Utc::now(),
            size_bytes: 1024 * 1024 * 10, // 10 MB
        },
        RepositoryInfo {
            id: Uuid::new_v4(),
            path: "/example/repo2".to_string(),
            branch_count: 5,
            commit_count: 128,
            last_activity: chrono::Utc::now(),
            size_bytes: 1024 * 1024 * 25, // 25 MB
        },
    ];

    Ok(Json(repos))
}

/// Get repository by ID
pub async fn get_repository(
    State(state): State<AdminState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RepositoryInfo>> {
    state.increment_requests().await;

    // TODO: Implement actual repository fetching from server
    tracing::debug!("Getting repository {}", id);

    // Return mock data for now
    let repo = RepositoryInfo {
        id,
        path: format!("/example/repo-{}", id),
        branch_count: 3,
        commit_count: 42,
        last_activity: chrono::Utc::now(),
        size_bytes: 1024 * 1024 * 10,
    };

    Ok(Json(repo))
}

/// Get server statistics
pub async fn get_stats(State(state): State<AdminState>) -> Result<Json<ServerStats>> {
    state.increment_requests().await;

    // TODO: Implement actual stats collection from server
    tracing::debug!("Getting server statistics");

    let stats = ServerStats {
        total_repositories: 2,
        total_commits: 170,
        total_branches: 8,
        total_storage_bytes: 1024 * 1024 * 35, // 35 MB
        total_operations: 1234,
        collected_at: chrono::Utc::now(),
    };

    Ok(Json(stats))
}

/// Run garbage collection
pub async fn run_garbage_collection(
    State(state): State<AdminState>,
) -> Result<Json<AdminResponse>> {
    state.increment_requests().await;

    // TODO: Implement actual garbage collection
    tracing::info!("Running garbage collection");

    Ok(Json(AdminResponse::success_with_data(
        "Garbage collection completed",
        json!({
            "reclaimed_bytes": 1024 * 1024 * 5,
            "duration_ms": 1250,
        }),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AdminConfig;

    #[tokio::test]
    async fn test_health() {
        let response = health().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_server_status() {
        let config = AdminConfig::default();
        let state = AdminState::new(config);

        let result = server_status(State(state)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_metrics() {
        let config = AdminConfig::default();
        let state = AdminState::new(config);

        let result = metrics(State(state)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_repositories() {
        let config = AdminConfig::default();
        let state = AdminState::new(config);

        let result = list_repositories(State(state)).await;
        assert!(result.is_ok());
        let repos = result.unwrap().0;
        assert_eq!(repos.len(), 2);
    }

    #[tokio::test]
    async fn test_get_repository() {
        let config = AdminConfig::default();
        let state = AdminState::new(config);
        let test_id = Uuid::new_v4();

        let result = get_repository(State(state), Path(test_id)).await;
        assert!(result.is_ok());
        let repo = result.unwrap().0;
        assert_eq!(repo.id, test_id);
    }

    #[tokio::test]
    async fn test_start_server() {
        let config = AdminConfig::default();
        let state = AdminState::new(config);

        let result = start_server(State(state.clone())).await;
        assert!(result.is_ok());

        let connection = state.server_connection.read().await;
        assert!(connection.connected);
    }

    #[tokio::test]
    async fn test_stop_server() {
        let config = AdminConfig::default();
        let state = AdminState::new(config);

        // First start the server
        {
            let mut connection = state.server_connection.write().await;
            connection.mark_connected(Uuid::new_v4());
        }

        let result = stop_server(State(state.clone())).await;
        assert!(result.is_ok());

        let connection = state.server_connection.read().await;
        assert!(!connection.connected);
    }
}
