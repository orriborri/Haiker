//! Leg query handlers.
//!
//! Contains query logic for listing and fetching legs and their statistics.

use crate::activity_catalog::ActivityId;

use super::leg_repository::LegRepository;
use super::legs::{Leg, LegId, LegSummary};
use super::RecordedActivityError;

/// A detailed view of a single leg including its summary statistics.
#[derive(Debug, Clone)]
pub struct LegDetail {
    /// The leg aggregate.
    pub leg: Leg,
    /// Aggregated statistics for this leg (if available).
    pub summary: Option<LegSummary>,
}

/// Aggregated statistics for an entire activity across all legs.
#[derive(Debug, Clone)]
pub struct ActivityLegStats {
    /// Total statistics aggregated across all legs.
    pub total: LegSummary,
    /// Per-leg statistics indexed by leg number.
    pub per_leg: Vec<LegWithSummary>,
}

/// A leg paired with its summary statistics.
#[derive(Debug, Clone)]
pub struct LegWithSummary {
    /// The leg.
    pub leg: Leg,
    /// The summary for this leg (empty if no data available).
    pub summary: LegSummary,
}

/// List all legs for an activity, ordered by leg number.
pub async fn list_legs_for_activity(
    activity_id: ActivityId,
    repo: &dyn LegRepository,
) -> Result<Vec<Leg>, RecordedActivityError> {
    repo.list_legs(activity_id).await
}

/// Get detailed information about a single leg, including statistics.
pub async fn get_leg_detail(
    leg_id: LegId,
    repo: &dyn LegRepository,
) -> Result<LegDetail, RecordedActivityError> {
    let leg = repo
        .find_leg(leg_id)
        .await?
        .ok_or(RecordedActivityError::LegNotFound { leg_id })?;

    let summary = repo.get_leg_summary(leg_id).await?;

    Ok(LegDetail { leg, summary })
}

