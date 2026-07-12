-- Create tables for the activity catalog bounded context.

CREATE TABLE activity_catalog.activities (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES identity.users(id),
    title TEXT NOT NULL,
    activity_type TEXT NOT NULL DEFAULT 'hike',
    started_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    current_route_version_id UUID,
    recorded_summary_json JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index for listing activities by owner
CREATE INDEX idx_activities_owner ON activity_catalog.activities (owner_id);

-- Index for listing activities by owner sorted by start time
CREATE INDEX idx_activities_owner_started ON activity_catalog.activities (owner_id, started_at DESC);
