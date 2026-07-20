-- Create the legs table for multi-leg hike support.
-- Each leg represents one segment of an activity (e.g., a day of a multi-day hike).
-- Existing single-track activities are migrated to have a single leg each.

CREATE TABLE recorded_activity.legs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    activity_id UUID NOT NULL REFERENCES activity_catalog.activities(id),
    leg_number INTEGER NOT NULL,
    title TEXT,
    date DATE NOT NULL,
    source_revision_id UUID REFERENCES recorded_activity.source_revisions(id),
    recorded_track_id UUID REFERENCES recorded_activity.recorded_tracks(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(activity_id, leg_number)
);

-- Index for efficient lookups by activity
CREATE INDEX idx_legs_activity_id ON recorded_activity.legs (activity_id);

-- Data migration: insert a single leg (leg_number=1) for each existing activity
-- that has a source revision and recorded track. Activities without tracks
-- (e.g., incomplete imports) are skipped.
INSERT INTO recorded_activity.legs (activity_id, leg_number, date, source_revision_id, recorded_track_id)
SELECT
    a.id,
    1,
    COALESCE(a.started_at::date, a.created_at::date),
    sr.id,
    rt.id
FROM activity_catalog.activities a
JOIN recorded_activity.source_revisions sr ON sr.activity_id = a.id
JOIN recorded_activity.recorded_tracks rt ON rt.source_revision_id = sr.id;
