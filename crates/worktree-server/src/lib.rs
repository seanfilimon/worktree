pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod storage;

pub async fn run() -> Result<(), error::ServerError> {
    tracing::info!("Server initialized");
    Ok(())
}
