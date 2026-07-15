-- Add a partial unique index on (job_type, correlation_id) for active jobs.
-- This ensures that only one active (pending/running) job can exist per
-- (job_type, correlation_id) pair, enabling idempotent enqueue operations.
CREATE UNIQUE INDEX idx_jobs_idempotent_correlation
    ON platform.jobs (job_type, correlation_id)
    WHERE correlation_id IS NOT NULL
      AND status NOT IN ('completed', 'cancelled', 'failed');
