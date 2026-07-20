//! Ownership policy enforcement.
//!
//! Implements a deny-by-default ownership check. A resource can only be accessed
//! by the user who owns it unless explicit sharing is configured.

use uuid::Uuid;

use crate::error::PlatformError;

/// Assert that the acting user is the owner of a resource.
///
/// Returns `Ok(())` if the user IDs match, or a `Forbidden` error otherwise.
/// This enforces the deny-by-default ownership policy.
pub fn assert_owner(actor_user_id: Uuid, resource_owner_id: Uuid) -> Result<(), PlatformError> {
    if actor_user_id == resource_owner_id {
        Ok(())
    } else {
        Err(PlatformError::forbidden(
            "you do not have permission to access this resource",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_matches_returns_ok() {
        let user_id = Uuid::new_v4();
        assert!(assert_owner(user_id, user_id).is_ok());
    }

    #[test]
    fn different_users_returns_forbidden() {
        let actor = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let result = assert_owner(actor, owner);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, PlatformError::Forbidden { .. }));
    }
}
