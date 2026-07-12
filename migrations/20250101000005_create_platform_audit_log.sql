-- Platform audit log for tracking user actions on resources.
CREATE TABLE platform.audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id UUID NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_log_actor ON platform.audit_log (actor_id);
CREATE INDEX idx_audit_log_resource ON platform.audit_log (resource_type, resource_id);
CREATE INDEX idx_audit_log_created ON platform.audit_log (created_at);
