//! Transactional outbox pattern implementation.
//!
//! Events are written in the same database transaction as domain state changes,
//! ensuring at-least-once delivery. A background process polls and dispatches
//! unprocessed events to their handlers.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// An event stored in the outbox.
#[derive(Debug, Clone)]
pub struct OutboxEvent {
    /// Unique event identifier.
    pub id: Uuid,
    /// The type of aggregate that produced this event.
    pub aggregate_type: String,
    /// The identifier of the aggregate instance.
    pub aggregate_id: String,
    /// The event type discriminator.
    pub event_type: String,
    /// JSON payload of the event.
    pub payload: Value,
    /// When the event was created.
    pub created_at: DateTime<Utc>,
    /// Number of dispatch retries.
    pub retry_count: i32,
}

/// Outbox for publishing events within a transaction.
#[derive(Clone)]
pub struct Outbox {
    pool: PgPool,
}

impl Outbox {
    /// Create a new outbox instance.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Publish an event within an existing transaction.
    ///
    /// The event is inserted into the outbox table as part of the provided
    /// transaction, ensuring atomicity with the domain state change.
    pub async fn publish(
        tx: &mut Transaction<'_, Postgres>,
        aggregate_type: &str,
        aggregate_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO platform.outbox (id, aggregate_type, aggregate_id, event_type, payload)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(id)
        .bind(aggregate_type)
        .bind(aggregate_id)
        .bind(event_type)
        .bind(&payload)
        .execute(&mut **tx)
        .await?;

        Ok(id)
    }

    /// Poll for unprocessed outbox events.
    ///
    /// Returns events that have not been processed or permanently failed,
    /// ordered by creation time.
    pub async fn poll_unprocessed(&self, batch_size: i64) -> Result<Vec<OutboxEvent>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (Uuid, String, String, String, Value, DateTime<Utc>, i32)>(
            r#"
            SELECT id, aggregate_type, aggregate_id, event_type, payload, created_at, retry_count
            FROM platform.outbox
            WHERE processed_at IS NULL AND failed_at IS NULL
            ORDER BY created_at ASC
            LIMIT $1
            "#,
        )
        .bind(batch_size)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    aggregate_type,
                    aggregate_id,
                    event_type,
                    payload,
                    created_at,
                    retry_count,
                )| {
                    OutboxEvent {
                        id,
                        aggregate_type,
                        aggregate_id,
                        event_type,
                        payload,
                        created_at,
                        retry_count,
                    }
                },
            )
            .collect())
    }

    /// Mark an event as successfully processed.
    pub async fn mark_processed(&self, event_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE platform.outbox
            SET processed_at = now()
            WHERE id = $1
            "#,
        )
        .bind(event_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Mark an event as failed.
    ///
    /// If the event has been retried fewer than the maximum allowed times,
    /// it remains available for reprocessing. Otherwise, it is permanently
    /// marked as failed.
    pub async fn mark_failed(
        &self,
        event_id: Uuid,
        error_message: &str,
        max_retries: i32,
    ) -> Result<(), sqlx::Error> {
        let row = sqlx::query_as::<_, (i32,)>(
            r#"
            SELECT retry_count FROM platform.outbox WHERE id = $1
            "#,
        )
        .bind(event_id)
        .fetch_one(&self.pool)
        .await?;

        let new_retry_count = row.0 + 1;

        if new_retry_count >= max_retries {
            sqlx::query(
                r#"
                UPDATE platform.outbox
                SET retry_count = $2, error_message = $3, failed_at = now()
                WHERE id = $1
                "#,
            )
            .bind(event_id)
            .bind(new_retry_count)
            .bind(error_message)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE platform.outbox
                SET retry_count = $2, error_message = $3
                WHERE id = $1
                "#,
            )
            .bind(event_id)
            .bind(new_retry_count)
            .bind(error_message)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
}

/// Trait for handling outbox events.
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// The event type this handler processes.
    fn event_type(&self) -> &str;

    /// Handle an outbox event.
    async fn handle(
        &self,
        event: &OutboxEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Dispatches outbox events to registered handlers.
pub struct OutboxDispatcher {
    outbox: Outbox,
    handlers: Vec<Box<dyn EventHandler>>,
    max_retries: i32,
}

impl OutboxDispatcher {
    /// Create a new outbox dispatcher.
    pub fn new(outbox: Outbox, max_retries: i32) -> Self {
        Self {
            outbox,
            handlers: Vec::new(),
            max_retries,
        }
    }

    /// Register an event handler.
    pub fn register_handler(&mut self, handler: Box<dyn EventHandler>) {
        self.handlers.push(handler);
    }

    /// Process a batch of unprocessed events.
    ///
    /// For each event, finds the matching handler and dispatches. On success,
    /// marks the event as processed. On failure, marks it as failed with retry
    /// tracking.
    pub async fn process_batch(&self, batch_size: i64) -> Result<usize, sqlx::Error> {
        let events = self.outbox.poll_unprocessed(batch_size).await?;
        let mut processed = 0;

        for event in &events {
            let handler = self
                .handlers
                .iter()
                .find(|h| h.event_type() == event.event_type);

            match handler {
                Some(h) => match h.handle(event).await {
                    Ok(()) => {
                        self.outbox.mark_processed(event.id).await?;
                        processed += 1;
                    }
                    Err(e) => {
                        self.outbox
                            .mark_failed(event.id, &e.to_string(), self.max_retries)
                            .await?;
                    }
                },
                None => {
                    // No handler registered for this event type; mark as failed
                    self.outbox
                        .mark_failed(
                            event.id,
                            &format!("no handler registered for event type: {}", event.event_type),
                            self.max_retries,
                        )
                        .await?;
                }
            }
        }

        Ok(processed)
    }
}
