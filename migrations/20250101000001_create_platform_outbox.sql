-- Transactional outbox table for reliable event publishing.
-- Events are written in the same transaction as domain state changes,
-- then dispatched asynchronously by the worker process.

CREATE TABLE IF NOT EXISTS platform.outbox (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ,
    failed_at TIMESTAMPTZ,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_outbox_unprocessed
    ON platform.outbox (created_at)
    WHERE processed_at IS NULL AND failed_at IS NULL;
