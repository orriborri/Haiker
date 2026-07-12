//! PostgreSQL-based job queue.
//!
//! Provides reliable background job processing using `FOR UPDATE SKIP LOCKED`
//! for safe concurrent polling by multiple worker instances.

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
    pub async fn enqueue(
        &self,
        job_type: &str,
        payload: Value,
    ) -> Result<Uuid, sqlx::Error> {
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
        let row = sqlx::query_as::<_, (Uuid, String, Value, String, i32, i32, DateTime<Utc>, DateTime<Utc>)>(
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
            RETURNING id, job_type, payload, status, retry_count, max_retries, scheduled_at, created_at
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(id, job_type, payload, status, retry_count, max_retries, scheduled_at, created_at)| Job {
            id,
            job_type,
            payload,
            status,
            retry_count,
            max_retries,
            scheduled_at,
            created_at,
        }))
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
