//! Worker runtime for background job processing.
//!
//! Provides a polling-based worker that routes jobs to registered handlers,
//! supports graceful shutdown, and propagates correlation IDs through tracing.

use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, Instrument};

use crate::job_queue::{JobHandler, JobQueue};

/// Configuration for the worker runtime.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Interval in milliseconds between poll attempts.
    pub poll_interval_ms: u64,
    /// Maximum number of jobs to process concurrently.
    pub max_concurrent_jobs: usize,
    /// Seconds to wait for in-flight jobs during shutdown.
    pub shutdown_timeout_seconds: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 1000,
            max_concurrent_jobs: 5,
            shutdown_timeout_seconds: 30,
        }
    }
}

/// Worker runtime that polls the job queue and dispatches to handlers.
pub struct WorkerRuntime {
    queue: JobQueue,
    handlers: Vec<Box<dyn JobHandler>>,
    config: WorkerConfig,
    cancellation_token: CancellationToken,
}

impl WorkerRuntime {
    /// Create a new worker runtime.
    pub fn new(
        queue: JobQueue,
        config: WorkerConfig,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            queue,
            handlers: Vec::new(),
            config,
            cancellation_token,
        }
    }

    /// Register a job handler.
    pub fn register_handler(&mut self, handler: Box<dyn JobHandler>) {
        self.handlers.push(handler);
    }

    /// Run the worker polling loop until cancellation is signalled.
    ///
    /// Polls the job queue at the configured interval, dispatches jobs to
    /// matching handlers (up to max_concurrent_jobs), and waits for in-flight
    /// jobs on shutdown.
    pub async fn run(self) {
        let handlers = Arc::new(self.handlers);
        let queue = self.queue.clone();
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_jobs));
        let poll_interval = Duration::from_millis(self.config.poll_interval_ms);
        let shutdown_timeout = Duration::from_secs(self.config.shutdown_timeout_seconds);
        let token = self.cancellation_token.clone();

        info!(
            poll_interval_ms = self.config.poll_interval_ms,
            max_concurrent_jobs = self.config.max_concurrent_jobs,
            "Worker runtime starting"
        );

        let poll_token = token.clone();
        let poll_semaphore = semaphore.clone();

        let poll_task = tokio::spawn(async move {
            let mut interval = time::interval(poll_interval);

            loop {
                tokio::select! {
                    _ = poll_token.cancelled() => {
                        info!("Worker polling loop cancelled");
                        break;
                    }
                    _ = interval.tick() => {
                        // Try to acquire a permit before polling
                        let permit = match poll_semaphore.clone().try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => {
                                // All slots occupied, skip this tick
                                continue;
                            }
                        };

                        let job = match queue.poll().await {
                            Ok(Some(job)) => job,
                            Ok(None) => {
                                drop(permit);
                                continue;
                            }
                            Err(e) => {
                                warn!(error = %e, "Failed to poll job queue");
                                drop(permit);
                                continue;
                            }
                        };

                        let job_id = job.id;
                        let job_type = job.job_type.clone();
                        let correlation_id = job.correlation_id;
                        let handler_queue = queue.clone();
                        let handlers_ref = handlers.clone();

                        tokio::spawn(async move {
                            let _permit = permit;

                            let span = if let Some(cid) = correlation_id {
                                tracing::info_span!(
                                    "job",
                                    job_id = %job_id,
                                    job_type = %job_type,
                                    correlation_id = %cid,
                                )
                            } else {
                                tracing::info_span!(
                                    "job",
                                    job_id = %job_id,
                                    job_type = %job_type,
                                )
                            };

                            async {
                                let handler = handlers_ref
                                    .iter()
                                    .find(|h| h.job_type() == job_type);

                                match handler {
                                    Some(h) => match h.handle(&job).await {
                                        Ok(()) => {
                                            if let Err(e) = handler_queue.complete(job_id).await {
                                                warn!(error = %e, "Failed to mark job as completed");
                                            }
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "Job handler failed");
                                            if let Err(e) = handler_queue.fail(job_id, &e.to_string()).await {
                                                warn!(error = %e, "Failed to mark job as failed");
                                            }
                                        }
                                    },
                                    None => {
                                        warn!("No handler registered for job type: {}", job_type);
                                        if let Err(e) = handler_queue.fail(
                                            job_id,
                                            &format!("no handler registered for job type: {}", job_type),
                                        ).await {
                                            warn!(error = %e, "Failed to mark job as failed");
                                        }
                                    }
                                }
                            }
                            .instrument(span)
                            .await;
                        });
                    }
                }
            }
        });

        // Wait for the poll task to finish (it exits on cancellation)
        let _ = poll_task.await;

        // Wait for all in-flight jobs to complete (semaphore fully released)
        info!("Waiting for in-flight jobs to complete");
        let drain_result = tokio::time::timeout(shutdown_timeout, async {
            // Acquire all permits to ensure no jobs are in flight
            let _permits = semaphore
                .acquire_many(self.config.max_concurrent_jobs as u32)
                .await;
        })
        .await;

        if drain_result.is_err() {
            warn!("Shutdown timeout exceeded; some jobs may not have completed");
        }

        info!("Worker runtime stopped");
    }
}
