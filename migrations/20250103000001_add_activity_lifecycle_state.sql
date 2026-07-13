-- Add lifecycle_state column to support soft deletion of activities.

ALTER TABLE activity_catalog.activities
    ADD COLUMN lifecycle_state TEXT NOT NULL DEFAULT 'active';

-- Index for filtering by lifecycle_state in list queries
CREATE INDEX idx_activities_lifecycle ON activity_catalog.activities (owner_id, lifecycle_state);
