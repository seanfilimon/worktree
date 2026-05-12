use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::str::FromStr;
use std::sync::Arc;
use worktree_protocol::core::id::{TenantId, TreeId};
use worktree_protocol::iam::permission::Permission;
use worktree_protocol::iam::scope::Scope;

use crate::AppState;

pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let token = {
        let auth_header = req.headers().get(header::AUTHORIZATION);
        auth_header
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .unwrap_or_default()
            .to_string()
    };

    if token.is_empty() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let user_id = {
        let sessions = state.sessions.read().unwrap();
        let session = match sessions.get(&token) {
            Some(s) => s.clone(),
            None => return StatusCode::UNAUTHORIZED.into_response(),
        };

        if session.is_expired() {
            return StatusCode::UNAUTHORIZED.into_response();
        }

        session.user_id
    };

    let path = req.uri().path().to_string();

    let tree_id = req
        .headers()
        .get("x-wt-tree-id")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| TreeId::from_str(s).ok());

    let (permission, scope) = match path.as_str() {
        "/init" => (Permission::TreeCreate, Scope::Global),
        "/status" => {
            if let Some(t) = tree_id {
                (Permission::TreeRead, Scope::Tree(TenantId::nil(), t))
            } else {
                return StatusCode::BAD_REQUEST.into_response();
            }
        }
        "/snapshot" => {
            if let Some(t) = tree_id {
                (Permission::SnapshotCreate, Scope::Tree(TenantId::nil(), t))
            } else {
                return StatusCode::BAD_REQUEST.into_response();
            }
        }
        "/staged" => {
            if let Some(t) = tree_id {
                (Permission::StagedCreate, Scope::Tree(TenantId::nil(), t))
            } else {
                return StatusCode::BAD_REQUEST.into_response();
            }
        }
        "/branch" => {
            if let Some(t) = tree_id {
                (Permission::BranchCreate, Scope::Tree(TenantId::nil(), t))
            } else {
                return StatusCode::BAD_REQUEST.into_response();
            }
        }
        "/health" => {
            // Health check bypasses auth
            return next.run(req).await;
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    let allowed = {
        let enforcer = state.enforcer.read().unwrap();
        enforcer.check(&user_id, &permission, &scope)
    };

    if !allowed {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Provide user id to next handler if needed
    let mut req = req;
    req.extensions_mut().insert(user_id);

    next.run(req).await
}
