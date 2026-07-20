//! PostgreSQL-backed implementation of LegRepository.
//!
//! Uses raw `sqlx::query()` (not compile-time checked) to avoid needing
//! database connectivity at build time (SQLX_OFFLINE=true compatible).

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

/// Row type returned by leg queries.
type LegRow = (
    Uuid,              // id
    Uuid,              // activity_id
    i32,               // leg_number
    Option<String>,    // title
    NaiveDate,         // date
    Option<Uuid>,      // source_revision_id
    Option<Uuid>,      // recorded_track_id
    DateTime<Utc>,     // created_at
    DateTime<Utc>,     // updated_at
);

/// Map a row tuple to a Leg domain object.
fn row_to_leg(row: LegRow) -> Leg {
    let (
        id,
        activity_id,
        leg_number,
        title,
        date,
        source_revision_id,
        recorded_track_id,
        created_at,
        updated_at,
    ) = row;

    let leg_title = title.and_then(|t| LegTitle::new(t).ok());

    Leg {
        id: LegId::new(id),
        activity_id: ActivityId::new(activity_id),
        leg_number: leg_number as u32,
        title: leg_title,
        date,
        source_revision_id: source_revision_id.map(SourceRevisionId::new),
        recorded_track_id: recorded_track_id.map(RecordedTrackId::new),
        created_at,
        updated_at,
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

        Ok(rows.into_iter().map(row_to_leg).collect())
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

        Ok(row.map(row_to_leg))
    }

    async fn save_leg(&self, leg: &Leg) -> Result<(), RecordedActivityError> {
        sqlx::query(
            r#"
            INSERT INTO recorded_activity.legs (
                id, activity_id, leg_number, title, date,
                source_revision_id, recorded_track_id, created_at, updated_at
            )
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
            SET leg_number = $2,
                title = $3,
                date = $4,
                source_revision_id = $5,
                recorded_track_id = $6,
                updated_at = $7
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
            DELETE FROM recorded_activity.legs
            WHERE id = $1
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
        let row: (Option<i32>,) = sqlx::query_as(
            r#"
            SELECT MAX(leg_number) as max_num
            FROM recorded_activity.legs
            WHERE activity_id = $1
            "#,
        )
        .bind(activity_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        Ok((row.0.unwrap_or(0) + 1) as u32)
    }

    async fn reorder_legs(
        &self,
        activity_id: ActivityId,
        leg_id: LegId,
        new_position: u32,
    ) -> Result<(), RecordedActivityError> {
        // Get the current leg number
        let current_row: Option<(i32,)> = sqlx::query_as(
            r#"
            SELECT leg_number
            FROM recorded_activity.legs
            WHERE id = $1 AND activity_id = $2
            "#,
        )
        .bind(leg_id.0)
        .bind(activity_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RecordedActivityError::Persistence {
            message: e.to_string(),
        })?;

        let current_position = current_row
            .ok_or(RecordedActivityError::LegNotFound { leg_id })?
            .0 as u32;

        if current_position == new_position {
            return Ok(());
        }

        let now = Utc::now();

        if new_position < current_position {
            // Moving up: shift legs in [new_position, current_position) down by +1
            sqlx::query(
                r#"
                UPDATE recorded_activity.legs
                SET leg_number = leg_number + 1, updated_at = $4
                WHERE activity_id = $1
                  AND leg_number >= $2
                  AND leg_number < $3
                "#,
            )
            .bind(activity_id.0)
            .bind(new_position as i32)
            .bind(current_position as i32)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| RecordedActivityError::Persistence {
                message: e.to_string(),
            })?;
        } else {
            // Moving down: shift legs in (current_position, new_position] up by -1
            sqlx::query(
                r#"
                UPDATE recorded_activity.legs
                SET leg_number = leg_number - 1, updated_at = $4
                WHERE activity_id = $1
                  AND leg_number > $2
                  AND leg_number <= $3
                "#,
            )
            .bind(activity_id.0)
            .bind(current_position as i32)
            .bind(new_position as i32)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| RecordedActivityError::Persistence {
                message: e.to_string(),
            })?;
        }

        // Set the moved leg to its new position
        sqlx::query(
            r#"
            UPDATE recorded_activity.legs
            SET leg_number = $2, updated_at = $3
            WHERE id = $1
            "#,
        )
        .bind(leg_id.0)
        .bind(new_position as i32)
        .bind(now)
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
        // Join with recorded tracks to get statistics for this leg.
        // Duration is calculated from started_at and ended_at timestamps.
        let row: Option<(f64, Option<f64>, Option<f64>, i32, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> = sqlx::query_as(
            r#"
            SELECT rt.distance_meters, rt.elevation_gain_meters, rt.elevation_loss_meters,
                   rt.point_count, rt.started_at, rt.ended_at
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

        Ok(row.map(|(distance, gain, loss, points, started_at, ended_at)| {
            let duration = match (started_at, ended_at) {
                (Some(start), Some(end)) => {
                    let secs = (end - start).num_seconds() as f64;
                    if secs > 0.0 { Some(secs) } else { None }
                }
                _ => None,
            };
            LegSummary::new(distance, gain, loss, points as u32, duration)
        }))
    }
}
