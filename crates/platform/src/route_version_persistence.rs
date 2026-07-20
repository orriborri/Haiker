//! PostgreSQL-backed implementation of RouteVersionRepository.
//!
//! Uses raw `sqlx::query()` (not compile-time checked) to avoid needing
//! database connectivity at build time (SQLX_OFFLINE=true compatible).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::identity::UserId;
use haiker_app::recorded_activity::{BoundingBox, Coordinate};
use haiker_app::route_versioning::repository::{RouteVersionPage, RouteVersionRepository};
use haiker_app::route_versioning::{
    CorrectedStatistics, RouteVersion, RouteVersionId, RouteVersioningError,
};

/// PostgreSQL implementation of the route version repository.
#[derive(Clone)]
pub struct PgRouteVersionRepository {
    pool: PgPool,
}

impl PgRouteVersionRepository {
    /// Create a new PgRouteVersionRepository with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Row type returned by route version queries.
type RouteVersionRow = (
    Uuid,              // id
    Uuid,              // activity_id
    Option<Uuid>,      // parent_version_id
    i32,               // version_number
    serde_json::Value, // geometry_json
    serde_json::Value, // bounding_box_json
    serde_json::Value, // corrected_statistics_json
    String,            // calculation_version
    Option<String>,    // edit_summary
    Uuid,              // created_by
    DateTime<Utc>,     // created_at
    Option<String>,    // idempotency_key
);

/// Map a database row to a RouteVersion domain object.
fn row_to_route_version(row: RouteVersionRow) -> Result<RouteVersion, RouteVersioningError> {
    let (
        id,
        activity_id,
        parent_version_id,
        version_number,
        geometry_json,
        bounding_box_json,
        corrected_statistics_json,
        calculation_version,
        edit_summary,
        created_by,
        created_at,
        _idempotency_key,
    ) = row;

    let geometry: Vec<Coordinate> = serde_json::from_value(geometry_json).map_err(|e| {
        RouteVersioningError::PersistenceError {
            message: format!("failed to deserialize geometry: {e}"),
        }
    })?;

    let bounding_box: BoundingBox = serde_json::from_value(bounding_box_json).map_err(|e| {
        RouteVersioningError::PersistenceError {
            message: format!("failed to deserialize bounding_box: {e}"),
        }
    })?;

    let corrected_statistics: CorrectedStatistics =
        serde_json::from_value(corrected_statistics_json).map_err(|e| {
            RouteVersioningError::PersistenceError {
                message: format!("failed to deserialize corrected_statistics: {e}"),
            }
        })?;

    Ok(RouteVersion {
        id: RouteVersionId(id),
        activity_id: ActivityId(activity_id),
        parent_version_id: parent_version_id.map(RouteVersionId),
        version_number,
        geometry,
        bounding_box,
        corrected_statistics,
        calculation_version,
        edit_summary,
        created_by: UserId(created_by),
        created_at,
    })
}

#[async_trait]
impl RouteVersionRepository for PgRouteVersionRepository {
    async fn save(&self, version: &RouteVersion) -> Result<(), RouteVersioningError> {
        let geometry_json = serde_json::to_value(&version.geometry).map_err(|e| {
            RouteVersioningError::PersistenceError {
                message: format!("failed to serialize geometry: {e}"),
            }
        })?;

        let bounding_box_json = serde_json::to_value(version.bounding_box).map_err(|e| {
            RouteVersioningError::PersistenceError {
                message: format!("failed to serialize bounding_box: {e}"),
            }
        })?;

        let corrected_statistics_json = serde_json::to_value(&version.corrected_statistics)
            .map_err(|e| RouteVersioningError::PersistenceError {
                message: format!("failed to serialize corrected_statistics: {e}"),
            })?;

        sqlx::query(
            r#"
            INSERT INTO route_versioning.route_versions (
                id, activity_id, parent_version_id, version_number,
                geometry, bounding_box, corrected_statistics,
                calculation_version, edit_summary, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(version.id.0)
        .bind(version.activity_id.0)
        .bind(version.parent_version_id.map(|id| id.0))
        .bind(version.version_number)
        .bind(&geometry_json)
        .bind(&bounding_box_json)
        .bind(&corrected_statistics_json)
        .bind(&version.calculation_version)
        .bind(&version.edit_summary)
        .bind(version.created_by.0)
        .bind(version.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to save route version: {e}"),
        })?;

        Ok(())
    }

    async fn find_by_id(
        &self,
        id: RouteVersionId,
    ) -> Result<Option<RouteVersion>, RouteVersioningError> {
        let row: Option<RouteVersionRow> = sqlx::query_as(
            r#"
            SELECT id, activity_id, parent_version_id, version_number,
                   geometry, bounding_box, corrected_statistics,
                   calculation_version, edit_summary, created_by, created_at,
                   idempotency_key
            FROM route_versioning.route_versions
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to find route version: {e}"),
        })?;

        match row {
            Some(r) => Ok(Some(row_to_route_version(r)?)),
            None => Ok(None),
        }
    }

