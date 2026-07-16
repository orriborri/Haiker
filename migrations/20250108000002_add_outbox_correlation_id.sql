-- Add correlation_id to the outbox for distributed tracing, matching platform.jobs.
ALTER TABLE platform.outbox ADD COLUMN IF NOT EXISTS correlation_id UUID;

CREATE INDEX IF NOT EXISTS idx_outbox_correlation ON platform.outbox (correlation_id) WHERE correlation_id IS NOT NULL;
