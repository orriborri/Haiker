use haiker_infrastructure::audit::AuditLog;
use haiker_infrastructure::config::AppConfig;
use haiker_infrastructure::database;
use haiker_infrastructure::export_worker::GenerateExportJobHandler;
use haiker_infrastructure::import_cleanup::cleanup_abandoned_imports;
use haiker_infrastructure::import_persistence::PgImportRepository;
use haiker_infrastructure::import_worker::ParseGpxJobHandler;
use haiker_infrastructure::job_queue::JobQueue;
use haiker_infrastructure::object_storage::ObjectStorageClient;
use haiker_infrastructure::outbox::{Outbox, OutboxDispatcher};
use haiker_infrastructure::session::SessionStore;
use haiker_infrastructure::telemetry::{self, TelemetryConfig};
use haiker_infrastructure::worker_runtime::{WorkerConfig, WorkerRuntime};
use tokio_util::sync::CancellationToken;

/// Interval for periodic maintenance tasks (stale job timeout, session cleanup).
const MAINTENANCE_INTERVAL_SECS: u64 = 60;

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
    let session_store = SessionStore::new(pool.clone());

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
    let mut runtime =
        WorkerRuntime::new(job_queue.clone(), worker_config, cancellation_token.clone());

    // Register job handlers
    let object_storage = ObjectStorageClient::new(&app_config.storage).await?;
    let audit_log = AuditLog::new(pool.clone());
    let parse_gpx_handler =
        ParseGpxJobHandler::new(pool.clone(), object_storage.clone(), audit_log);
    runtime.register_handler(Box::new(parse_gpx_handler));

    let export_handler = GenerateExportJobHandler::new(pool.clone(), object_storage.clone());
    runtime.register_handler(Box::new(export_handler));

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

    // Spawn periodic maintenance: stale job timeout, session cleanup, abandoned imports
    let maintenance_token = cancellation_token.clone();
    let maintenance_queue = job_queue.clone();
    let import_repository = PgImportRepository::new(pool.clone());
    let maintenance_handle = tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(MAINTENANCE_INTERVAL_SECS));
        loop {
            tokio::select! {
                _ = maintenance_token.cancelled() => {
                    tracing::info!("Maintenance task shutting down");
                    break;
                }
                _ = interval.tick() => {
                    // Timeout stale jobs that exceeded their deadline
                    match maintenance_queue.timeout_stale_jobs().await {
                        Ok(count) => {
                            if count > 0 {
                                tracing::info!(count, "Timed out stale jobs");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to timeout stale jobs");
                        }
                    }

                    // Cleanup expired sessions
                    match session_store.cleanup_expired().await {
                        Ok(count) => {
                            if count > 0 {
                                tracing::info!(count, "Cleaned up expired sessions");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to cleanup expired sessions");
                        }
                    }

                    // Cleanup abandoned imports stuck in processing states
                    match cleanup_abandoned_imports(&import_repository).await {
                        Ok(count) => {
                            if count > 0 {
                                tracing::info!(count, "Cleaned up abandoned imports");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to cleanup abandoned imports");
                        }
                    }
                }
            }
        }
    });

    runtime.run().await;
    let _ = dispatcher_handle.await;
    let _ = maintenance_handle.await;

    tracing::info!("Worker shutting down cleanly");
    Ok(())
}
