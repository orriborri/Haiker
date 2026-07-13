//! Activity catalog command handlers.
//!
//! Contains command logic for mutating activities. Follows the vertical slice
//! pattern: validate inputs, load aggregate, apply domain rules, persist changes.

use async_trait::async_trait;
use uuid::Uuid;

use crate::identity::UserId;

use super::repository::ActivityRepository;
use super::{ActivityCatalogError, ActivityId, ActivityTitle, LifecycleState};

/// Trait for recording audit events.
///
/// Abstracts the audit log so domain commands can request auditing
/// without depending on infrastructure (database, platform crate).
#[async_trait]
pub trait AuditSink: Send + Sync {
    /// Record an audit event.
    async fn record(
        &self,
        actor_id: Uuid,
        action: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(), ActivityCatalogError>;
}

/// A no-op audit sink for testing or environments where audit is not needed.
pub struct NoOpAuditSink;

#[async_trait]
impl AuditSink for NoOpAuditSink {
    async fn record(
        &self,
        _actor_id: Uuid,
        _action: &str,
        _resource_type: &str,
        _resource_id: &str,
    ) -> Result<(), ActivityCatalogError> {
        Ok(())
    }
}

/// Rename an activity's title.
///
/// Validates the new title, loads the activity, verifies ownership and lifecycle,
/// updates the title, and persists the change. Records an audit event on success.
///
/// Returns `ActivityNotFound` (non-disclosing) if the activity is missing,
/// belongs to another owner, or is in Deleted state.
pub async fn rename_activity(
    activity_id: ActivityId,
    owner_id: UserId,
    new_title: &str,
    repo: &dyn ActivityRepository,
    audit: &dyn AuditSink,
) -> Result<super::Activity, ActivityCatalogError> {
    // Validate title first (fail fast before loading)
    let title = ActivityTitle::new(new_title)?;

    // Load the activity
    let mut activity = repo
        .find_by_id(activity_id)
        .await?
        .ok_or(ActivityCatalogError::ActivityNotFound)?;

    // Verify ownership (non-disclosing: return same error as not-found)
    if activity.owner_id != owner_id {
        return Err(ActivityCatalogError::Unauthorized);
    }

    // Verify lifecycle state
    if activity.lifecycle_state == LifecycleState::Deleted {
        return Err(ActivityCatalogError::ActivityNotFound);
    }

    // Apply domain mutation
    activity.update_title(title);

    // Persist
    repo.update(&activity).await?;

    // Record audit event
    audit
        .record(
            owner_id.0,
            "activity.title.updated",
            "activity",
            &activity_id.to_string(),
        )
        .await?;

    Ok(activity)
}

/// Delete (soft-delete) an activity.
///
/// Loads the activity, verifies ownership, transitions to Deleted state,
/// persists the change, and records an audit event. Idempotent: repeated
/// deletion of an already-deleted activity succeeds without error.
///
/// Returns `ActivityNotFound` (non-disclosing) if the activity does not exist
/// or belongs to another owner.
pub async fn delete_activity(
    activity_id: ActivityId,
    owner_id: UserId,
    repo: &dyn ActivityRepository,
    audit: &dyn AuditSink,
) -> Result<(), ActivityCatalogError> {
    // Load the activity
    let mut activity = repo
        .find_by_id(activity_id)
        .await?
        .ok_or(ActivityCatalogError::ActivityNotFound)?;

    // Verify ownership (non-disclosing: return same error as not-found)
    if activity.owner_id != owner_id {
        return Err(ActivityCatalogError::Unauthorized);
    }

    // Idempotent: if already deleted, return success without re-persisting
    if activity.lifecycle_state == LifecycleState::Deleted {
        return Ok(());
    }

    // Apply domain mutation
    activity.delete();

    // Persist
    repo.update(&activity).await?;

    // Record audit event
    audit
        .record(
            owner_id.0,
            "activity.deleted",
            "activity",
            &activity_id.to_string(),
        )
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity_catalog::{Activity, ActivityTitle, ActivityType};
    use crate::identity::UserId;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    use super::super::repository::ActivityPage;

    /// Simple in-memory repository for command tests.
    struct TestRepo {
        activities: Mutex<HashMap<ActivityId, Activity>>,
    }

    impl TestRepo {
        fn with(activity: Activity) -> Self {
            let mut map = HashMap::new();
            map.insert(activity.id, activity);
            Self {
                activities: Mutex::new(map),
            }
        }
    }

    #[async_trait]
    impl ActivityRepository for TestRepo {
        async fn list_activities(
            &self,
            _owner_id: UserId,
            _cursor: Option<&str>,
            _page_size: u32,
        ) -> Result<ActivityPage, ActivityCatalogError> {
            unimplemented!()
        }

        async fn find_by_id(
            &self,
            id: ActivityId,
        ) -> Result<Option<Activity>, ActivityCatalogError> {
            Ok(self.activities.lock().unwrap().get(&id).cloned())
        }

        async fn save(&self, activity: &Activity) -> Result<(), ActivityCatalogError> {
            self.activities
                .lock()
                .unwrap()
                .insert(activity.id, activity.clone());
            Ok(())
        }

        async fn update(&self, activity: &Activity) -> Result<(), ActivityCatalogError> {
            self.activities
                .lock()
                .unwrap()
                .insert(activity.id, activity.clone());
            Ok(())
        }

        async fn delete(&self, id: ActivityId) -> Result<(), ActivityCatalogError> {
            self.activities.lock().unwrap().remove(&id);
            Ok(())
        }
    }

    /// Audit sink that records calls for verification.
    struct TestAuditSink {
        calls: Mutex<Vec<(Uuid, String, String, String)>>,
    }

    impl TestAuditSink {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(Uuid, String, String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AuditSink for TestAuditSink {
        async fn record(
            &self,
            actor_id: Uuid,
            action: &str,
            resource_type: &str,
            resource_id: &str,
        ) -> Result<(), ActivityCatalogError> {
            self.calls.lock().unwrap().push((
                actor_id,
                action.to_string(),
                resource_type.to_string(),
                resource_id.to_string(),
            ));
            Ok(())
        }
    }

    fn make_active_activity(owner_id: UserId, title: &str) -> Activity {
        let title = ActivityTitle::new(title).unwrap();
        Activity::new(owner_id, title, ActivityType::Hike, None, None)
    }

    #[tokio::test]
    async fn rename_succeeds_with_valid_title() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Old Title");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = rename_activity(activity_id, owner, "New Title", &repo, &audit).await;

        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.title.as_str(), "New Title");

        // Verify persistence
        let stored = repo.find_by_id(activity_id).await.unwrap().unwrap();
        assert_eq!(stored.title.as_str(), "New Title");

        // Verify audit
        let calls = audit.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, owner.0);
        assert_eq!(calls[0].1, "activity.title.updated");
        assert_eq!(calls[0].2, "activity");
        assert_eq!(calls[0].3, activity_id.to_string());
    }

