-- Route drafts table
CREATE TABLE route_editing.drafts (
    id UUID PRIMARY KEY,
    activity_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES identity.users(id),
    base_route_version_id UUID,
    revision BIGINT NOT NULL DEFAULT 0,
    geometry JSONB NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ
);
CREATE INDEX idx_drafts_activity_owner ON route_editing.drafts (activity_id, owner_id) WHERE state = 'active';
CREATE INDEX idx_drafts_owner ON route_editing.drafts (owner_id);

-- Draft operations for history/idempotency
CREATE TABLE route_editing.draft_operations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    draft_id UUID NOT NULL REFERENCES route_editing.drafts(id) ON DELETE CASCADE,
    operation_id UUID NOT NULL UNIQUE,
    operation_type TEXT NOT NULL,
    operation_data JSONB NOT NULL,
    sequence_number INTEGER NOT NULL,
    is_undone BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_draft_ops_draft_seq ON route_editing.draft_operations (draft_id, sequence_number);

-- Idempotency key storage
CREATE TABLE route_editing.idempotency_keys (
    key TEXT NOT NULL,
    owner_id UUID NOT NULL,
    draft_id UUID NOT NULL REFERENCES route_editing.drafts(id) ON DELETE CASCADE,
    response_data JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '24 hours'),
    PRIMARY KEY (owner_id, key)
);
