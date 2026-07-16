//! Cross-context transactional commit interface for route version publication.
//!
//! Defines the data structure and trait that the API handler uses to commit
//! a publication. The implementation lives in the platform layer and uses a
//! single database transaction to ensure atomicity across route_versioning,
//! route_editing, activity_catalog, audit, and outbox.

use async_trait::async_trait;

use crate::identity::UserId;
use crate::route_editing::RouteDraftId;

use super::{RouteVersionId, RouteVersioningError};

/// All data needed to commit a route version publication in a single transaction.
///
/// Note: `activity_id` is intentionally not included here. The committer resolves
/// it from the locked draft row inside the transaction, eliminating the TOCTOU
/// window that would exist if the handler pre-loaded the draft to extract it.
#[derive(Debug, Clone)]
pub struct PublicationCommitData {
    /// The draft being published.
    pub draft_id: RouteDraftId,
    /// Expected revision of the draft for optimistic concurrency.
    pub expected_revision: u64,
    /// The user performing the publication.
    pub actor_id: UserId,
    /// Idempotency key to prevent duplicate publications.
    pub idempotency_key: String,
    /// Optional human-readable summary of edits.
    pub edit_summary: Option<String>,
}

/// The result of a successful publication commit.
#[derive(Debug, Clone)]
pub struct PublicationResult {
    /// The ID of the newly created route version.
    pub route_version_id: RouteVersionId,
    /// The version number assigned to the new route version.
    pub version_number: i32,
    /// The draft that was published.
    pub draft_id: RouteDraftId,
}

/// Trait for committing a route version publication in a single transaction.
///
/// Implementations must atomically:
/// 1. Load and lock the draft (SELECT FOR UPDATE)
/// 2. Validate owner, state, and expected revision
/// 3. Check idempotency (return existing result if replay)
/// 4. Compute geometry, bounding box, and statistics from the draft
/// 5. Determine the next version number
/// 6. INSERT into route_versioning.route_versions
/// 7. UPDATE activity_catalog.activities SET current_route_version_id
/// 8. UPDATE route_editing.route_drafts SET state = 'published'
/// 9. INSERT audit log entry
/// 10. INSERT outbox event (RouteVersionPublished)
/// 11. Commit transaction
///
/// If any step fails, the entire transaction must roll back.
#[async_trait]
pub trait CommitPublication: Send + Sync {
    /// Commit the publication atomically.
    ///
    /// Returns the publication result on success.
    async fn commit(
        &self,
        data: &PublicationCommitData,
    ) -> Result<PublicationResult, RouteVersioningError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn publication_commit_data_can_be_constructed() {
        let data = PublicationCommitData {
            draft_id: RouteDraftId::new(Uuid::new_v4()),
            expected_revision: 3,
            actor_id: UserId::new(Uuid::new_v4()),
            idempotency_key: "test-key-123".to_string(),
            edit_summary: Some("Fixed trail section".to_string()),
        };

        assert_eq!(data.expected_revision, 3);
        assert_eq!(data.edit_summary, Some("Fixed trail section".to_string()));
    }

    #[test]
    fn publication_result_can_be_constructed() {
        let result = PublicationResult {
            route_version_id: RouteVersionId::generate(),
            version_number: 2,
            draft_id: RouteDraftId::new(Uuid::new_v4()),
        };

        assert_eq!(result.version_number, 2);
    }
}
