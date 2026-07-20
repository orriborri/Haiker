//! PostgreSQL-based job queue.
//!
//! Provides reliable background job processing using `FOR UPDATE SKIP LOCKED`
//! for safe concurrent polling by multiple worker instances.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Represents a job in the queue.
#[derive(Debug, Clone)]
pub struct Job {
    /// Unique job identifier.
    pub id: Uuid,
    /// Type discriminator for routing to the correct handler.
    pub job_type: String,
    /// JSON payload for the job.
    pub payload: Value,
    /// Current job status.
    pub status: String,
    /// Number of retry attempts so far.
    pub retry_count: i32,
    /// Maximum number of retries allowed.
    pub max_retries: i32,
    /// When the job is scheduled to run.
    pub scheduled_at: DateTime<Utc>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// Optional correlation ID for distributed tracing.
    pub correlation_id: Option<Uuid>,
    /// Timeout in seconds for job execution.
    pub timeout_seconds: i32,
}

/// Trait for handling jobs from the queue.
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// The job type this handler processes.
    fn job_type(&self) -> &str;

    /// Handle a job. Return Ok(()) on success, Err on failure.
    async fn handle(&self, job: &Job) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Job queue backed by PostgreSQL.
#[derive(Clone)]
pub struct JobQueue {
    pool: PgPool,
}

