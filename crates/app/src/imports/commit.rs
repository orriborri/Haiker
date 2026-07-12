//! Cross-context transactional commit interface for imports.
//!
//! Defines the data structure and trait that the orchestrator uses to commit
//! a successfully parsed import. The implementation lives in the platform layer
//! and uses a single database transaction to ensure atomicity.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::activity_catalog::{ActivityId, ActivityType};
use crate::identity::UserId;
use crate::recorded_activity::{
    BoundingBox, Coordinate, RecordedStatistics, RecordedTrackId, SourceArtifactId,
    SourceRevisionId, TrackSegment,
};

use super::ImportError;
use super::ImportId;

/// All data needed to commit a successfully parsed import across bounded contexts.
///
/// This struct is produced by the orchestrator after normalization and passed
/// to the CommitImport implementation. The infrastructure layer uses this to
/// insert data into multiple tables within a single transaction.
#[derive(Debug, Clone)]
pub struct ImportCommitData {
    // -- Identity --
    pub owner_id: UserId,
    pub import_id: ImportId,
    pub correlation_id: Uuid,

    // -- Source Artifact (recorded_activity context) --
    pub source_artifact_id: SourceArtifactId,
    pub object_storage_key: String,
    pub content_type: String,
    pub file_size_bytes: i64,
    pub checksum: String,

    // -- Source Revision (recorded_activity context) --
    pub source_revision_id: SourceRevisionId,
    pub revision_number: i32,
    pub parser_version: String,

    // -- Recorded Track (recorded_activity context) --
    pub recorded_track_id: RecordedTrackId,
    pub segments: Vec<TrackSegment>,
    pub bounding_box: BoundingBox,
    pub statistics: RecordedStatistics,
    pub preview_geometry: Vec<Coordinate>,

    // -- Activity (activity_catalog context) --
    pub activity_id: ActivityId,
    pub activity_title: String,
    pub activity_type: ActivityType,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// Trait for committing a fully parsed import in a single transaction.
///
/// Implementations must:
/// 1. Insert the source artifact record
/// 2. Insert the source revision linking artifact to activity
/// 3. Insert the recorded track with segments and statistics
/// 4. Insert the activity in the catalog
/// 5. Update the import status to Completed with the activity_id
/// 6. Write an audit event
/// 7. Write an outbox event (ImportedActivityCommitted)
///
/// All of the above must happen in a single database transaction.
/// If any step fails, the entire transaction must roll back.
#[async_trait]
pub trait CommitImport: Send + Sync {
    /// Commit the import data atomically.
    ///
    /// Returns the activity_id of the newly created activity on success.
    async fn commit(&self, data: &ImportCommitData) -> Result<ActivityId, ImportError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorded_activity::{Coordinate, Elevation, TrackPoint};

    #[test]
    fn import_commit_data_can_be_constructed() {
        let owner_id = UserId::new(Uuid::new_v4());
        let import_id = ImportId::new(Uuid::new_v4());

        let p1 = TrackPoint::new(
            Coordinate::new(47.0, 11.0).unwrap(),
            Some(Elevation::new(500.0)),
            None,
        );
        let p2 = TrackPoint::new(
            Coordinate::new(47.1, 11.1).unwrap(),
            Some(Elevation::new(600.0)),
            None,
        );
        let segment = TrackSegment::new(vec![p1, p2]).unwrap();

        let bbox = BoundingBox::new(
            Coordinate::new(47.0, 11.0).unwrap(),
            Coordinate::new(47.1, 11.1).unwrap(),
        );

        let data = ImportCommitData {
            owner_id,
            import_id,
            correlation_id: Uuid::new_v4(),
            source_artifact_id: SourceArtifactId::generate(),
            object_storage_key: "imports/user/file".to_string(),
            content_type: "application/gpx+xml".to_string(),
            file_size_bytes: 1024,
            checksum: "a".repeat(64),
            source_revision_id: SourceRevisionId::generate(),
            revision_number: 1,
            parser_version: "1.0.0".to_string(),
            recorded_track_id: RecordedTrackId::generate(),
            segments: vec![segment],
            bounding_box: bbox,
            statistics: RecordedStatistics {
                distance_meters: 1000.0,
                duration_seconds: Some(600.0),
                elevation_gain_meters: Some(100.0),
                elevation_loss_meters: Some(50.0),
                point_count: 2,
                segment_count: 1,
            },
            preview_geometry: vec![
                Coordinate::new(47.0, 11.0).unwrap(),
                Coordinate::new(47.1, 11.1).unwrap(),
            ],
            activity_id: ActivityId::generate(),
            activity_title: "Morning Hike".to_string(),
            activity_type: ActivityType::Hike,
            started_at: None,
            ended_at: None,
        };

        assert_eq!(data.owner_id, owner_id);
        assert_eq!(data.import_id, import_id);
        assert_eq!(data.activity_title, "Morning Hike");
    }
}
