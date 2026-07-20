//! Leg repository trait.
//!
//! Defines the persistence interface for leg aggregates. Implementations
//! live in the platform/persistence layer.

use async_trait::async_trait;

use crate::activity_catalog::ActivityId;

use super::legs::{Leg, LegId, LegSummary};
use super::RecordedActivityError;

/// Repository trait for leg persistence.
///
/// Domain code programs against this trait; the actual persistence implementation
/// is provided by the infrastructure layer.
#[async_trait]
pub trait LegRepository: Send + Sync {
    /// List all legs for a given activity, ordered by leg_number ascending.
    async fn list_legs(&self, activity_id: ActivityId) -> Result<Vec<Leg>, RecordedActivityError>;

    /// Find a leg by its ID.
    async fn find_leg(&self, leg_id: LegId) -> Result<Option<Leg>, RecordedActivityError>;

    /// Save a new leg.
    async fn save_leg(&self, leg: &Leg) -> Result<(), RecordedActivityError>;

    /// Update an existing leg.
    async fn update_leg(&self, leg: &Leg) -> Result<(), RecordedActivityError>;

    /// Delete a leg by its ID.
    async fn delete_leg(&self, leg_id: LegId) -> Result<(), RecordedActivityError>;

    /// Get the next available leg number for an activity.
    ///
    /// Returns the next sequential number (max existing leg_number + 1, or 1
    /// if no legs exist).
    async fn next_leg_number(&self, activity_id: ActivityId) -> Result<u32, RecordedActivityError>;

    /// Reorder legs by moving a specific leg to a new position.
    ///
    /// Shifts other leg numbers accordingly to maintain a contiguous sequence.
    async fn reorder_legs(
        &self,
        activity_id: ActivityId,
        leg_id: LegId,
        new_position: u32,
    ) -> Result<(), RecordedActivityError>;

    /// Get the aggregated summary statistics for a leg.
    async fn get_leg_summary(
        &self,
        leg_id: LegId,
    ) -> Result<Option<LegSummary>, RecordedActivityError>;
}
