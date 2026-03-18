pub mod config;
pub mod service;
pub mod watcher;
pub mod engine;
pub mod sync;
pub mod git;
pub mod storage;
pub mod auth;
pub mod api;
pub mod error;

pub async fn run() -> Result<(), error::ServerError> {
    tracing::info!("Server initialized");
    Ok(())
}