/// Get aggregated statistics for all legs of an activity.
///
/// Returns per-leg stats and the total aggregated across all legs.
/// Legs without route data contribute an empty summary.
pub async fn get_aggregated_stats(
    activity_id: ActivityId,
    repo: &dyn LegRepository,
) -> Result<ActivityLegStats, RecordedActivityError> {
    let legs = repo.list_legs(activity_id).await?;

    let mut per_leg = Vec::with_capacity(legs.len());
    let mut total = LegSummary::empty();

    for leg in legs {
        let summary = repo
            .get_leg_summary(leg.id)
            .await?
            .unwrap_or_else(LegSummary::empty);

        total = total.combine(&summary);
        per_leg.push(LegWithSummary { leg, summary });
    }

    Ok(ActivityLegStats { total, per_leg })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::NaiveDate;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    /// In-memory repository with configurable summaries for query tests.
    struct TestLegQueryRepo {
        legs: Mutex<HashMap<LegId, Leg>>,
        summaries: Mutex<HashMap<LegId, LegSummary>>,
    }

    impl TestLegQueryRepo {
        fn new() -> Self {
            Self {
                legs: Mutex::new(HashMap::new()),
                summaries: Mutex::new(HashMap::new()),
            }
        }

        fn add_leg(&self, leg: Leg) {
            self.legs.lock().unwrap().insert(leg.id, leg);
        }

        fn add_summary(&self, leg_id: LegId, summary: LegSummary) {
            self.summaries.lock().unwrap().insert(leg_id, summary);
        }
    }

    #[async_trait]
    impl LegRepository for TestLegQueryRepo {
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
            _activity_id: ActivityId,
            _leg_id: LegId,
            _new_position: u32,
        ) -> Result<(), RecordedActivityError> {
            Ok(())
        }

        async fn get_leg_summary(
            &self,
            leg_id: LegId,
        ) -> Result<Option<LegSummary>, RecordedActivityError> {
            Ok(self.summaries.lock().unwrap().get(&leg_id).cloned())
        }
    }

    fn make_leg(activity_id: ActivityId, leg_number: u32) -> Leg {
        let date = NaiveDate::from_ymd_opt(2024, 7, leg_number).unwrap();
        Leg::new(activity_id, leg_number, None, date, None, None).unwrap()
    }

    #[tokio::test]
    async fn list_legs_for_activity_returns_ordered() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegQueryRepo::new();

        let leg1 = make_leg(activity_id, 1);
        let leg2 = make_leg(activity_id, 2);
        let leg3 = make_leg(activity_id, 3);
        repo.add_leg(leg3.clone());
        repo.add_leg(leg1.clone());
        repo.add_leg(leg2.clone());

        let legs = list_legs_for_activity(activity_id, &repo).await.unwrap();
        assert_eq!(legs.len(), 3);
        assert_eq!(legs[0].leg_number, 1);
        assert_eq!(legs[1].leg_number, 2);
        assert_eq!(legs[2].leg_number, 3);
    }

    #[tokio::test]
    async fn list_legs_for_activity_empty() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegQueryRepo::new();

        let legs = list_legs_for_activity(activity_id, &repo).await.unwrap();
        assert!(legs.is_empty());
    }

    #[tokio::test]
    async fn list_legs_filters_by_activity() {
        let activity_a = ActivityId::new(Uuid::new_v4());
        let activity_b = ActivityId::new(Uuid::new_v4());
        let repo = TestLegQueryRepo::new();

        repo.add_leg(make_leg(activity_a, 1));
        repo.add_leg(make_leg(activity_a, 2));
        repo.add_leg(make_leg(activity_b, 1));

        let legs_a = list_legs_for_activity(activity_a, &repo).await.unwrap();
        assert_eq!(legs_a.len(), 2);

        let legs_b = list_legs_for_activity(activity_b, &repo).await.unwrap();
        assert_eq!(legs_b.len(), 1);
    }

    #[tokio::test]
    async fn get_leg_detail_with_summary() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegQueryRepo::new();

        let leg = make_leg(activity_id, 1);
        let leg_id = leg.id;
        repo.add_leg(leg);

        let summary = LegSummary::new(10000.0, Some(200.0), Some(100.0), 500, Some(3600.0));
        repo.add_summary(leg_id, summary);

        let detail = get_leg_detail(leg_id, &repo).await.unwrap();
        assert_eq!(detail.leg.id, leg_id);
        assert!(detail.summary.is_some());
        assert_eq!(detail.summary.unwrap().distance_meters, 10000.0);
    }

    #[tokio::test]
    async fn get_leg_detail_without_summary() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegQueryRepo::new();

        let leg = make_leg(activity_id, 1);
        let leg_id = leg.id;
        repo.add_leg(leg);

        let detail = get_leg_detail(leg_id, &repo).await.unwrap();
        assert_eq!(detail.leg.id, leg_id);
        assert!(detail.summary.is_none());
    }

    #[tokio::test]
    async fn get_leg_detail_not_found() {
        let repo = TestLegQueryRepo::new();
        let fake_id = LegId::generate();

        let result = get_leg_detail(fake_id, &repo).await;
        assert!(matches!(
            result.unwrap_err(),
            RecordedActivityError::LegNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn get_aggregated_stats_combines_summaries() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegQueryRepo::new();

        let leg1 = make_leg(activity_id, 1);
        let leg2 = make_leg(activity_id, 2);
        let leg1_id = leg1.id;
        let leg2_id = leg2.id;
        repo.add_leg(leg1);
        repo.add_leg(leg2);

        repo.add_summary(
            leg1_id,
            LegSummary::new(10000.0, Some(200.0), Some(100.0), 500, Some(3600.0)),
        );
        repo.add_summary(
            leg2_id,
            LegSummary::new(8000.0, Some(300.0), Some(150.0), 400, Some(2400.0)),
        );

        let stats = get_aggregated_stats(activity_id, &repo).await.unwrap();

        assert_eq!(stats.per_leg.len(), 2);
        assert_eq!(stats.total.distance_meters, 18000.0);
        assert_eq!(stats.total.elevation_gain_meters, Some(500.0));
        assert_eq!(stats.total.elevation_loss_meters, Some(250.0));
        assert_eq!(stats.total.point_count, 900);
        assert_eq!(stats.total.duration_seconds, Some(6000.0));
    }

    #[tokio::test]
    async fn get_aggregated_stats_empty_activity() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegQueryRepo::new();

        let stats = get_aggregated_stats(activity_id, &repo).await.unwrap();

        assert!(stats.per_leg.is_empty());
        assert_eq!(stats.total.distance_meters, 0.0);
        assert_eq!(stats.total.point_count, 0);
    }

    #[tokio::test]
    async fn get_aggregated_stats_legs_without_summaries() {
        let activity_id = ActivityId::new(Uuid::new_v4());
        let repo = TestLegQueryRepo::new();

        let leg1 = make_leg(activity_id, 1);
        let leg2 = make_leg(activity_id, 2);
        repo.add_leg(leg1);
        repo.add_leg(leg2);

        // No summaries added - legs have no route data yet
        let stats = get_aggregated_stats(activity_id, &repo).await.unwrap();

        assert_eq!(stats.per_leg.len(), 2);
        assert_eq!(stats.total.distance_meters, 0.0);
        assert_eq!(stats.total.point_count, 0);
        assert_eq!(stats.total.elevation_gain_meters, None);
    }
}