impl JobQueue {
    /// Create a new job queue instance.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Enqueue a new job for processing.
    pub async fn enqueue(&self, job_type: &str, payload: Value) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO platform.jobs (id, job_type, payload, status, scheduled_at)
            VALUES ($1, $2, $3, 'pending', now())
            "#,
        )
        .bind(id)
        .bind(job_type)
        .bind(&payload)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    /// Enqueue a new job with a correlation ID for distributed tracing.
    pub async fn enqueue_with_correlation(
        &self,
        job_type: &str,
        payload: Value,
        correlation_id: Uuid,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO platform.jobs (id, job_type, payload, status, scheduled_at, correlation_id)
            VALUES ($1, $2, $3, 'pending', now(), $4)
            "#,
        )
        .bind(id)
        .bind(job_type)
        .bind(&payload)
        .bind(correlation_id)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    /// Enqueue a job idempotently using a correlation_id as the deduplication key.
    ///
    /// Uses `ON CONFLICT DO NOTHING` against the partial unique index on
    /// (job_type, correlation_id) for active jobs. If a job with the same
    /// job_type and correlation_id already exists in a non-terminal state,
    /// the existing job's ID is returned instead of creating a duplicate.
    ///
    /// Handles the TOCTOU race where the conflicting job transitions to a
    /// terminal state between the INSERT and the SELECT: if the SELECT returns
    /// no rows, the INSERT is retried (the partial unique index no longer blocks
    /// since the old job is now terminal).
    pub async fn enqueue_idempotent(
        &self,
        job_type: &str,
        payload: Value,
        correlation_id: Uuid,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();

        // Attempt to insert; ON CONFLICT DO NOTHING if an active job already exists
        let result = sqlx::query(
            r#"
            INSERT INTO platform.jobs (id, job_type, payload, status, scheduled_at, correlation_id)
            VALUES ($1, $2, $3, 'pending', now(), $4)
            ON CONFLICT (job_type, correlation_id)
                WHERE correlation_id IS NOT NULL
                  AND status NOT IN ('completed', 'cancelled', 'failed')
            DO NOTHING
            "#,
        )
        .bind(id)
        .bind(job_type)
        .bind(&payload)
        .bind(correlation_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            // A job already exists - look up its ID using fetch_optional to handle
            // the race where the job transitions to a terminal state between the
            // INSERT and this SELECT.
            let existing_id = sqlx::query_as::<_, (Uuid,)>(
                r#"
                SELECT id FROM platform.jobs
                WHERE job_type = $1
                  AND correlation_id = $2
                  AND status NOT IN ('completed', 'cancelled', 'failed')
                LIMIT 1
                "#,
            )
            .bind(job_type)
            .bind(correlation_id)
            .fetch_optional(&self.pool)
            .await?;

            match existing_id {
                Some(row) => Ok(row.0),
                None => {
                    // The conflicting job transitioned to a terminal state between
                    // our INSERT and SELECT. The partial unique index no longer
                    // blocks, so retry the INSERT with a fresh ID.
                    let retry_id = Uuid::new_v4();
                    sqlx::query(
                        r#"
                        INSERT INTO platform.jobs (id, job_type, payload, status, scheduled_at, correlation_id)
                        VALUES ($1, $2, $3, 'pending', now(), $4)
                        ON CONFLICT (job_type, correlation_id)
                            WHERE correlation_id IS NOT NULL
                              AND status NOT IN ('completed', 'cancelled', 'failed')
                        DO NOTHING
                        "#,
                    )
                    .bind(retry_id)
                    .bind(job_type)
                    .bind(&payload)
                    .bind(correlation_id)
                    .execute(&self.pool)
                    .await?;

                    Ok(retry_id)
                }
            }
        } else {
            Ok(id)
        }
    }

    /// Enqueue a job with a specific scheduled time.
    pub async fn enqueue_scheduled(
        &self,
        job_type: &str,
        payload: Value,
        scheduled_at: DateTime<Utc>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO platform.jobs (id, job_type, payload, status, scheduled_at)
            VALUES ($1, $2, $3, 'pending', $4)
            "#,
        )
        .bind(id)
        .bind(job_type)
        .bind(&payload)
        .bind(scheduled_at)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    /// Poll for the next available job using FOR UPDATE SKIP LOCKED.
    ///
    /// This atomically selects and locks a pending job, preventing other workers
    /// from picking up the same job.
    pub async fn poll(&self) -> Result<Option<Job>, sqlx::Error> {
        let row = sqlx::query_as::<_, (Uuid, String, Value, String, i32, i32, DateTime<Utc>, DateTime<Utc>, Option<Uuid>, i32)>(
            r#"
            UPDATE platform.jobs
            SET status = 'running', started_at = now(), updated_at = now()
            WHERE id = (
                SELECT id FROM platform.jobs
                WHERE status = 'pending' AND scheduled_at <= now()
                ORDER BY priority DESC, scheduled_at ASC
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            RETURNING id, job_type, payload, status, retry_count, max_retries, scheduled_at, created_at, correlation_id, timeout_seconds
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(
                id,
                job_type,
                payload,
                status,
                retry_count,
                max_retries,
                scheduled_at,
                created_at,
                correlation_id,
                timeout_seconds,
            )| Job {
                id,
                job_type,
                payload,
                status,
                retry_count,
                max_retries,
                scheduled_at,
                created_at,
                correlation_id,
                timeout_seconds,
            },
        ))
    }

    /// Mark a job as completed.
    pub async fn complete(&self, job_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE platform.jobs
            SET status = 'completed', completed_at = now(), updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Cancel a job that is pending or running.
    pub async fn cancel(&self, job_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE platform.jobs
            SET status = 'cancelled', updated_at = now()
            WHERE id = $1 AND status IN ('pending', 'running')
            "#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Find and handle stale jobs that have exceeded their timeout.
    ///
    /// Jobs in 'running' state past their timeout are reset to 'pending' if
    /// retries remain, otherwise marked as 'failed'.
    pub async fn timeout_stale_jobs(&self) -> Result<u64, sqlx::Error> {
        // Reset timed-out jobs that have retries remaining back to pending
        let reset_result = sqlx::query(
            r#"
            UPDATE platform.jobs
            SET status = 'pending',
                retry_count = retry_count + 1,
                error_message = 'job timed out',
                updated_at = now()
            WHERE status = 'running'
              AND started_at + make_interval(secs => timeout_seconds::double precision) < now()
              AND retry_count + 1 < max_retries
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Mark timed-out jobs that exhausted retries as failed
        let failed_result = sqlx::query(
            r#"
            UPDATE platform.jobs
            SET status = 'failed',
                retry_count = retry_count + 1,
                error_message = 'job timed out (max retries exhausted)',
                failed_at = now(),
                updated_at = now()
            WHERE status = 'running'
              AND started_at + make_interval(secs => timeout_seconds::double precision) < now()
              AND retry_count + 1 >= max_retries
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(reset_result.rows_affected() + failed_result.rows_affected())
    }

    /// Mark a job as failed with exponential backoff retry.
    ///
    /// If the job has remaining retries, it is rescheduled with an exponential
    /// backoff delay. Otherwise, it is permanently marked as failed.
    pub async fn fail(&self, job_id: Uuid, error_message: &str) -> Result<(), sqlx::Error> {
        // Check current retry state
        let row = sqlx::query_as::<_, (i32, i32)>(
            r#"
            SELECT retry_count, max_retries FROM platform.jobs WHERE id = $1
            "#,
        )
        .bind(job_id)
        .fetch_one(&self.pool)
        .await?;

        let (retry_count, max_retries) = row;
        let new_retry_count = retry_count + 1;

        if new_retry_count < max_retries {
            // Exponential backoff: 2^retry_count seconds
            let backoff_seconds = 2_i64.pow(new_retry_count as u32);

            sqlx::query(
                r#"
                UPDATE platform.jobs
                SET status = 'pending',
                    retry_count = $2,
                    error_message = $3,
                    scheduled_at = now() + make_interval(secs => $4::double precision),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(job_id)
            .bind(new_retry_count)
            .bind(error_message)
            .bind(backoff_seconds as f64)
            .execute(&self.pool)
            .await?;
        } else {
            // Permanently failed
            sqlx::query(
                r#"
                UPDATE platform.jobs
                SET status = 'failed',
                    retry_count = $2,
                    error_message = $3,
                    failed_at = now(),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(job_id)
            .bind(new_retry_count)
            .bind(error_message)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
}
