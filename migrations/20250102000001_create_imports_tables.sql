-- Create the imports table for tracking import jobs through their lifecycle.

CREATE TABLE imports.imports (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES identity.users(id),
    source_artifact_id UUID,
    format TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'requested',
    checksum TEXT,
    failure_reason TEXT,
    failure_code TEXT,
    idempotency_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    activity_id UUID,
    UNIQUE(owner_id, idempotency_key)
);

-- Index for listing imports by owner and status
CREATE INDEX idx_imports_owner_status ON imports.imports (owner_id, status);

-- Unique partial index for duplicate detection: only one completed import per checksum per user
CREATE UNIQUE INDEX idx_imports_owner_checksum_completed
    ON imports.imports (owner_id, checksum)
    WHERE status = 'completed';
