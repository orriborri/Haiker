//! PostgreSQL-backed implementation of LegRepository.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::recorded_activity::leg_repository::LegRepository;
use haiker_app::recorded_activity::legs::{Leg, LegId, LegSummary, LegTitle};
use haiker_app::recorded_activity::{RecordedActivityError, RecordedTrackId, SourceRevisionId};

/// PostgreSQL implementation of the leg repository.
#[derive(Clone)]
pub struct PgLegRepository {
    pool: PgPool,
}

impl PgLegRepository {
    /// Create a new PgLegRepository with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Row type returned from leg queries.
#[derive(sqlx::FromRow)]
struct LegRow {
    id: Uuid,
    activity_id: Uuid,
    leg_number: i32,
    title: Option<String>,
    date: NaiveDate,
    source_revision_id: Option<Uuid>,
    recorded_track_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl LegRow {
    fn into_leg(self) -> Leg {
        Leg {
            id: LegId::new(self.id),
            activity_id: ActivityId::new(self.activity_id),
            leg_number: self.leg_number as u32,
            title: self.title.and_then(|t| LegTitle::new(t).ok()),
            date: self.date,
            source_revision_id: self.source_revision_id.map(SourceRevisionId),
            recorded_track_id: self.recorded_track_id.map(RecordedTrackId),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[async_trait]
impl LegRepository for PgLegRepository {
    async fn list_legs(&self, activity_id: ActivityId) -> Result<Vec<Leg>, RecordedActivityError> {
        let rows = sqlx::query_as::<_, LegRow>(
            r#"
            SELECT id, activity_id, leg_number, title, date,
                   source_revision_id, recorded_track_id, created_at, updated_at
            FROM recorded_activity.legs
            WHERE activity_id = $1
            ORDER BY leg_number ASC
            "#,
        )
        .bind(activity_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok(rows.into_iter().map(LegRow::into_leg).collect())
    }

    async fn find_leg(&self, leg_id: LegId) -> Result<Option<Leg>, RecordedActivityError> {
        let row = sqlx::query_as::<_, LegRow>(
            r#"
            SELECT id, activity_id, leg_number, title, date,
                   source_revision_id, recorded_track_id, created_at, updated_at
            FROM recorded_activity.legs
            WHERE id = $1
            "#,
        )
        .bind(leg_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok(row.map(LegRow::into_leg))
    }

    async fn save_leg(&self, leg: &Leg) -> Result<(), RecordedActivityError> {
        sqlx::query(
            r#"
            INSERT INTO recorded_activity.legs
                (id, activity_id, leg_number, title, date,
                 source_revision_id, recorded_track_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(leg.id.0)
        .bind(leg.activity_id.0)
        .bind(leg.leg_number as i32)
        .bind(leg.title.as_ref().map(|t| t.as_str()))
        .bind(leg.date)
        .bind(leg.source_revision_id.map(|id| id.0))
        .bind(leg.recorded_track_id.map(|id| id.0))
        .bind(leg.created_at)
        .bind(leg.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok(())
    }

    async fn update_leg(&self, leg: &Leg) -> Result<(), RecordedActivityError> {
        sqlx::query(
            r#"
            UPDATE recorded_activity.legs
            SET leg_number = $2, title = $3, date = $4,
                source_revision_id = $5, recorded_track_id = $6, updated_at = $7
            WHERE id = $1
            "#,
        )
        .bind(leg.id.0)
        .bind(leg.leg_number as i32)
        .bind(leg.title.as_ref().map(|t| t.as_str()))
        .bind(leg.date)
        .bind(leg.source_revision_id.map(|id| id.0))
        .bind(leg.recorded_track_id.map(|id| id.0))
        .bind(leg.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok(())
    }

    async fn delete_leg(&self, leg_id: LegId) -> Result<(), RecordedActivityError> {
        sqlx::query(
            r#"
            DELETE FROM recorded_activity.legs WHERE id = $1
            "#,
        )
        .bind(leg_id.0)
        .execute(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok(())
    }

    async fn next_leg_number(
        &self,
        activity_id: ActivityId,
    ) -> Result<u32, RecordedActivityError> {
        let row = sqlx::query_as::<_, (Option<i32>,)>(
            r#"
            SELECT MAX(leg_number) FROM recorded_activity.legs
            WHERE activity_id = $1
            "#,
        )
        .bind(activity_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok(row.0.unwrap_or(0) as u32 + 1)
    }

    async fn reorder_legs(
        &self,
        activity_id: ActivityId,
        leg_id: LegId,
        new_position: u32,
    ) -> Result<(), RecordedActivityError> {
        // Get current leg number
        let current_leg = self
            .find_leg(leg_id)
            .await?
            .ok_or(RecordedActivityError::LegNotFound { leg_id })?;

        let old_pos = current_leg.leg_number as i32;
        let new_pos = new_position as i32;

        if old_pos == new_pos {
            return Ok(());
        }

        // Shift other legs to make room
        if new_pos < old_pos {
            // Moving up: shift legs between new_pos and old_pos down by 1
            sqlx::query(
                r#"
                UPDATE recorded_activity.legs
                SET leg_number = leg_number + 1, updated_at = now()
                WHERE activity_id = $1 AND leg_number >= $2 AND leg_number < $3 AND id != $4
                "#,
            )
            .bind(activity_id.0)
            .bind(new_pos)
            .bind(old_pos)
            .bind(leg_id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| RecordedActivityError::Persistence {
                message: e.to_string(),
            })?;
        } else {
            // Moving down: shift legs between old_pos and new_pos up by 1
            sqlx::query(
                r#"
                UPDATE recorded_activity.legs
                SET leg_number = leg_number - 1, updated_at = now()
                WHERE activity_id = $1 AND leg_number > $2 AND leg_number <= $3 AND id != $4
                "#,
            )
            .bind(activity_id.0)
            .bind(old_pos)
            .bind(new_pos)
            .bind(leg_id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| RecordedActivityError::Persistence {
                message: e.to_string(),
            })?;
        }

        // Set the leg to its new position
        sqlx::query(
            r#"
            UPDATE recorded_activity.legs
            SET leg_number = $2, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(leg_id.0)
        .bind(new_pos)
        .execute(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok(())
    }

    async fn get_leg_summary(
        &self,
        leg_id: LegId,
    ) -> Result<Option<LegSummary>, RecordedActivityError> {
        let row = sqlx::query_as::<_, (Option<f64>, Option<f64>, Option<f64>, Option<i32>)>(
            r#"
            SELECT rt.distance_meters, rt.elevation_gain_meters,
                   rt.elevation_loss_meters, rt.point_count
            FROM recorded_activity.legs l
            JOIN recorded_activity.recorded_tracks rt ON rt.id = l.recorded_track_id
            WHERE l.id = $1
            "#,
        )
        .bind(leg_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok(row.map(|(distance, gain, loss, points)| {
            LegSummary::new(
                distance.unwrap_or(0.0),
                gain,
                loss,
                points.unwrap_or(0) as u32,
                None, // duration not stored in recorded_tracks
            )
        }))
    }
}
