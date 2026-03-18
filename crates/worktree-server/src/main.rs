use tracing_subscriber;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("Worktree Server v{} starting...", env!("CARGO_PKG_VERSION"));
    if let Err(e) = worktree_server::run().await {
        tracing::error!("Server error: {}", e);
        std::process::exit(1);
    }
}
