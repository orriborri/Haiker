-- Add corrected_summary_json column to activities for the catalog read model.
-- This stores the corrected statistics computed during publication, enabling
-- the activity library to display corrected values alongside recorded values.

ALTER TABLE activity_catalog.activities
    ADD COLUMN corrected_summary_json JSONB;

COMMENT ON COLUMN activity_catalog.activities.corrected_summary_json IS
    'Corrected statistics computed from the published route geometry (distance, point count, algorithm version)';
