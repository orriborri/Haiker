//! Leg domain model for the recorded activity bounded context.
//!
//! A leg represents a distinct section of a multi-day or multi-segment activity.
//! Each activity can have multiple legs ordered by `leg_number`. Legs carry
//! optional metadata like title, date, source revision, and recorded track
//! references.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::activity_catalog::ActivityId;

use super::{RecordedActivityError, RecordedTrackId, SourceRevisionId};

/// Maximum number of legs allowed per activity.
pub const MAX_LEGS_PER_ACTIVITY: u32 = 30;

/// Maximum allowed length for a leg title.
const MAX_LEG_TITLE_LENGTH: usize = 255;

/// A strongly-typed leg identifier wrapping a UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LegId(pub Uuid);

impl LegId {
    /// Create a new `LegId` from an existing UUID.
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }

    /// Generate a new random `LegId`.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for LegId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A validated leg title (optional, max 255 chars, trimmed).
///
/// Unlike `ActivityTitle`, a leg title is optional at the aggregate level.
/// This value object validates the content when a title is provided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegTitle(String);

impl LegTitle {
    /// Create a new `LegTitle`, validating that it is non-empty after trimming
    /// and does not exceed 255 characters.
    ///
    /// Returns an error if the title is empty after trimming or exceeds the
    /// maximum length.
    pub fn new(value: impl Into<String>) -> Result<Self, RecordedActivityError> {
        let value = value.into();
        let trimmed = value.trim().to_string();

        if trimmed.is_empty() {
            return Err(RecordedActivityError::InvalidLegTitle {
                message: "leg title must not be empty".to_string(),
            });
        }

        if trimmed.len() > MAX_LEG_TITLE_LENGTH {
            return Err(RecordedActivityError::InvalidLegTitle {
                message: format!(
                    "leg title must not exceed {MAX_LEG_TITLE_LENGTH} characters, got {}",
                    trimmed.len()
                ),
            });
        }

        Ok(Self(trimmed))
    }

