-- Create tables for the recorded activity bounded context.

CREATE TABLE recorded_activity.source_artifacts (
    id UUID PRIMARY KEY,
    import_id UUID NOT NULL,
    owner_id UUID NOT NULL,
    object_storage_key TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    checksum TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE recorded_activity.source_revisions (
    id UUID PRIMARY KEY,
    activity_id UUID NOT NULL,
    source_artifact_id UUID NOT NULL REFERENCES recorded_activity.source_artifacts(id),
    revision_number INTEGER NOT NULL DEFAULT 1,
    provider TEXT NOT NULL DEFAULT 'gpx_upload',
    parser_version TEXT NOT NULL,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(activity_id, revision_number)
);

CREATE TABLE recorded_activity.recorded_tracks (
    id UUID PRIMARY KEY,
    source_revision_id UUID NOT NULL REFERENCES recorded_activity.source_revisions(id),
    geometry_json JSONB NOT NULL,
    bounding_box_json JSONB NOT NULL,
    started_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    distance_meters DOUBLE PRECISION NOT NULL,
    elevation_gain_meters DOUBLE PRECISION,
    elevation_loss_meters DOUBLE PRECISION,
    point_count INTEGER NOT NULL,
    segment_count INTEGER NOT NULL,
    calculation_version TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
