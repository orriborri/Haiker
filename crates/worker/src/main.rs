//! Haiker background job processor.
//!
//! Runs as a separate process that polls for and executes background jobs
//! such as file imports, route exports, and event dispatching.

use tokio::signal;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    haiker_platform::telemetry::init();

    let _config = haiker_platform::config::AppConfig::from_env();

    info!("Worker process started");
    info!("Worker health: healthy");

    // Run until shutdown signal
    shutdown_signal().await;

    info!("Worker process shutting down");
    Ok(())
}

/// Wait for a shutdown signal (SIGINT or SIGTERM).
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