    /// Returns the title value as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LegTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The Leg aggregate representing a single section of a multi-leg activity.
///
/// Legs are ordered by `leg_number` within an activity. Each leg records the
/// date of that section and optionally references a source revision and
/// recorded track.
#[derive(Debug, Clone)]
pub struct Leg {
    /// Unique identifier for this leg.
    pub id: LegId,
    /// The activity this leg belongs to.
    pub activity_id: ActivityId,
    /// Position of this leg within the activity (1-based).
    pub leg_number: u32,
    /// Optional human-readable title for the leg.
    pub title: Option<LegTitle>,
    /// The date of this leg (without timezone).
    pub date: NaiveDate,
    /// Optional reference to the source revision that produced this leg.
    pub source_revision_id: Option<SourceRevisionId>,
    /// Optional reference to the recorded track for this leg.
    pub recorded_track_id: Option<RecordedTrackId>,
    /// When this leg was created.
    pub created_at: DateTime<Utc>,
    /// When this leg was last updated.
    pub updated_at: DateTime<Utc>,
}

impl Leg {
    /// Create a new Leg with the given parameters.
    ///
    /// Validates that `leg_number` is at least 1 and does not exceed the
    /// maximum legs per activity.
    pub fn new(
        activity_id: ActivityId,
        leg_number: u32,
        title: Option<LegTitle>,
        date: NaiveDate,
        source_revision_id: Option<SourceRevisionId>,
        recorded_track_id: Option<RecordedTrackId>,
    ) -> Result<Self, RecordedActivityError> {
        if leg_number < 1 {
            return Err(RecordedActivityError::InvalidLegNumber {
                leg_number,
                reason: "leg number must be at least 1".to_string(),
            });
        }

        if leg_number > MAX_LEGS_PER_ACTIVITY {
            return Err(RecordedActivityError::MaxLegsExceeded {
                activity_id,
                max: MAX_LEGS_PER_ACTIVITY,
            });
        }

        let now = Utc::now();
        Ok(Self {
            id: LegId::generate(),
            activity_id,
            leg_number,
            title,
            date,
            source_revision_id,
            recorded_track_id,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rename this leg's title.
    ///
    /// Pass `None` to remove the title, or `Some(LegTitle)` to set a new one.
    pub fn rename(&mut self, title: Option<LegTitle>) {
        self.title = title;
        self.updated_at = Utc::now();
    }

    /// Update the date of this leg.
    pub fn update_date(&mut self, date: NaiveDate) {
        self.date = date;
        self.updated_at = Utc::now();
    }

    /// Update the leg number (used during reordering).
    ///
    /// Validates that the new number is within bounds.
    pub fn update_leg_number(&mut self, leg_number: u32) -> Result<(), RecordedActivityError> {
        if leg_number < 1 {
            return Err(RecordedActivityError::InvalidLegNumber {
                leg_number,
                reason: "leg number must be at least 1".to_string(),
            });
        }

        if leg_number > MAX_LEGS_PER_ACTIVITY {
            return Err(RecordedActivityError::MaxLegsExceeded {
                activity_id: self.activity_id,
                max: MAX_LEGS_PER_ACTIVITY,
            });
        }

        self.leg_number = leg_number;
        self.updated_at = Utc::now();
        Ok(())
    }
}

/// Aggregated statistics for a leg.
///
/// Contains computed metrics summarizing the route data for a single leg.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LegSummary {
    /// Total distance in meters.
    pub distance_meters: f64,
    /// Total elevation gained in meters.
    pub elevation_gain_meters: Option<f64>,
    /// Total elevation lost in meters.
    pub elevation_loss_meters: Option<f64>,
    /// Number of GPS points in this leg.
    pub point_count: u32,
    /// Total duration in seconds.
    pub duration_seconds: Option<f64>,
}

impl LegSummary {
    /// Create a new `LegSummary` with the given values.
    pub fn new(
        distance_meters: f64,
        elevation_gain_meters: Option<f64>,
        elevation_loss_meters: Option<f64>,
        point_count: u32,
        duration_seconds: Option<f64>,
    ) -> Self {
        Self {
            distance_meters,
            elevation_gain_meters,
            elevation_loss_meters,
            point_count,
            duration_seconds,
        }
    }

    /// Create an empty summary with zeroed values.
    pub fn empty() -> Self {
        Self {
            distance_meters: 0.0,
            elevation_gain_meters: None,
            elevation_loss_meters: None,
            point_count: 0,
            duration_seconds: None,
        }
    }

    /// Combine two summaries by summing their values.
    pub fn combine(&self, other: &LegSummary) -> LegSummary {
        LegSummary {
            distance_meters: self.distance_meters + other.distance_meters,
            elevation_gain_meters: combine_optional(
                self.elevation_gain_meters,
                other.elevation_gain_meters,
            ),
            elevation_loss_meters: combine_optional(
                self.elevation_loss_meters,
                other.elevation_loss_meters,
            ),
            point_count: self.point_count + other.point_count,
            duration_seconds: combine_optional(self.duration_seconds, other.duration_seconds),
        }
    }
}

/// Combine two optional f64 values by summing them.
/// Returns None only if both are None; otherwise sums available values.
fn combine_optional(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use uuid::Uuid;

    fn sample_activity_id() -> ActivityId {
        ActivityId::new(Uuid::new_v4())
    }

    fn sample_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 7, 15).unwrap()
    }

    // --- LegId tests ---

    #[test]
    fn leg_id_display() {
        let id = Uuid::new_v4();
        let leg_id = LegId::new(id);
        assert_eq!(leg_id.to_string(), id.to_string());
    }

    #[test]
    fn leg_id_generate_is_unique() {
        let id1 = LegId::generate();
        let id2 = LegId::generate();
        assert_ne!(id1, id2);
    }

    // --- LegTitle tests ---

    #[test]
    fn leg_title_valid() {
        let title = LegTitle::new("Day 1: Valley to Summit").unwrap();
        assert_eq!(title.as_str(), "Day 1: Valley to Summit");
    }

    #[test]
    fn leg_title_trims_whitespace() {
        let title = LegTitle::new("  Day 1  ").unwrap();
        assert_eq!(title.as_str(), "Day 1");
    }

