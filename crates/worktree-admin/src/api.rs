//! API route definitions for the Worktree admin panel

use crate::{handlers, state::AdminState};
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

/// Build the main API router
pub fn router(state: AdminState) -> Router {
    let api_router = Router::new()
        // Health and status endpoints
        .route("/health", get(handlers::health))
        .route("/status", get(handlers::server_status))
        .route("/metrics", get(handlers::metrics))
        // Server control endpoints
        .route("/server/start", post(handlers::start_server))
        .route("/server/stop", post(handlers::stop_server))
        .route("/server/restart", post(handlers::restart_server))
        // Repository endpoints
        .route("/repositories", get(handlers::list_repositories))
        .route("/repositories/:id", get(handlers::get_repository))
        // Statistics endpoints
        .route("/stats", get(handlers::get_stats))
        // Maintenance endpoints
        .route("/maintenance/gc", post(handlers::run_garbage_collection))
        // Add authentication middleware if enabled
        .layer(middleware::from_fn_with_state(
            state.clone(),
            handlers::auth_middleware,
        ));

    Router::new()
        .nest("/api", api_router)
        // Add CORS layer
        .layer(CorsLayer::permissive())
        // Add tracing layer
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AdminConfig;

    #[test]
    fn test_router_creation() {
        let config = AdminConfig::default();
        let state = AdminState::new(config);
        let _router = router(state);
        // If we get here without panicking, the router was created successfully
    }
}