    #[tokio::test]
    async fn rename_rejects_empty_title() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Existing");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = rename_activity(activity_id, owner, "", &repo, &audit).await;

        assert!(matches!(
            result,
            Err(ActivityCatalogError::InvalidTitle { .. })
        ));
        // Audit should not be called on failure
        assert_eq!(audit.calls().len(), 0);
    }

    #[tokio::test]
    async fn rename_rejects_whitespace_only_title() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Existing");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = rename_activity(activity_id, owner, "   \t\n  ", &repo, &audit).await;

        assert!(matches!(
            result,
            Err(ActivityCatalogError::InvalidTitle { .. })
        ));
    }

    #[tokio::test]
    async fn rename_truncates_excessive_length() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Short");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let long_title = "x".repeat(600);
        let result = rename_activity(activity_id, owner, &long_title, &repo, &audit).await;

        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.title.as_str().len(), 500);
    }

    #[tokio::test]
    async fn rename_on_deleted_activity_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let mut activity = make_active_activity(owner, "Deleted Activity");
        activity.lifecycle_state = LifecycleState::Deleted;
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = rename_activity(activity_id, owner, "New Name", &repo, &audit).await;

        assert!(matches!(
            result,
            Err(ActivityCatalogError::ActivityNotFound)
        ));
        assert_eq!(audit.calls().len(), 0);
    }

    #[tokio::test]
    async fn rename_cross_owner_returns_unauthorized() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Owner's Activity");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = rename_activity(activity_id, other_user, "Hijack", &repo, &audit).await;

        assert!(matches!(result, Err(ActivityCatalogError::Unauthorized)));
        assert_eq!(audit.calls().len(), 0);
    }

    #[tokio::test]
    async fn rename_nonexistent_activity_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Exists");

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let fake_id = ActivityId::generate();
        let result = rename_activity(fake_id, owner, "New Name", &repo, &audit).await;

        assert!(matches!(
            result,
            Err(ActivityCatalogError::ActivityNotFound)
        ));
    }

    #[tokio::test]
    async fn rename_trims_whitespace_in_title() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Original");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = rename_activity(activity_id, owner, "  Trimmed Title  ", &repo, &audit).await;

        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.title.as_str(), "Trimmed Title");
    }

    // --- delete_activity command tests ---

    #[tokio::test]
    async fn delete_active_activity_succeeds() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "To Delete");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = delete_activity(activity_id, owner, &repo, &audit).await;

        assert!(result.is_ok());

        // Verify persistence: activity should now be Deleted
        let stored = repo.find_by_id(activity_id).await.unwrap().unwrap();
        assert_eq!(stored.lifecycle_state, LifecycleState::Deleted);

        // Verify audit event
        let calls = audit.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, owner.0);
        assert_eq!(calls[0].1, "activity.deleted");
        assert_eq!(calls[0].2, "activity");
        assert_eq!(calls[0].3, activity_id.to_string());
    }

    #[tokio::test]
    async fn delete_already_deleted_is_idempotent() {
        let owner = UserId::new(Uuid::new_v4());
        let mut activity = make_active_activity(owner, "Already Deleted");
        activity.lifecycle_state = LifecycleState::Deleted;
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = delete_activity(activity_id, owner, &repo, &audit).await;

        assert!(result.is_ok());

        // No audit event for idempotent re-deletion
        assert_eq!(audit.calls().len(), 0);
    }

    #[tokio::test]
    async fn delete_cross_owner_returns_unauthorized() {
        let owner = UserId::new(Uuid::new_v4());
        let other_user = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Owner's Activity");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let result = delete_activity(activity_id, other_user, &repo, &audit).await;

        assert!(matches!(result, Err(ActivityCatalogError::Unauthorized)));
        assert_eq!(audit.calls().len(), 0);
    }

    #[tokio::test]
    async fn delete_nonexistent_returns_not_found() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Exists");

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        let fake_id = ActivityId::generate();
        let result = delete_activity(fake_id, owner, &repo, &audit).await;

        assert!(matches!(
            result,
            Err(ActivityCatalogError::ActivityNotFound)
        ));
    }

    #[tokio::test]
    async fn deleted_activity_rejects_rename() {
        let owner = UserId::new(Uuid::new_v4());
        let activity = make_active_activity(owner, "Will Delete");
        let activity_id = activity.id;

        let repo = TestRepo::with(activity);
        let audit = TestAuditSink::new();

        // First delete it
        delete_activity(activity_id, owner, &repo, &audit)
            .await
            .unwrap();

        // Now try to rename
        let result = rename_activity(activity_id, owner, "New Name", &repo, &audit).await;
        assert!(matches!(
            result,
            Err(ActivityCatalogError::ActivityNotFound)
        ));
    }
}
