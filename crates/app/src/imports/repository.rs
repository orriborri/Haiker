//! Import repository trait.
//!
//! Defines the persistence interface for import aggregates. Implementations
//! live in the platform/persistence layer.

use async_trait::async_trait;

use crate::activity_catalog::ActivityId;
use crate::identity::UserId;

use super::checksum::Checksum;
use super::{Import, ImportError, ImportId};

/// Repository trait for import persistence.
///
/// Domain code programs against this trait; the actual persistence implementation
/// is provided by the infrastructure layer.
#[async_trait]
pub trait ImportRepository: Send + Sync {
    /// Save a new import to the store.
    async fn save(&self, import: &Import) -> Result<(), ImportError>;

    /// Find an import by its ID.
    async fn find_by_id(&self, id: ImportId) -> Result<Option<Import>, ImportError>;

    /// Find an import by idempotency key for a given owner.
    async fn find_by_idempotency_key(
        &self,
        owner_id: UserId,
        key: &str,
    ) -> Result<Option<Import>, ImportError>;

    /// Find an import by checksum for a given owner (for duplicate detection).
    async fn find_by_checksum(
        &self,
        owner_id: UserId,
        checksum: &Checksum,
    ) -> Result<Option<Import>, ImportError>;

    /// Find a completed import by checksum for a given owner, returning the
    /// import ID and its associated activity ID (if any).
    ///
    /// This is used by the duplicate detection subsystem to provide a reference
    /// to the existing activity without loading the full Import aggregate.
    async fn find_completed_by_checksum(
        &self,
        owner_id: UserId,
        checksum: &Checksum,
    ) -> Result<Option<(ImportId, Option<ActivityId>)>, ImportError>;

    /// Update an existing import (persists all fields).
    async fn update(&self, import: &Import) -> Result<(), ImportError>;
}
