//! Job type definitions for the export processing pipeline.
//!
//! Defines the contract between the API (enqueue side) and the worker
//! (handler side) for asynchronous export generation jobs.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Job type identifier for export generation jobs.
pub const GENERATE_EXPORT_JOB_TYPE: &str = "generate_export";

/// Payload for the `generate_export` job.
///
/// Contains all information the worker needs to generate an export file
/// without additional repository lookups for the initial dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateExportJob {
    /// The export job aggregate ID to process.
    pub export_job_id: Uuid,
    /// The activity being exported.
    pub activity_id: Uuid,
    /// The specific route version to export.
    pub route_version_id: Uuid,
    /// The user who requested the export.
    pub owner_id: Uuid,
    /// The export format (e.g., "gpx").
    pub format: String,
    /// Correlation ID for distributed tracing.
    pub correlation_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_export_job_serializes_to_json() {
        let job = GenerateExportJob {
            export_job_id: Uuid::new_v4(),
            activity_id: Uuid::new_v4(),
            route_version_id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            format: "gpx".to_string(),
            correlation_id: Uuid::new_v4(),
        };

        let json = serde_json::to_value(&job).unwrap();
        assert!(json["export_job_id"].is_string());
        assert!(json["activity_id"].is_string());
        assert!(json["route_version_id"].is_string());
        assert!(json["owner_id"].is_string());
        assert_eq!(json["format"], "gpx");
        assert!(json["correlation_id"].is_string());
    }

    #[test]
    fn generate_export_job_deserializes_from_json() {
        let export_job_id = Uuid::new_v4();
        let activity_id = Uuid::new_v4();
        let route_version_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let correlation_id = Uuid::new_v4();

        let json = serde_json::json!({
            "export_job_id": export_job_id.to_string(),
            "activity_id": activity_id.to_string(),
            "route_version_id": route_version_id.to_string(),
            "owner_id": owner_id.to_string(),
            "format": "gpx",
            "correlation_id": correlation_id.to_string(),
        });

        let job: GenerateExportJob = serde_json::from_value(json).unwrap();
        assert_eq!(job.export_job_id, export_job_id);
        assert_eq!(job.activity_id, activity_id);
        assert_eq!(job.route_version_id, route_version_id);
        assert_eq!(job.owner_id, owner_id);
        assert_eq!(job.format, "gpx");
        assert_eq!(job.correlation_id, correlation_id);
    }

    #[test]
    fn job_type_constant_is_correct() {
        assert_eq!(GENERATE_EXPORT_JOB_TYPE, "generate_export");
    }
}
