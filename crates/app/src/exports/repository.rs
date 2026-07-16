//! Export repository trait.
//!
//! Defines the persistence interface for export job aggregates. Implementations
//! live in the platform/persistence layer.

use async_trait::async_trait;

use crate::identity::UserId;

use super::{ExportError, ExportJob, ExportJobId};

/// Repository trait for export job persistence.
///
/// Domain code programs against this trait; the actual persistence implementation
/// is provided by the infrastructure layer.
#[async_trait]
pub trait ExportRepository: Send + Sync {
    /// Save a new export job to the store.
    async fn save(&self, export_job: &ExportJob) -> Result<(), ExportError>;

    /// Find an export job by its ID.
    async fn find_by_id(&self, id: ExportJobId) -> Result<Option<ExportJob>, ExportError>;

    /// Find an export job by idempotency key for a given owner.
    async fn find_by_idempotency_key(
        &self,
        owner_id: UserId,
        key: &str,
    ) -> Result<Option<ExportJob>, ExportError>;

    /// Update an existing export job (persists all fields).
    async fn update(&self, export_job: &ExportJob) -> Result<(), ExportError>;
}