    async fn find_latest_by_activity(
        &self,
        activity_id: ActivityId,
    ) -> Result<Option<RouteVersion>, RouteVersioningError> {
        let row: Option<RouteVersionRow> = sqlx::query_as(
            r#"
            SELECT id, activity_id, parent_version_id, version_number,
                   geometry, bounding_box, corrected_statistics,
                   calculation_version, edit_summary, created_by, created_at,
                   idempotency_key
            FROM route_versioning.route_versions
            WHERE activity_id = $1
            ORDER BY version_number DESC
            LIMIT 1
            "#,
        )
        .bind(activity_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to find latest route version: {e}"),
        })?;

        match row {
            Some(r) => Ok(Some(row_to_route_version(r)?)),
            None => Ok(None),
        }
    }

    async fn find_by_idempotency_key(
        &self,
        key: &str,
    ) -> Result<Option<RouteVersion>, RouteVersioningError> {
        let row: Option<RouteVersionRow> = sqlx::query_as(
            r#"
            SELECT id, activity_id, parent_version_id, version_number,
                   geometry, bounding_box, corrected_statistics,
                   calculation_version, edit_summary, created_by, created_at,
                   idempotency_key
            FROM route_versioning.route_versions
            WHERE idempotency_key = $1
            "#,
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to find route version by idempotency key: {e}"),
        })?;

        match row {
            Some(r) => Ok(Some(row_to_route_version(r)?)),
            None => Ok(None),
        }
    }

    async fn list_by_activity(
        &self,
        activity_id: ActivityId,
        cursor: Option<&str>,
        page_size: u32,
    ) -> Result<RouteVersionPage, RouteVersioningError> {
        // Cursor is the version_number to start after (versions before this number)
        let cursor_version: Option<i32> = cursor
            .map(|c| {
                c.parse::<i32>()
                    .map_err(|_| RouteVersioningError::PersistenceError {
                        message: "invalid cursor format".to_string(),
                    })
            })
            .transpose()?;

        // Fetch page_size + 1 to detect if there are more results
        let limit = (page_size + 1) as i64;

        let rows: Vec<RouteVersionRow> = if let Some(cv) = cursor_version {
            sqlx::query_as(
                r#"
                SELECT id, activity_id, parent_version_id, version_number,
                       geometry, bounding_box, corrected_statistics,
                       calculation_version, edit_summary, created_by, created_at,
                       idempotency_key
                FROM route_versioning.route_versions
                WHERE activity_id = $1 AND version_number < $2
                ORDER BY version_number DESC
                LIMIT $3
                "#,
            )
            .bind(activity_id.0)
            .bind(cv)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as(
                r#"
                SELECT id, activity_id, parent_version_id, version_number,
                       geometry, bounding_box, corrected_statistics,
                       calculation_version, edit_summary, created_by, created_at,
                       idempotency_key
                FROM route_versioning.route_versions
                WHERE activity_id = $1
                ORDER BY version_number DESC
                LIMIT $2
                "#,
            )
            .bind(activity_id.0)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|e| RouteVersioningError::PersistenceError {
            message: format!("failed to list route versions: {e}"),
        })?;

        let has_more = rows.len() as u32 > page_size;
        let items_rows: Vec<RouteVersionRow> = rows.into_iter().take(page_size as usize).collect();

        let items: Vec<RouteVersion> = items_rows
            .into_iter()
            .map(row_to_route_version)
            .collect::<Result<Vec<_>, _>>()?;

        let next_cursor = if has_more {
            items.last().map(|v| v.version_number.to_string())
        } else {
            None
        };

        Ok(RouteVersionPage {
            items,
            next_cursor,
            has_more,
        })
    }
}
