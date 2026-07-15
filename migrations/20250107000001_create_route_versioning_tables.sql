-- Create the route_versioning.route_versions table for immutable published route versions.

CREATE TABLE route_versioning.route_versions (
    id UUID PRIMARY KEY,
    activity_id UUID NOT NULL REFERENCES activity_catalog.activities(id),
    parent_version_id UUID REFERENCES route_versioning.route_versions(id),
    version_number INTEGER NOT NULL,
    geometry_json JSONB NOT NULL,
    bounding_box_json JSONB NOT NULL,
    corrected_statistics_json JSONB,
    calculation_version TEXT NOT NULL,
    edit_summary TEXT,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(activity_id, version_number)
);

CREATE INDEX idx_route_versions_activity ON route_versioning.route_versions (activity_id);

-- Add FK constraint on activity_catalog.activities.current_route_version_id
-- The column already exists but has no foreign key constraint.
ALTER TABLE activity_catalog.activities
    ADD CONSTRAINT fk_activities_current_route_version
    FOREIGN KEY (current_route_version_id)
    REFERENCES route_versioning.route_versions(id);

-- Immutability trigger: prevent UPDATE on route_versioning.route_versions
CREATE OR REPLACE FUNCTION route_versioning.prevent_route_version_update()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'route_versions are immutable: UPDATE is not allowed';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_route_versions_no_update
    BEFORE UPDATE ON route_versioning.route_versions
    FOR EACH ROW
    EXECUTE FUNCTION route_versioning.prevent_route_version_update();

-- Immutability trigger: prevent DELETE on route_versioning.route_versions
CREATE OR REPLACE FUNCTION route_versioning.prevent_route_version_delete()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'route_versions are immutable: DELETE is not allowed';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_route_versions_no_delete
    BEFORE DELETE ON route_versioning.route_versions
    FOR EACH ROW
    EXECUTE FUNCTION route_versioning.prevent_route_version_delete();
