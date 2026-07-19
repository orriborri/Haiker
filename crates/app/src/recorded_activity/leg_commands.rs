//! Leg command handlers.
//!
//! Contains command logic for mutating legs within an activity. Follows the
//! vertical slice pattern: validate inputs, load aggregate, apply domain rules,
//! persist changes.

use chrono::NaiveDate;

use crate::activity_catalog::ActivityId;

use super::leg_repository::LegRepository;
use super::legs::{Leg, LegId, LegTitle, MAX_LEGS_PER_ACTIVITY};
use super::{RecordedActivityError, RecordedTrackId, SourceRevisionId};

/// Add a new leg to an activity.
///
/// Determines the next available leg number, validates constraints, creates
/// the leg, and persists it.
///
/// Returns the created leg on success.
pub async fn add_leg(
    activity_id: ActivityId,
    title: Option<&str>,
    date: NaiveDate,
    source_revision_id: Option<SourceRevisionId>,
    recorded_track_id: Option<RecordedTrackId>,
    repo: &dyn LegRepository,
) -> Result<Leg, RecordedActivityError> {
    // Determine next leg number
    let leg_number = repo.next_leg_number(activity_id).await?;

    // Enforce max legs constraint
    if leg_number > MAX_LEGS_PER_ACTIVITY {
        return Err(RecordedActivityError::MaxLegsExceeded {
            activity_id,
            max: MAX_LEGS_PER_ACTIVITY,
        });
    }

    // Validate optional title
    let leg_title = match title {
        Some(t) => Some(LegTitle::new(t)?),
        None => None,
    };

    // Create the leg
    let leg = Leg::new(
        activity_id,
        leg_number,
        leg_title,
        date,
        source_revision_id,
        recorded_track_id,
    )?;

    // Persist
    repo.save_leg(&leg).await?;

    Ok(leg)
}

/// Rename a leg's title.
///
/// Pass `None` to remove the title, or `Some(title_str)` to set a new one.
/// Validates the title if provided, loads the leg, applies the change, and
/// persists.
pub async fn rename_leg(
    leg_id: LegId,
    new_title: Option<&str>,
    repo: &dyn LegRepository,
) -> Result<Leg, RecordedActivityError> {
    // Validate title if provided
    let title = match new_title {
        Some(t) => Some(LegTitle::new(t)?),
        None => None,
    };

    // Load the leg
    let mut leg = repo
        .find_leg(leg_id)
        .await?
        .ok_or(RecordedActivityError::LegNotFound { leg_id })?;

    // Apply domain mutation
    leg.rename(title);

    // Persist
    repo.update_leg(&leg).await?;

    Ok(leg)
}

/// Update a leg's date.
///
/// Loads the leg, applies the date change, and persists.
pub async fn update_leg_date(
    leg_id: LegId,
    new_date: NaiveDate,
    repo: &dyn LegRepository,
) -> Result<Leg, RecordedActivityError> {
    // Load the leg
    let mut leg = repo
        .find_leg(leg_id)
        .await?
        .ok_or(RecordedActivityError::LegNotFound { leg_id })?;

    // Apply domain mutation
    leg.update_date(new_date);

    // Persist
    repo.update_leg(&leg).await?;

    Ok(leg)
}

/// Reorder a leg to a new position within its activity.
///
/// Delegates to the repository to shift other legs and update positions
/// atomically.
pub async fn reorder_leg(
    leg_id: LegId,
    new_position: u32,
    repo: &dyn LegRepository,
) -> Result<(), RecordedActivityError> {
    // Validate the new position
    if new_position < 1 {
        return Err(RecordedActivityError::InvalidLegNumber {
            leg_number: new_position,
            reason: "position must be at least 1".to_string(),
        });
    }

    if new_position > MAX_LEGS_PER_ACTIVITY {
        return Err(RecordedActivityError::MaxLegsExceeded {
            activity_id: ActivityId::new(uuid::Uuid::nil()),
            max: MAX_LEGS_PER_ACTIVITY,
        });
    }

    // Load the leg to get the activity_id
    let leg = repo
        .find_leg(leg_id)
        .await?
        .ok_or(RecordedActivityError::LegNotFound { leg_id })?;

    // Delegate reordering to repository (handles shifting atomically)
    repo.reorder_legs(leg.activity_id, leg_id, new_position)
        .await?;

    Ok(())
}

