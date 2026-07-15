-- Add immutability triggers to recorded_activity tables.
-- source_revisions and recorded_tracks are immutable after import per domain rules.

-- Immutability trigger: prevent UPDATE on recorded_activity.source_revisions
CREATE OR REPLACE FUNCTION recorded_activity.prevent_source_revision_update()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'source_revisions are immutable: UPDATE is not allowed';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_source_revisions_no_update
    BEFORE UPDATE ON recorded_activity.source_revisions
    FOR EACH ROW
    EXECUTE FUNCTION recorded_activity.prevent_source_revision_update();

-- Immutability trigger: prevent DELETE on recorded_activity.source_revisions
CREATE OR REPLACE FUNCTION recorded_activity.prevent_source_revision_delete()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'source_revisions are immutable: DELETE is not allowed';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_source_revisions_no_delete
    BEFORE DELETE ON recorded_activity.source_revisions
    FOR EACH ROW
    EXECUTE FUNCTION recorded_activity.prevent_source_revision_delete();

-- Immutability trigger: prevent UPDATE on recorded_activity.recorded_tracks
CREATE OR REPLACE FUNCTION recorded_activity.prevent_recorded_track_update()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'recorded_tracks are immutable: UPDATE is not allowed';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_recorded_tracks_no_update
    BEFORE UPDATE ON recorded_activity.recorded_tracks
    FOR EACH ROW
    EXECUTE FUNCTION recorded_activity.prevent_recorded_track_update();

-- Immutability trigger: prevent DELETE on recorded_activity.recorded_tracks
CREATE OR REPLACE FUNCTION recorded_activity.prevent_recorded_track_delete()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'recorded_tracks are immutable: DELETE is not allowed';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_recorded_tracks_no_delete
    BEFORE DELETE ON recorded_activity.recorded_tracks
    FOR EACH ROW
    EXECUTE FUNCTION recorded_activity.prevent_recorded_track_delete();
