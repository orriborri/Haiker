-- Add correlation_id and timeout_seconds to job queue for tracing and timeout detection.
ALTER TABLE platform.jobs ADD COLUMN IF NOT EXISTS correlation_id UUID;
ALTER TABLE platform.jobs ADD COLUMN IF NOT EXISTS timeout_seconds INTEGER NOT NULL DEFAULT 300;

CREATE INDEX idx_jobs_correlation ON platform.jobs (correlation_id) WHERE correlation_id IS NOT NULL;
