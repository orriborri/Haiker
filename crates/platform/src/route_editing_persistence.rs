//! PostgreSQL-backed implementation of RouteDraftRepository.
//!
//! Uses raw `sqlx::query()` (not compile-time checked) to avoid needing
//! database connectivity at build time (SQLX_OFFLINE=true compatible).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::types::JsonValue;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use haiker_app::activity_catalog::ActivityId;
use haiker_app::identity::UserId;
use haiker_app::route_editing::{
    DraftState, OperationEntry, OperationId, RouteDraft, RouteDraftId, RouteDraftRepository,
    RouteEditingError, RouteOperation, RoutePoint,
};

/// PostgreSQL implementation of the route draft repository.
#[derive(Clone)]
pub struct PgRouteDraftRepository {
    pool: PgPool,
}

impl PgRouteDraftRepository {
    /// Create a new PgRouteDraftRepository with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Convert a DraftState to its database string representation.
fn state_to_str(state: DraftState) -> &'static str {
    match state {
        DraftState::Active => "active",
        DraftState::Published => "published",
        DraftState::Discarded => "discarded",
    }
}

/// Parse a state string from the database into a DraftState.
fn parse_state(s: &str) -> DraftState {
    match s {
        "active" => DraftState::Active,
        "published" => DraftState::Published,
        "discarded" => DraftState::Discarded,
        _ => DraftState::Discarded, // Defensive fallback
    }
}

/// Map a database error to a RouteEditingError.
fn map_db_error(e: sqlx::Error) -> RouteEditingError {
    RouteEditingError::OperationFailed {
        message: format!("database error: {e}"),
    }
}

/// Serialize geometry (Vec<Vec<RoutePoint>>) to a serde_json::Value for JSONB storage.
fn geometry_to_json(geometry: &[Vec<RoutePoint>]) -> JsonValue {
    serde_json::to_value(geometry).unwrap_or(JsonValue::Array(vec![]))
}

/// Deserialize geometry from a serde_json::Value back to Vec<Vec<RoutePoint>>.
fn json_to_geometry(value: JsonValue) -> Vec<Vec<RoutePoint>> {
    serde_json::from_value(value).unwrap_or_default()
}

/// Serialize a RouteOperation to a serde_json::Value for JSONB storage.
fn operation_to_json(op: &RouteOperation) -> JsonValue {
    serde_json::to_value(op).unwrap_or(JsonValue::Null)
}

/// Deserialize a RouteOperation from a serde_json::Value.
fn json_to_operation(value: JsonValue) -> Option<RouteOperation> {
    serde_json::from_value(value).ok()
}

/// Determine the operation_type string from a RouteOperation variant.
fn operation_type_str(op: &RouteOperation) -> &'static str {
    match op {
        RouteOperation::MovePoint { .. } => "move_point",
        RouteOperation::AddPoint { .. } => "add_point",
        RouteOperation::DeletePoint { .. } => "delete_point",
        RouteOperation::DeleteSection { .. } => "delete_section",
        RouteOperation::ReplaceSection { .. } => "replace_section",
        RouteOperation::SplitSegment { .. } => "split_segment",
        RouteOperation::JoinSegments { .. } => "join_segments",
    }
}

