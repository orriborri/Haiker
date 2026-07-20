//! PostgreSQL-backed implementation of ActivityRepository.
//!
//! Uses raw `sqlx::query()` (not compile-time checked) to avoid needing
//! database connectivity at build time (SQLX_OFFLINE=true compatible).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::activity_catalog::queries::{decode_cursor, encode_cursor, CursorPayload};
use haiker_app::activity_catalog::repository::{ActivityPage, ActivityRepository};
use haiker_app::activity_catalog::{
    Activity, ActivityCatalogError, ActivityId, ActivityTitle, ActivityType, LifecycleState,
};
use haiker_app::identity::UserId;
use haiker_app::route_versioning::RouteVersionId;

/// PostgreSQL implementation of the activity repository.
#[derive(Clone)]
pub struct PgActivityRepository {
    pool: PgPool,
}

impl PgActivityRepository {
    /// Create a new PgActivityRepository with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Type alias for the row tuple returned by activity queries.
type ActivityRow = (
    Uuid,                      // id
    Uuid,                      // owner_id
    String,                    // title
    String,                    // activity_type
    String,                    // lifecycle_state
    Option<DateTime<Utc>>,     // started_at
    Option<DateTime<Utc>>,     // ended_at
    Option<serde_json::Value>, // recorded_summary_json
    Option<serde_json::Value>, // corrected_summary_json
    Option<Uuid>,              // current_route_version_id
    DateTime<Utc>,             // created_at
    DateTime<Utc>,             // updated_at
);

/// Parse an activity type string from the database.
fn parse_activity_type(s: &str) -> ActivityType {
    match s {
        "hike" => ActivityType::Hike,
        "walk" => ActivityType::Walk,
        "run" => ActivityType::Run,
        _ => ActivityType::Other,
    }
}

/// Parse a lifecycle state string from the database.
fn parse_lifecycle_state(s: &str) -> LifecycleState {
    match s {
        "active" => LifecycleState::Active,
        "deleted" => LifecycleState::Deleted,
        _ => LifecycleState::Active, // Defensive fallback
    }
}

/// Map a row tuple to an Activity domain object.
fn row_to_activity(row: ActivityRow) -> Activity {
    let (
        id,
        owner_id,
        title,
        activity_type,
        lifecycle_state,
        started_at,
        ended_at,
        recorded_summary,
        corrected_summary,
        current_route_version_id,
        created_at,
        updated_at,
    ) = row;

    Activity {
        id: ActivityId::new(id),
        owner_id: UserId::new(owner_id),
        title: ActivityTitle::new(title)
            .unwrap_or_else(|_| ActivityTitle::new("Untitled").unwrap()),
        activity_type: parse_activity_type(&activity_type),
        lifecycle_state: parse_lifecycle_state(&lifecycle_state),
        started_at,
        ended_at,
        recorded_summary,
        corrected_summary,
        current_route_version_id: current_route_version_id.map(RouteVersionId::new),
        created_at,
        updated_at,
    }
}

#[async_trait]
impl ActivityRepository for PgActivityRepository {
    async fn list_activities(
        &self,
        owner_id: UserId,
        cursor: Option<&str>,
        page_size: u32,
    ) -> Result<ActivityPage, ActivityCatalogError> {
        // Fetch one extra row to determine has_more
        let limit = (page_size + 1) as i64;

        let rows = if let Some(cursor_str) = cursor {
            let cursor_payload = decode_cursor(cursor_str)?;

            let cursor_id: Uuid =
                cursor_payload
                    .id
                    .parse()
                    .map_err(|_| ActivityCatalogError::InvalidCursor {
                        message: "invalid id in cursor".to_string(),
                    })?;

            let cursor_started_at: Option<DateTime<Utc>> = cursor_payload
                .started_at
                .as_deref()
                .map(|s| {
                    s.parse::<DateTime<Utc>>()
                        .map_err(|_| ActivityCatalogError::InvalidCursor {
                            message: "invalid started_at in cursor".to_string(),
                        })
                })
                .transpose()?;

            match cursor_started_at {
                Some(started_at) => sqlx::query_as::<_, ActivityRow>(
                    r#"
                        SELECT id, owner_id, title, activity_type, lifecycle_state,
                               started_at, ended_at, recorded_summary_json,
                               corrected_summary_json, created_at, updated_at
                        FROM activity_catalog.activities
                        WHERE owner_id = $1
                          AND lifecycle_state != 'deleted'
                          AND (
                            (started_at < $2)
                            OR (started_at = $2 AND id < $3)
                            OR (started_at IS NULL)
                          )
                        ORDER BY started_at DESC NULLS LAST, id DESC
                        LIMIT $4
                        "#,
                )
                .bind(owner_id.0)
                .bind(started_at)
                .bind(cursor_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| ActivityCatalogError::PersistenceError {
                    message: e.to_string(),
                })?,
                None => {
                    // Activities with NULL started_at are sorted last;
                    // cursor with NULL started_at means we look for entries
                    // where started_at IS NULL AND id < cursor_id
                    sqlx::query_as::<_, ActivityRow>(
                        r#"
                        SELECT id, owner_id, title, activity_type, lifecycle_state,
                               started_at, ended_at, recorded_summary_json,
                               corrected_summary_json, created_at, updated_at
                        FROM activity_catalog.activities
                        WHERE owner_id = $1
                          AND lifecycle_state != 'deleted'
                          AND (started_at IS NULL AND id < $2)
                        ORDER BY started_at DESC NULLS LAST, id DESC
                        LIMIT $3
                        "#,
                    )
                    .bind(owner_id.0)
                    .bind(cursor_id)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|e| ActivityCatalogError::PersistenceError {
                        message: e.to_string(),
                    })?
                }
            }
        } else {
            sqlx::query_as::<_, ActivityRow>(
                r#"
                SELECT id, owner_id, title, activity_type, lifecycle_state,
                       started_at, ended_at, recorded_summary_json,
                       corrected_summary_json, created_at, updated_at
                FROM activity_catalog.activities
                WHERE owner_id = $1
                  AND lifecycle_state != 'deleted'
                ORDER BY started_at DESC NULLS LAST, id DESC
                LIMIT $2
                "#,
            )
            .bind(owner_id.0)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ActivityCatalogError::PersistenceError {
                message: e.to_string(),
            })?
        };

