//! Duplicate detection for imports.
//!
//! Provides a trait-based interface for checking whether a file has already
//! been imported by the same owner.

use async_trait::async_trait;

use crate::activity_catalog::ActivityId;
use crate::identity::UserId;

use super::checksum::Checksum;
use super::{ImportError, ImportId};

/// Result of a duplicate check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuplicateCheckResult {
    /// No duplicate found.
    NotDuplicate,
    /// An exact duplicate exists.
    ExactDuplicate {
        existing_import_id: ImportId,
        existing_activity_id: Option<ActivityId>,
    },
}

/// Trait for checking if a file has already been imported.
///
/// Implementations live in the persistence layer.
#[async_trait]
pub trait CheckDuplicate: Send + Sync {
    /// Check if a file with the given checksum has already been imported by this owner.
    async fn check(
        &self,
        owner_id: UserId,
        checksum: &Checksum,
    ) -> Result<DuplicateCheckResult, ImportError>;
}
