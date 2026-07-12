//! Job type definitions for the import processing pipeline.
//!
//! Defines the contract between the API (enqueue side) and the worker
//! (handler side) for asynchronous import processing jobs.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Job type identifier for GPX parsing jobs.
pub const PARSE_GPX_JOB_TYPE: &str = "parse_gpx";

/// Payload for the `parse_gpx` job.
///
/// Contains all information the worker needs to process a GPX import
/// without additional repository lookups for the initial dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseGpxJob {
    /// The import aggregate ID to process.
    pub import_id: Uuid,
    /// The user who owns this import.
    pub owner_id: Uuid,
    /// The object storage key where the GPX file is stored.
    pub object_storage_key: String,
    /// Correlation ID for distributed tracing.
    pub correlation_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gpx_job_serializes_to_json() {
        let job = ParseGpxJob {
            import_id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            object_storage_key: "imports/user-123/import-456".to_string(),
            correlation_id: Uuid::new_v4(),
        };

        let json = serde_json::to_value(&job).unwrap();
        assert!(json["import_id"].is_string());
        assert!(json["owner_id"].is_string());
        assert_eq!(json["object_storage_key"], "imports/user-123/import-456");
        assert!(json["correlation_id"].is_string());
    }

    #[test]
    fn parse_gpx_job_deserializes_from_json() {
        let import_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let correlation_id = Uuid::new_v4();

        let json = serde_json::json!({
            "import_id": import_id.to_string(),
            "owner_id": owner_id.to_string(),
            "object_storage_key": "imports/abc/def",
            "correlation_id": correlation_id.to_string(),
        });

        let job: ParseGpxJob = serde_json::from_value(json).unwrap();
        assert_eq!(job.import_id, import_id);
        assert_eq!(job.owner_id, owner_id);
        assert_eq!(job.object_storage_key, "imports/abc/def");
        assert_eq!(job.correlation_id, correlation_id);
    }

    #[test]
    fn job_type_constant_is_correct() {
        assert_eq!(PARSE_GPX_JOB_TYPE, "parse_gpx");
    }
}
