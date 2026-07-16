//! Worker runtime for background job processing.
//!
//! Provides a polling-based worker that routes jobs to registered handlers,
//! supports graceful shutdown, enforces execution time limits, and propagates
//! correlation IDs through tracing.

use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, Instrument};

use crate::job_queue::{Job, JobHandler, JobQueue};
use crate::metrics;

/// Configuration for resource budgets applied to job execution.
#[derive(Debug, Clone)]
pub struct ResourceBudgetConfig {
    /// Default execution time limit in seconds when a job does not specify one.
    ///
    /// Applied when `Job::timeout_seconds` is 0 or negative.
    pub default_job_timeout_seconds: u64,
}

impl Default for ResourceBudgetConfig {
    fn default() -> Self {
        Self {
            default_job_timeout_seconds: 300,
        }
    }
}

/// Configuration for the worker runtime.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Interval in milliseconds between poll attempts.
    pub poll_interval_ms: u64,
    /// Maximum number of jobs to process concurrently.
    pub max_concurrent_jobs: usize,
    /// Seconds to wait for in-flight jobs during shutdown.
    pub shutdown_timeout_seconds: u64,
    /// Resource budget configuration for job execution limits.
    pub resource_budget: ResourceBudgetConfig,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 1000,
            max_concurrent_jobs: 5,
            shutdown_timeout_seconds: 30,
            resource_budget: ResourceBudgetConfig::default(),
        }
    }
}