/// Remove a leg and renumber the remaining legs.
///
/// Deletes the specified leg and instructs the repository to compact the
/// remaining leg numbers to maintain a contiguous sequence.
pub async fn remove_leg(
    leg_id: LegId,
    repo: &dyn LegRepository,
) -> Result<(), RecordedActivityError> {
    // Load the leg to verify it exists
    let leg = repo
        .find_leg(leg_id)
        .await?
        .ok_or(RecordedActivityError::LegNotFound { leg_id })?;

    // Delete the leg
    repo.delete_leg(leg_id).await?;

    // Reload remaining legs and renumber them
    let mut remaining_legs = repo.list_legs(leg.activity_id).await?;
    remaining_legs.sort_by_key(|l| l.leg_number);

    for (idx, mut remaining_leg) in remaining_legs.into_iter().enumerate() {
        let expected_number = (idx as u32) + 1;
        if remaining_leg.leg_number != expected_number {
            remaining_leg.update_leg_number(expected_number)?;
            repo.update_leg(&remaining_leg).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    use super::super::legs::LegSummary;

    /// Simple in-memory repository for command tests.
    struct TestLegRepo {
        legs: Mutex<HashMap<LegId, Leg>>,
    }

    impl TestLegRepo {
        fn new() -> Self {
            Self {
                legs: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl LegRepository for TestLegRepo {
        async fn list_legs(
            &self,
            activity_id: ActivityId,
        ) -> Result<Vec<Leg>, RecordedActivityError> {
            let legs = self.legs.lock().unwrap();
            let mut result: Vec<Leg> = legs
                .values()
                .filter(|l| l.activity_id == activity_id)
                .cloned()
                .collect();
            result.sort_by_key(|l| l.leg_number);
            Ok(result)
        }

        async fn find_leg(&self, leg_id: LegId) -> Result<Option<Leg>, RecordedActivityError> {
            Ok(self.legs.lock().unwrap().get(&leg_id).cloned())
        }

        async fn save_leg(&self, leg: &Leg) -> Result<(), RecordedActivityError> {
            self.legs.lock().unwrap().insert(leg.id, leg.clone());
            Ok(())
        }

        async fn update_leg(&self, leg: &Leg) -> Result<(), RecordedActivityError> {
            self.legs.lock().unwrap().insert(leg.id, leg.clone());
            Ok(())
        }

        async fn delete_leg(&self, leg_id: LegId) -> Result<(), RecordedActivityError> {
            self.legs.lock().unwrap().remove(&leg_id);
            Ok(())
        }

        async fn next_leg_number(
            &self,
            activity_id: ActivityId,
        ) -> Result<u32, RecordedActivityError> {
            let legs = self.legs.lock().unwrap();
            let max = legs
                .values()
                .filter(|l| l.activity_id == activity_id)
                .map(|l| l.leg_number)
                .max()
                .unwrap_or(0);
            Ok(max + 1)
        }

        async fn reorder_legs(
            &self,
            activity_id: ActivityId,
            leg_id: LegId,
            new_position: u32,
        ) -> Result<(), RecordedActivityError> {
            let mut legs = self.legs.lock().unwrap();
            let mut activity_legs: Vec<Leg> = legs
                .values()
                .filter(|l| l.activity_id == activity_id)
                .cloned()
                .collect();
            activity_legs.sort_by_key(|l| l.leg_number);

            // Remove the leg being moved
            let idx = activity_legs
                .iter()
                .position(|l| l.id == leg_id)
                .ok_or(RecordedActivityError::LegNotFound { leg_id })?;
            let moving_leg = activity_legs.remove(idx);

            // Insert at new position (0-indexed from 1-based position)
            let insert_idx = ((new_position as usize) - 1).min(activity_legs.len());
            activity_legs.insert(insert_idx, moving_leg);

            // Renumber all legs
            for (i, leg) in activity_legs.iter_mut().enumerate() {
                leg.leg_number = (i as u32) + 1;
                legs.insert(leg.id, leg.clone());
            }

            Ok(())
        }

        async fn get_leg_summary(
            &self,
            _leg_id: LegId,
        ) -> Result<Option<LegSummary>, RecordedActivityError> {
            Ok(None)
        }
    }

    fn sample_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 7, 15).unwrap()
    }

    #[tokio::test]
    async fn add_leg_creates_first_leg() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let leg = add_leg(activity_id, Some("Day 1"), sample_date(), None, None, &repo)
            .await
            .unwrap();

        assert_eq!(leg.leg_number, 1);
        assert_eq!(leg.title.as_ref().unwrap().as_str(), "Day 1");
        assert_eq!(leg.activity_id, activity_id);
    }

    #[tokio::test]
    async fn add_leg_increments_leg_number() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        add_leg(activity_id, Some("Day 1"), sample_date(), None, None, &repo)
            .await
            .unwrap();

        let leg2 = add_leg(activity_id, Some("Day 2"), sample_date(), None, None, &repo)
            .await
            .unwrap();

        assert_eq!(leg2.leg_number, 2);
    }

    #[tokio::test]
    async fn add_leg_without_title() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let leg = add_leg(activity_id, None, sample_date(), None, None, &repo)
            .await
            .unwrap();

        assert!(leg.title.is_none());
    }

    #[tokio::test]
    async fn add_leg_rejects_empty_title() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let result = add_leg(activity_id, Some(""), sample_date(), None, None, &repo).await;

        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidLegTitle { .. }
        ));
    }

    #[tokio::test]
    async fn add_leg_enforces_max_legs() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        // Add max legs
        for i in 1..=MAX_LEGS_PER_ACTIVITY {
            let date = NaiveDate::from_ymd_opt(2024, 7, i as u32).unwrap();
            add_leg(activity_id, None, date, None, None, &repo)
                .await
                .unwrap();
        }

        // Attempt to add one more
        let result = add_leg(activity_id, None, sample_date(), None, None, &repo).await;
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::MaxLegsExceeded { max: 30, .. }
        ));
    }

    #[tokio::test]
    async fn rename_leg_sets_new_title() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let leg = add_leg(
            activity_id,
            Some("Original"),
            sample_date(),
            None,
            None,
            &repo,
        )
        .await
        .unwrap();

        let updated = rename_leg(leg.id, Some("Renamed"), &repo).await.unwrap();
        assert_eq!(updated.title.as_ref().unwrap().as_str(), "Renamed");
    }

    #[tokio::test]
    async fn rename_leg_clears_title() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let leg = add_leg(
            activity_id,
            Some("Has Title"),
            sample_date(),
            None,
            None,
            &repo,
        )
        .await
        .unwrap();

        let updated = rename_leg(leg.id, None, &repo).await.unwrap();
        assert!(updated.title.is_none());
    }

    #[tokio::test]
    async fn rename_leg_not_found() {
        let repo = TestLegRepo::new();
        let fake_id = LegId::generate();

        let result = rename_leg(fake_id, Some("Title"), &repo).await;
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::LegNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn update_leg_date_succeeds() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let leg = add_leg(activity_id, None, sample_date(), None, None, &repo)
            .await
            .unwrap();

        let new_date = NaiveDate::from_ymd_opt(2024, 8, 20).unwrap();
        let updated = update_leg_date(leg.id, new_date, &repo).await.unwrap();

        assert_eq!(updated.date, new_date);
    }

    #[tokio::test]
    async fn update_leg_date_not_found() {
        let repo = TestLegRepo::new();
        let fake_id = LegId::generate();
        let date = sample_date();

        let result = update_leg_date(fake_id, date, &repo).await;
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::LegNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn reorder_leg_moves_to_new_position() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let leg1 = add_leg(activity_id, Some("Day 1"), sample_date(), None, None, &repo)
            .await
            .unwrap();
        let _leg2 = add_leg(activity_id, Some("Day 2"), sample_date(), None, None, &repo)
            .await
            .unwrap();
        let leg3 = add_leg(activity_id, Some("Day 3"), sample_date(), None, None, &repo)
            .await
            .unwrap();

        // Move leg3 to position 1
        reorder_leg(leg3.id, 1, &repo).await.unwrap();

        let legs = repo.list_legs(activity_id).await.unwrap();
        assert_eq!(legs[0].id, leg3.id);
        assert_eq!(legs[0].leg_number, 1);
        assert_eq!(legs[1].id, leg1.id);
        assert_eq!(legs[1].leg_number, 2);
    }

    #[tokio::test]
    async fn reorder_leg_rejects_zero_position() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let leg = add_leg(activity_id, None, sample_date(), None, None, &repo)
            .await
            .unwrap();

        let result = reorder_leg(leg.id, 0, &repo).await;
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::InvalidLegNumber { leg_number: 0, .. }
        ));
    }

    #[tokio::test]
    async fn reorder_leg_not_found() {
        let repo = TestLegRepo::new();
        let fake_id = LegId::generate();

        let result = reorder_leg(fake_id, 1, &repo).await;
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::LegNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn remove_leg_deletes_and_renumbers() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegRepo::new();

        let _leg1 = add_leg(activity_id, Some("Day 1"), sample_date(), None, None, &repo)
            .await
            .unwrap();
        let leg2 = add_leg(activity_id, Some("Day 2"), sample_date(), None, None, &repo)
            .await
            .unwrap();
        let leg3 = add_leg(activity_id, Some("Day 3"), sample_date(), None, None, &repo)
            .await
            .unwrap();

        // Remove the middle leg
        remove_leg(leg2.id, &repo).await.unwrap();

        let legs = repo.list_legs(activity_id).await.unwrap();
        assert_eq!(legs.len(), 2);
        assert_eq!(legs[0].leg_number, 1);
        assert_eq!(legs[0].title.as_ref().unwrap().as_str(), "Day 1");
        assert_eq!(legs[1].id, leg3.id);
        assert_eq!(legs[1].leg_number, 2);
    }

    #[tokio::test]
    async fn remove_leg_not_found() {
        let repo = TestLegRepo::new();
        let fake_id = LegId::generate();

        let result = remove_leg(fake_id, &repo).await;
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::LegNotFound { .. }
        ));
    }
}