        let has_more = rows.len() as u32 > page_size;
        let items: Vec<Activity> = rows
            .into_iter()
            .take(page_size as usize)
            .map(row_to_activity)
            .collect();

        let next_cursor = if has_more {
            items.last().map(|last| {
                encode_cursor(&CursorPayload {
                    started_at: last.started_at.map(|ts| ts.to_rfc3339()),
                    id: last.id.0.to_string(),
                })
            })
        } else {
            None
        };

        Ok(ActivityPage {
            items,
            next_cursor,
            has_more,
        })
    }

    async fn find_by_id(&self, id: ActivityId) -> Result<Option<Activity>, ActivityCatalogError> {
        let row = sqlx::query_as::<_, ActivityRow>(
            r#"
            SELECT id, owner_id, title, activity_type, lifecycle_state,
                   started_at, ended_at, recorded_summary_json,
                   corrected_summary_json, created_at, updated_at
            FROM activity_catalog.activities
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ActivityCatalogError::PersistenceError {
            message: e.to_string(),
        })?;

        Ok(row.map(row_to_activity))
    }

    async fn save(&self, activity: &Activity) -> Result<(), ActivityCatalogError> {
        sqlx::query(
            r#"
            INSERT INTO activity_catalog.activities (
                id, owner_id, title, activity_type, lifecycle_state,
                started_at, ended_at, recorded_summary_json,
                corrected_summary_json, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(activity.id.0)
        .bind(activity.owner_id.0)
        .bind(activity.title.as_str())
        .bind(activity.activity_type.to_string())
        .bind(activity.lifecycle_state.to_string())
        .bind(activity.started_at)
        .bind(activity.ended_at)
        .bind(&activity.recorded_summary)
        .bind(&activity.corrected_summary)
        .bind(activity.created_at)
        .bind(activity.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| ActivityCatalogError::PersistenceError {
            message: e.to_string(),
        })?;

        Ok(())
    }

    async fn update(&self, activity: &Activity) -> Result<(), ActivityCatalogError> {
        sqlx::query(
            r#"
            UPDATE activity_catalog.activities
            SET title = $2,
                activity_type = $3,
                lifecycle_state = $4,
                started_at = $5,
                ended_at = $6,
                recorded_summary_json = $7,
                corrected_summary_json = $8,
                updated_at = $9
            WHERE id = $1
            "#,
        )
        .bind(activity.id.0)
        .bind(activity.title.as_str())
        .bind(activity.activity_type.to_string())
        .bind(activity.lifecycle_state.to_string())
        .bind(activity.started_at)
        .bind(activity.ended_at)
        .bind(&activity.recorded_summary)
        .bind(&activity.corrected_summary)
        .bind(activity.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| ActivityCatalogError::PersistenceError {
            message: e.to_string(),
        })?;

        Ok(())
    }

    async fn delete(&self, id: ActivityId) -> Result<(), ActivityCatalogError> {
        let now = Utc::now();
        sqlx::query(
            r#"
            UPDATE activity_catalog.activities
            SET lifecycle_state = 'deleted', updated_at = $2
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| ActivityCatalogError::PersistenceError {
            message: e.to_string(),
        })?;

        Ok(())
    }
}