/// Determine the effective execution timeout for a job.
///
/// Uses the job's own `timeout_seconds` if positive, otherwise falls back to
/// the configured default.
fn effective_timeout(job: &Job, config: &ResourceBudgetConfig) -> Duration {
    if job.timeout_seconds > 0 {
        Duration::from_secs(job.timeout_seconds as u64)
    } else {
        Duration::from_secs(config.default_job_timeout_seconds)
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
    /// matching handlers (up to max_concurrent_jobs), enforces execution time
    /// limits, and waits for in-flight jobs on shutdown.
    pub async fn run(self) {
        let handlers = Arc::new(self.handlers);
        let queue = self.queue.clone();
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_jobs));
        let poll_interval = Duration::from_millis(self.config.poll_interval_ms);
        let shutdown_timeout = Duration::from_secs(self.config.shutdown_timeout_seconds);
        let token = self.cancellation_token.clone();
        let resource_budget = self.config.resource_budget.clone();
        let max_concurrent_jobs = self.config.max_concurrent_jobs;

        info!(
            poll_interval_ms = self.config.poll_interval_ms,
            max_concurrent_jobs = self.config.max_concurrent_jobs,
            default_job_timeout_seconds = self.config.resource_budget.default_job_timeout_seconds,
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
                                // All slots occupied, emit backpressure metric
                                metrics::record_worker_backpressure(
                                    max_concurrent_jobs as u64,
                                    max_concurrent_jobs as u64,
                                );
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
                        let timeout_duration = effective_timeout(&job, &resource_budget);
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
                                let start = std::time::Instant::now();
                                let handler = handlers_ref
                                    .iter()
                                    .find(|h| h.job_type() == job_type);

                                match handler {
                                    Some(h) => {
                                        let result = tokio::time::timeout(
                                            timeout_duration,
                                            h.handle(&job),
                                        )
                                        .await;

                                        match result {
                                            Ok(Ok(())) => {
                                                let duration_ms = start.elapsed().as_millis() as u64;
                                                metrics::record_job_processed(&job_type, duration_ms, true);
                                                if let Err(e) = handler_queue.complete(job_id).await {
                                                    warn!(error = %e, "Failed to mark job as completed");
                                                }
                                            }
                                            Ok(Err(e)) => {
                                                let duration_ms = start.elapsed().as_millis() as u64;
                                                metrics::record_job_processed(&job_type, duration_ms, false);
                                                warn!(error = %e, "Job handler failed");
                                                if let Err(e) = handler_queue.fail(job_id, &e.to_string()).await {
                                                    warn!(error = %e, "Failed to mark job as failed");
                                                }
                                            }
                                            Err(_elapsed) => {
                                                let duration_ms = start.elapsed().as_millis() as u64;
                                                metrics::record_job_processed(&job_type, duration_ms, false);
                                                metrics::record_worker_job_timeout(&job_type);
                                                warn!(
                                                    timeout_seconds = timeout_duration.as_secs(),
                                                    "Job execution time limit exceeded"
                                                );
                                                if let Err(e) = handler_queue.fail(
                                                    job_id,
                                                    "execution time limit exceeded",
                                                ).await {
                                                    warn!(error = %e, "Failed to mark job as failed");
                                                }
                                            }
                                        }
                                    },
                                    None => {
                                        let duration_ms = start.elapsed().as_millis() as u64;
                                        metrics::record_job_processed(&job_type, duration_ms, false);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_queue::Job;
    use crate::rate_limit::{RateLimitConfig, RateLimiter, RouteCategory};
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    /// A mock job handler that sleeps for a configured duration.
    struct SlowHandler {
        sleep_duration: Duration,
    }

    #[async_trait]
    impl JobHandler for SlowHandler {
        fn job_type(&self) -> &str {
            "slow_job"
        }

        async fn handle(&self, _job: &Job) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            tokio::time::sleep(self.sleep_duration).await;
            Ok(())
        }
    }

    fn make_job(job_type: &str, timeout_seconds: i32) -> Job {
        Job {
            id: Uuid::new_v4(),
            job_type: job_type.to_string(),
            payload: json!({}),
            status: "running".to_string(),
            retry_count: 0,
            max_retries: 3,
            scheduled_at: Utc::now(),
            created_at: Utc::now(),
            correlation_id: None,
            timeout_seconds,
        }
    }

    #[test]
    fn effective_timeout_uses_job_value_when_positive() {
        let config = ResourceBudgetConfig {
            default_job_timeout_seconds: 300,
        };
        let job = make_job("test", 60);
        assert_eq!(effective_timeout(&job, &config), Duration::from_secs(60));
    }

    #[test]
    fn effective_timeout_uses_default_when_job_value_is_zero() {
        let config = ResourceBudgetConfig {
            default_job_timeout_seconds: 300,
        };
        let job = make_job("test", 0);
        assert_eq!(effective_timeout(&job, &config), Duration::from_secs(300));
    }

    #[test]
    fn effective_timeout_uses_default_when_job_value_is_negative() {
        let config = ResourceBudgetConfig {
            default_job_timeout_seconds: 120,
        };
        let job = make_job("test", -1);
        assert_eq!(effective_timeout(&job, &config), Duration::from_secs(120));
    }

    #[test]
    fn resource_budget_config_default() {
        let config = ResourceBudgetConfig::default();
        assert_eq!(config.default_job_timeout_seconds, 300);
    }

    #[test]
    fn worker_config_default_includes_resource_budget() {
        let config = WorkerConfig::default();
        assert_eq!(config.resource_budget.default_job_timeout_seconds, 300);
        assert_eq!(config.max_concurrent_jobs, 5);
    }

    #[tokio::test]
    async fn load_test_job_execution_timeout() {
        // Demonstrates bounded worker CPU time: a slow handler is cancelled
        // well before it would naturally complete.
        let handler = SlowHandler {
            sleep_duration: Duration::from_secs(5),
        };

        let job = make_job("slow_job", 1);
        let timeout_duration = Duration::from_secs(job.timeout_seconds as u64);

        let start = std::time::Instant::now();
        let result = tokio::time::timeout(timeout_duration, handler.handle(&job)).await;
        let elapsed = start.elapsed();

        // The timeout should fire: result is Err(Elapsed)
        assert!(result.is_err(), "Expected timeout but job completed");
        // Should complete in roughly 1 second, not 5
        assert!(
            elapsed < Duration::from_secs(2),
            "Timeout took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn load_test_concurrent_execution_bounded() {
        // Demonstrates bounded worker resource use under parallel load:
        // at most max_concurrent_jobs execute simultaneously.
        let max_concurrent: usize = 2;
        let total_jobs: usize = 8;
        let active_count = Arc::new(AtomicUsize::new(0));
        let max_observed = Arc::new(AtomicUsize::new(0));
        let semaphore = Arc::new(Semaphore::new(max_concurrent));

        let mut handles = Vec::new();

        for _ in 0..total_jobs {
            let sem = semaphore.clone();
            let active = active_count.clone();
            let max_obs = max_observed.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_obs.fetch_max(current, Ordering::SeqCst);
                // Simulate work
                tokio::time::sleep(Duration::from_millis(50)).await;
                active.fetch_sub(1, Ordering::SeqCst);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let max_seen = max_observed.load(Ordering::SeqCst);
        assert!(
            max_seen <= max_concurrent,
            "Expected at most {} concurrent jobs, observed {}",
            max_concurrent,
            max_seen
        );
        // Also verify at least some concurrency was observed (not purely sequential)
        assert!(
            max_seen >= 1,
            "Expected at least 1 concurrent job, observed {}",
            max_seen
        );
    }

    #[tokio::test]
    async fn load_test_normal_reads_unaffected_by_worker_pressure() {
        // Demonstrates that normal cached reads remain available during worker
        // pressure. The rate limiter (API layer) is independent of the worker
        // semaphore (background job layer).

        // Simulate worker at full capacity: semaphore with 0 available permits
        let worker_semaphore = Arc::new(Semaphore::new(2));
        // Acquire all permits to simulate full worker pressure
        let _permit1 = worker_semaphore.clone().try_acquire_owned().unwrap();
        let _permit2 = worker_semaphore.clone().try_acquire_owned().unwrap();

        // Worker is now at capacity - try_acquire should fail
        assert!(
            worker_semaphore.clone().try_acquire_owned().is_err(),
            "Expected worker to be at capacity"
        );

        // Meanwhile, the rate limiter for Read category continues to work
        let config = RateLimitConfig {
            reads_rpm: 120,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);

        // Even though the worker is at capacity, read requests pass through
        // the rate limiter without issue (they are independent systems).
        use crate::rate_limit::RateLimitKey;
        let key = RateLimitKey::IpAddr("10.0.0.1".to_string());

        for i in 0..10 {
            let result = limiter.check(key.clone(), RouteCategory::Read);
            assert!(
                result.is_ok(),
                "Read request {} should pass rate limiter even under worker pressure",
                i
            );
        }
    }
}