    #[test]
    fn leg_title_rejects_empty() {
        let result = LegTitle::new("");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidLegTitle { .. }
        ));
    }

    #[test]
    fn leg_title_rejects_whitespace_only() {
        let result = LegTitle::new("   \t  ");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidLegTitle { .. }
        ));
    }

    #[test]
    fn leg_title_rejects_over_255_chars() {
        let long_title = "a".repeat(256);
        let result = LegTitle::new(long_title);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidLegTitle { .. }
        ));
    }

    #[test]
    fn leg_title_exactly_255_chars_is_ok() {
        let exact_title = "b".repeat(255);
        let title = LegTitle::new(exact_title.clone()).unwrap();
        assert_eq!(title.as_str(), exact_title);
    }

    #[test]
    fn leg_title_display() {
        let title = LegTitle::new("Mountain Pass").unwrap();
        assert_eq!(title.to_string(), "Mountain Pass");
    }

    // --- Leg creation tests ---

    #[test]
    fn leg_creation_with_all_fields() {
        let activity_id = sample_activity_id();
        let title = LegTitle::new("Day 1").unwrap();
        let date = sample_date();
        let source_rev = SourceRevisionId::generate();
        let track_id = RecordedTrackId::generate();

        let leg = Leg::new(
            activity_id,
            1,
            Some(title),
            date,
            Some(source_rev),
            Some(track_id),
        )
        .unwrap();

        assert_eq!(leg.activity_id, activity_id);
        assert_eq!(leg.leg_number, 1);
        assert_eq!(leg.title.as_ref().unwrap().as_str(), "Day 1");
        assert_eq!(leg.date, date);
        assert_eq!(leg.source_revision_id, Some(source_rev));
        assert_eq!(leg.recorded_track_id, Some(track_id));
    }

    #[test]
    fn leg_creation_without_optional_fields() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let leg = Leg::new(activity_id, 1, None, date, None, None).unwrap();

        assert!(leg.title.is_none());
        assert!(leg.source_revision_id.is_none());
        assert!(leg.recorded_track_id.is_none());
    }

    #[test]
    fn leg_creation_rejects_zero_leg_number() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let result = Leg::new(activity_id, 0, None, date, None, None);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidLegNumber { leg_number: 0, .. }
        ));
    }

    #[test]
    fn leg_creation_rejects_exceeding_max() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let result = Leg::new(
            activity_id,
            MAX_LEGS_PER_ACTIVITY + 1,
            None,
            date,
            None,
            None,
        );
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::MaxLegsExceeded { max: 30, .. }
        ));
    }

    #[test]
    fn leg_creation_at_max_is_ok() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let leg = Leg::new(activity_id, MAX_LEGS_PER_ACTIVITY, None, date, None, None).unwrap();
        assert_eq!(leg.leg_number, MAX_LEGS_PER_ACTIVITY);
    }

    // --- Leg mutation tests ---

    #[test]
    fn leg_rename_sets_title() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let mut leg = Leg::new(activity_id, 1, None, date, None, None).unwrap();
        let old_updated = leg.updated_at;

        let new_title = LegTitle::new("Renamed Leg").unwrap();
        leg.rename(Some(new_title));

        assert_eq!(leg.title.as_ref().unwrap().as_str(), "Renamed Leg");
        assert!(leg.updated_at >= old_updated);
    }

    #[test]
    fn leg_rename_to_none_clears_title() {
        let activity_id = sample_activity_id();
        let title = LegTitle::new("Has Title").unwrap();
        let date = sample_date();

        let mut leg = Leg::new(activity_id, 1, Some(title), date, None, None).unwrap();
        leg.rename(None);

        assert!(leg.title.is_none());
    }

    #[test]
    fn leg_update_date() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let mut leg = Leg::new(activity_id, 1, None, date, None, None).unwrap();
        let new_date = NaiveDate::from_ymd_opt(2024, 8, 20).unwrap();
        let old_updated = leg.updated_at;

        leg.update_date(new_date);

        assert_eq!(leg.date, new_date);
        assert!(leg.updated_at >= old_updated);
    }

    #[test]
    fn leg_update_leg_number_valid() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let mut leg = Leg::new(activity_id, 1, None, date, None, None).unwrap();
        leg.update_leg_number(5).unwrap();
        assert_eq!(leg.leg_number, 5);
    }

    #[test]
    fn leg_update_leg_number_rejects_zero() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let mut leg = Leg::new(activity_id, 1, None, date, None, None).unwrap();
        let result = leg.update_leg_number(0);
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidLegNumber { leg_number: 0, .. }
        ));
    }

    #[test]
    fn leg_update_leg_number_rejects_exceeding_max() {
        let activity_id = sample_activity_id();
        let date = sample_date();

        let mut leg = Leg::new(activity_id, 1, None, date, None, None).unwrap();
        let result = leg.update_leg_number(MAX_LEGS_PER_ACTIVITY + 1);
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::MaxLegsExceeded { .. }
        ));
    }

    // --- LegSummary tests ---

    #[test]
    fn leg_summary_creation() {
        let summary = LegSummary::new(15000.0, Some(500.0), Some(300.0), 1200, Some(7200.0));
        assert_eq!(summary.distance_meters, 15000.0);
        assert_eq!(summary.elevation_gain_meters, Some(500.0));
        assert_eq!(summary.elevation_loss_meters, Some(300.0));
        assert_eq!(summary.point_count, 1200);
        assert_eq!(summary.duration_seconds, Some(7200.0));
    }

    #[test]
    fn leg_summary_empty() {
        let summary = LegSummary::empty();
        assert_eq!(summary.distance_meters, 0.0);
        assert_eq!(summary.elevation_gain_meters, None);
        assert_eq!(summary.elevation_loss_meters, None);
        assert_eq!(summary.point_count, 0);
        assert_eq!(summary.duration_seconds, None);
    }

    #[test]
    fn leg_summary_combine_both_present() {
        let s1 = LegSummary::new(10000.0, Some(200.0), Some(100.0), 500, Some(3600.0));
        let s2 = LegSummary::new(8000.0, Some(300.0), Some(150.0), 400, Some(2400.0));

        let combined = s1.combine(&s2);

        assert_eq!(combined.distance_meters, 18000.0);
        assert_eq!(combined.elevation_gain_meters, Some(500.0));
        assert_eq!(combined.elevation_loss_meters, Some(250.0));
        assert_eq!(combined.point_count, 900);
        assert_eq!(combined.duration_seconds, Some(6000.0));
    }

    #[test]
    fn leg_summary_combine_with_empty() {
        let s1 = LegSummary::new(10000.0, Some(200.0), Some(100.0), 500, Some(3600.0));
        let s2 = LegSummary::empty();

        let combined = s1.combine(&s2);

        assert_eq!(combined.distance_meters, 10000.0);
        assert_eq!(combined.elevation_gain_meters, Some(200.0));
        assert_eq!(combined.elevation_loss_meters, Some(100.0));
        assert_eq!(combined.point_count, 500);
        assert_eq!(combined.duration_seconds, Some(3600.0));
    }

    #[test]
    fn leg_summary_combine_none_with_none() {
        let s1 = LegSummary::new(5000.0, None, None, 200, None);
        let s2 = LegSummary::new(3000.0, None, None, 150, None);

        let combined = s1.combine(&s2);

        assert_eq!(combined.distance_meters, 8000.0);
        assert_eq!(combined.elevation_gain_meters, None);
        assert_eq!(combined.elevation_loss_meters, None);
        assert_eq!(combined.point_count, 350);
        assert_eq!(combined.duration_seconds, None);
    }

    // --- Error display tests ---

    #[test]
    fn error_invalid_leg_number_display() {
        let err = RecordedActivityError::InvalidLegNumber {
            leg_number: 0,
            reason: "leg number must be at least 1".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "invalid leg number 0: leg number must be at least 1"
        );
    }

    #[test]
    fn error_leg_not_found_display() {
        let leg_id = LegId::generate();
        let err = RecordedActivityError::LegNotFound { leg_id };
        assert_eq!(err.to_string(), format!("leg not found: {leg_id}"));
    }

    #[test]
    fn error_max_legs_exceeded_display() {
        let activity_id = sample_activity_id();
        let err = RecordedActivityError::MaxLegsExceeded {
            activity_id,
            max: 30,
        };
        assert_eq!(
            err.to_string(),
            format!("maximum legs exceeded for activity {activity_id}: limit is 30")
        );
    }

    #[test]
    fn error_duplicate_leg_number_display() {
        let err = RecordedActivityError::DuplicateLegNumber { leg_number: 3 };
        assert_eq!(err.to_string(), "duplicate leg number: 3");
    }

    #[test]
    fn error_invalid_leg_title_display() {
        let err = RecordedActivityError::InvalidLegTitle {
            message: "too long".to_string(),
        };
        assert_eq!(err.to_string(), "invalid leg title: too long");
    }
}
