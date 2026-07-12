//! Activity Catalog bounded context.
//!
//! Owns activity identity, title, type, timestamps, current route version,
//! summary statistics, and lifecycle management.

pub mod queries;
pub mod repository;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::identity::UserId;

/// A strongly-typed activity identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActivityId(pub Uuid);

impl ActivityId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for ActivityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Maximum allowed length for an activity title.
const MAX_TITLE_LENGTH: usize = 500;

/// A validated activity title (non-empty after trim, max 500 characters).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityTitle(String);

impl ActivityTitle {
    /// Create a new ActivityTitle, validating that it is non-empty after trimming
    /// and truncating at 500 characters if needed.
    pub fn new(value: impl Into<String>) -> Result<Self, ActivityCatalogError> {
        let value = value.into();
        let trimmed = value.trim().to_string();

        if trimmed.is_empty() {
            return Err(ActivityCatalogError::InvalidTitle {
                message: "title must not be empty".to_string(),
            });
        }

        let truncated = if trimmed.len() > MAX_TITLE_LENGTH {
            trimmed[..MAX_TITLE_LENGTH].to_string()
        } else {
            trimmed
        };

        Ok(Self(truncated))
    }

    /// Returns the title value as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ActivityTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The type/category of an activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    /// A hiking activity.
    Hike,
    /// A walking activity.
    Walk,
    /// A running activity.
    Run,
    /// Any other activity type.
    Other,
}

/// Lifecycle state of an activity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    /// The activity is active and visible.
    #[default]
    Active,
    /// The activity has been soft-deleted.
    Deleted,
}

impl std::fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            LifecycleState::Active => "active",
            LifecycleState::Deleted => "deleted",
        };
        write!(f, "{s}")
    }
}

impl std::fmt::Display for ActivityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ActivityType::Hike => "hike",
            ActivityType::Walk => "walk",
            ActivityType::Run => "run",
            ActivityType::Other => "other",
        };
        write!(f, "{s}")
    }
}

/// The Activity aggregate representing a user's activity in the catalog.
#[derive(Debug, Clone)]
pub struct Activity {
    pub id: ActivityId,
    pub owner_id: UserId,
    pub title: ActivityTitle,
    pub activity_type: ActivityType,
    pub lifecycle_state: LifecycleState,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub recorded_summary: Option<serde_json::Value>,
    pub corrected_summary: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Activity {
    /// Create a new Activity with the given parameters.
    pub fn new(
        owner_id: UserId,
        title: ActivityTitle,
        activity_type: ActivityType,
        started_at: Option<DateTime<Utc>>,
        ended_at: Option<DateTime<Utc>>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: ActivityId::generate(),
            owner_id,
            title,
            activity_type,
            lifecycle_state: LifecycleState::default(),
            started_at,
            ended_at,
            recorded_summary: None,
            corrected_summary: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Update the activity title.
    pub fn update_title(&mut self, title: ActivityTitle) {
        self.title = title;
        self.updated_at = Utc::now();
    }

    /// Update the activity type.
    pub fn update_activity_type(&mut self, activity_type: ActivityType) {
        self.activity_type = activity_type;
        self.updated_at = Utc::now();
    }
}

/// Errors that can occur in the activity catalog context.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ActivityCatalogError {
    /// The requested activity was not found.
    #[error("activity not found")]
    ActivityNotFound,

    /// The activity title is invalid.
    #[error("invalid title: {message}")]
    InvalidTitle { message: String },

    /// The user is not authorized to access this activity.
    #[error("unauthorized")]
    Unauthorized,

    /// A persistence error occurred.
    #[error("persistence error: {message}")]
    PersistenceError { message: String },

    /// Invalid cursor provided for pagination.
    #[error("invalid cursor: {message}")]
    InvalidCursor { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_title_valid() {
        let title = ActivityTitle::new("Morning Hike").unwrap();
        assert_eq!(title.as_str(), "Morning Hike");
    }

    #[test]
    fn activity_title_trims_whitespace() {
        let title = ActivityTitle::new("  Morning Hike  ").unwrap();
        assert_eq!(title.as_str(), "Morning Hike");
    }

    #[test]
    fn activity_title_rejects_empty() {
        let result = ActivityTitle::new("");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ActivityCatalogError::InvalidTitle { .. }
        ));
    }

    #[test]
    fn activity_title_rejects_whitespace_only() {
        let result = ActivityTitle::new("   ");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ActivityCatalogError::InvalidTitle { .. }
        ));
    }

    #[test]
    fn activity_title_truncates_at_500_chars() {
        let long_title = "a".repeat(600);
        let title = ActivityTitle::new(long_title).unwrap();
        assert_eq!(title.as_str().len(), 500);
    }

    #[test]
    fn activity_title_exactly_500_chars_is_ok() {
        let exact_title = "b".repeat(500);
        let title = ActivityTitle::new(exact_title.clone()).unwrap();
        assert_eq!(title.as_str(), exact_title);
    }

    #[test]
    fn activity_creation() {
        let owner_id = UserId::new(Uuid::new_v4());
        let title = ActivityTitle::new("Evening Walk").unwrap();
        let activity = Activity::new(owner_id, title, ActivityType::Walk, None, None);

        assert_eq!(activity.owner_id, owner_id);
        assert_eq!(activity.title.as_str(), "Evening Walk");
        assert_eq!(activity.activity_type, ActivityType::Walk);
        assert!(activity.started_at.is_none());
        assert!(activity.ended_at.is_none());
    }

    #[test]
    fn activity_update_title() {
        let owner_id = UserId::new(Uuid::new_v4());
        let title = ActivityTitle::new("Old Title").unwrap();
        let mut activity = Activity::new(owner_id, title, ActivityType::Hike, None, None);

        let old_updated = activity.updated_at;
        // Small delay to ensure timestamp changes
        let new_title = ActivityTitle::new("New Title").unwrap();
        activity.update_title(new_title);

        assert_eq!(activity.title.as_str(), "New Title");
        assert!(activity.updated_at >= old_updated);
    }

    #[test]
    fn activity_type_display() {
        assert_eq!(ActivityType::Hike.to_string(), "hike");
        assert_eq!(ActivityType::Walk.to_string(), "walk");
        assert_eq!(ActivityType::Run.to_string(), "run");
        assert_eq!(ActivityType::Other.to_string(), "other");
    }

    #[test]
    fn activity_id_display() {
        let id = Uuid::new_v4();
        let activity_id = ActivityId::new(id);
        assert_eq!(activity_id.to_string(), id.to_string());
    }

    #[test]
    fn error_display() {
        let err = ActivityCatalogError::ActivityNotFound;
        assert_eq!(err.to_string(), "activity not found");

        let err = ActivityCatalogError::InvalidTitle {
            message: "too short".to_string(),
        };
        assert_eq!(err.to_string(), "invalid title: too short");

        let err = ActivityCatalogError::Unauthorized;
        assert_eq!(err.to_string(), "unauthorized");
    }
}