/// Reconstruct a RouteDraft from its database row and operations.
async fn load_draft_with_operations(
    pool: &PgPool,
    row: &sqlx::postgres::PgRow,
) -> Result<RouteDraft, RouteEditingError> {
    let id: Uuid = row.get("id");
    let activity_id: Uuid = row.get("activity_id");
    let owner_id: Uuid = row.get("owner_id");
    let base_route_version_id: Option<Uuid> = row.get("base_route_version_id");
    let revision: i64 = row.get("revision");
    let geometry_json: JsonValue = row.get("geometry");
    let state_str: String = row.get("state");
    let created_at: DateTime<Utc> = row.get("created_at");
    let updated_at: DateTime<Utc> = row.get("updated_at");

    // Load operations ordered by sequence_number
    let op_rows = sqlx::query(
        r#"
        SELECT operation_id, operation_type, operation_data, sequence_number, is_undone
        FROM route_editing.draft_operations
        WHERE draft_id = $1
        ORDER BY sequence_number ASC
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(map_db_error)?;

    let mut applied_operations = Vec::new();
    let mut undone_operations = Vec::new();

    for op_row in &op_rows {
        let op_id: Uuid = op_row.get("operation_id");
        let op_data: JsonValue = op_row.get("operation_data");
        let is_undone: bool = op_row.get("is_undone");

        // operation_data stores both the operation and geometry_before
        #[derive(serde::Deserialize)]
        struct StoredOperation {
            operation: serde_json::Value,
            geometry_before: serde_json::Value,
        }

        let stored: StoredOperation = serde_json::from_value(op_data).unwrap_or(StoredOperation {
            operation: JsonValue::Null,
            geometry_before: JsonValue::Array(vec![]),
        });

        let operation = match json_to_operation(stored.operation) {
            Some(op) => op,
            None => continue, // Skip malformed operations
        };
        let geometry_before = json_to_geometry(stored.geometry_before);

        let entry = OperationEntry {
            id: OperationId::new(op_id),
            operation,
            geometry_before,
        };

        if is_undone {
            undone_operations.push(entry);
        } else {
            applied_operations.push(entry);
        }
    }

    Ok(RouteDraft {
        id: RouteDraftId::new(id),
        activity_id: ActivityId(activity_id),
        owner_id: UserId::new(owner_id),
        base_route_version_id,
        revision: revision as u64,
        geometry: json_to_geometry(geometry_json),
        applied_operations,
        undone_operations,
        state: parse_state(&state_str),
        created_at,
        updated_at,
    })
}

#[async_trait]
impl RouteDraftRepository for PgRouteDraftRepository {
    async fn save(&self, draft: &RouteDraft) -> Result<(), RouteEditingError> {
        let mut tx = self.pool.begin().await.map_err(map_db_error)?;

        // Insert the draft row
        sqlx::query(
            r#"
            INSERT INTO route_editing.drafts (
                id, activity_id, owner_id, base_route_version_id,
                revision, geometry, state, created_at, updated_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(draft.id.0)
        .bind(draft.activity_id.0)
        .bind(draft.owner_id.0)
        .bind(draft.base_route_version_id)
        .bind(draft.revision as i64)
        .bind(geometry_to_json(&draft.geometry))
        .bind(state_to_str(draft.state))
        .bind(draft.created_at)
        .bind(draft.updated_at)
        .bind(None::<DateTime<Utc>>) // expires_at
        .execute(&mut *tx)
        .await
        .map_err(map_db_error)?;

        // Insert any initial operations (usually none for a new draft)
        for (seq, entry) in draft.applied_operations.iter().enumerate() {
            let op_data = serde_json::json!({
                "operation": operation_to_json(&entry.operation),
                "geometry_before": geometry_to_json(&entry.geometry_before),
            });

            sqlx::query(
                r#"
                INSERT INTO route_editing.draft_operations (
                    draft_id, operation_id, operation_type, operation_data,
                    sequence_number, is_undone, created_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, now())
                "#,
            )
            .bind(draft.id.0)
            .bind(entry.id.0)
            .bind(operation_type_str(&entry.operation))
            .bind(op_data)
            .bind(seq as i32)
            .bind(false)
            .execute(&mut *tx)
            .await
            .map_err(map_db_error)?;
        }

        tx.commit().await.map_err(map_db_error)?;
        Ok(())
    }

    async fn find_by_id(&self, id: RouteDraftId) -> Result<Option<RouteDraft>, RouteEditingError> {
        let row = sqlx::query(
            r#"
            SELECT id, activity_id, owner_id, base_route_version_id,
                   revision, geometry, state, created_at, updated_at
            FROM route_editing.drafts
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        match row {
            Some(r) => Ok(Some(load_draft_with_operations(&self.pool, &r).await?)),
            None => Ok(None),
        }
    }

    async fn find_active_by_activity(
        &self,
        activity_id: ActivityId,
        owner_id: UserId,
    ) -> Result<Option<RouteDraft>, RouteEditingError> {
        let row = sqlx::query(
            r#"
            SELECT id, activity_id, owner_id, base_route_version_id,
                   revision, geometry, state, created_at, updated_at
            FROM route_editing.drafts
            WHERE activity_id = $1 AND owner_id = $2 AND state = 'active'
            "#,
        )
        .bind(activity_id.0)
        .bind(owner_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        match row {
            Some(r) => Ok(Some(load_draft_with_operations(&self.pool, &r).await?)),
            None => Ok(None),
        }
    }

    async fn update(&self, draft: &RouteDraft) -> Result<(), RouteEditingError> {
        let mut tx = self.pool.begin().await.map_err(map_db_error)?;

        // Update the draft row
        sqlx::query(
            r#"
            UPDATE route_editing.drafts
            SET revision = $2,
                geometry = $3,
                state = $4,
                updated_at = $5
            WHERE id = $1
            "#,
        )
        .bind(draft.id.0)
        .bind(draft.revision as i64)
        .bind(geometry_to_json(&draft.geometry))
        .bind(state_to_str(draft.state))
        .bind(draft.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(map_db_error)?;

        // Replace all operations: delete existing and re-insert current state.
        // This ensures undo/redo stack changes are persisted correctly.
        sqlx::query(
            r#"
            DELETE FROM route_editing.draft_operations WHERE draft_id = $1
            "#,
        )
        .bind(draft.id.0)
        .execute(&mut *tx)
        .await
        .map_err(map_db_error)?;

        // Insert applied operations
        for (seq, entry) in draft.applied_operations.iter().enumerate() {
            let op_data = serde_json::json!({
                "operation": operation_to_json(&entry.operation),
                "geometry_before": geometry_to_json(&entry.geometry_before),
            });

            sqlx::query(
                r#"
                INSERT INTO route_editing.draft_operations (
                    draft_id, operation_id, operation_type, operation_data,
                    sequence_number, is_undone, created_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, now())
                "#,
            )
            .bind(draft.id.0)
            .bind(entry.id.0)
            .bind(operation_type_str(&entry.operation))
            .bind(op_data)
            .bind(seq as i32)
            .bind(false)
            .execute(&mut *tx)
            .await
            .map_err(map_db_error)?;
        }

        // Insert undone operations (with offset sequence numbers)
        let offset = draft.applied_operations.len();
        for (seq, entry) in draft.undone_operations.iter().enumerate() {
            let op_data = serde_json::json!({
                "operation": operation_to_json(&entry.operation),
                "geometry_before": geometry_to_json(&entry.geometry_before),
            });

            sqlx::query(
                r#"
                INSERT INTO route_editing.draft_operations (
                    draft_id, operation_id, operation_type, operation_data,
                    sequence_number, is_undone, created_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, now())
                "#,
            )
            .bind(draft.id.0)
            .bind(entry.id.0)
            .bind(operation_type_str(&entry.operation))
            .bind(op_data)
            .bind((offset + seq) as i32)
            .bind(true)
            .execute(&mut *tx)
            .await
            .map_err(map_db_error)?;
        }

        tx.commit().await.map_err(map_db_error)?;
        Ok(())
    }

    async fn find_by_operation_id(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<RouteDraftId>, RouteEditingError> {
        let row = sqlx::query(
            r#"
            SELECT draft_id
            FROM route_editing.draft_operations
            WHERE operation_id = $1
            "#,
        )
        .bind(operation_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(row.map(|r| {
            let draft_id: Uuid = r.get("draft_id");
            RouteDraftId::new(draft_id)
        }))
    }
}
