-- Create identity.sessions table for cookie-based session management.
CREATE TABLE identity.sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES identity.users(id),
    token_hash TEXT NOT NULL,
    csrf_token TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ
);

-- Unique index on token_hash for fast session lookup.
CREATE UNIQUE INDEX idx_sessions_token_hash ON identity.sessions (token_hash);

-- Index on user_id for listing/revoking user sessions.
CREATE INDEX idx_sessions_user_id ON identity.sessions (user_id);

-- Partial index on expires_at for cleanup of active (non-revoked) sessions.
CREATE INDEX idx_sessions_expires_at_active ON identity.sessions (expires_at)
    WHERE revoked_at IS NULL;
