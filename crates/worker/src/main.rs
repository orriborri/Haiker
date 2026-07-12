use haiker_platform::config::AppConfig;
use haiker_platform::database;
use haiker_platform::job_queue::JobQueue;
use haiker_platform::outbox::{Outbox, OutboxDispatcher};
use haiker_platform::telemetry::{self, TelemetryConfig};
use haiker_platform::worker_runtime::{WorkerConfig, WorkerRuntime};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry_config = TelemetryConfig::from_env();
    telemetry::init_telemetry(&telemetry_config);

    tracing::info!("Starting Haiker background worker");

    let app_config = AppConfig::from_env();
    let pool = database::connect(&app_config.database).await?;

    let job_queue = JobQueue::new(pool.clone());
    let outbox = Outbox::new(pool.clone());
    let outbox_dispatcher = OutboxDispatcher::new(outbox, 5);

    let cancellation_token = CancellationToken::new();

    // Spawn signal handler to cancel the token on Ctrl+C
    let signal_token = cancellation_token.clone();
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "Failed to listen for ctrl_c signal");
        }
        tracing::info!("Shutdown signal received");
        signal_token.cancel();
    });

    let worker_config = WorkerConfig::default();
    let runtime = WorkerRuntime::new(job_queue, worker_config, cancellation_token.clone());

    // Run the worker runtime and outbox dispatcher concurrently
    let dispatcher_token = cancellation_token.clone();
    let dispatcher_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = dispatcher_token.cancelled() => {
                    tracing::info!("Outbox dispatcher shutting down");
                    break;
                }
                _ = interval.tick() => {
                    match outbox_dispatcher.process_batch(100).await {
                        Ok(count) => {
                            if count > 0 {
                                tracing::info!(processed = count, "Outbox events dispatched");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Outbox dispatch batch failed");
                        }
                    }
                }
            }
        }
    });

    runtime.run().await;
    let _ = dispatcher_handle.await;

    tracing::info!("Worker shutting down cleanly");
    Ok(())
}
