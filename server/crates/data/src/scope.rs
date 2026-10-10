use postit_core::UserId;

/// What a caller may do to owned rows in the workspace named by [`OwnerScope::owner`].
/// Only `Owner` access exists until plan 03 phase B2 adds `Access::Delegated`; the type is
/// introduced now so no call site changes when that variant arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// The workspace owner acting in their own workspace.
    Owner,
}

/// The three delegation levels from plan 01. Unused by any P4 caller (every P4 repository
/// is either owner-only or admin-only), defined now because `OwnerScope::require` must
/// exist from the start per plan 01's data-model section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// May read.
    View,
    /// May read and change.
    Edit,
    /// May read, change, and publish.
    Publish,
}

/// Why a scope check failed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScopeError {
    /// The delegation grant is below the required level.
    #[error("delegation grant does not allow this action")]
    InsufficientAccess,
}

/// Who is acting (`actor`), whose workspace they're acting in (`owner`), and at what level
/// (`access`). `actor == owner` for the caller's own workspace; they differ only once
/// delegation exists (plan 03).
#[derive(Debug, Clone, Copy)]
pub struct OwnerScope {
    /// The user performing the action.
    pub actor: UserId,
    /// The user whose workspace the action targets.
    pub owner: UserId,
    /// The actor's access level in that workspace.
    pub access: Access,
}

impl OwnerScope {
    /// The scope of `user` acting in their own workspace.
    #[must_use]
    pub fn own(user: UserId) -> Self {
        Self {
            actor: user,
            owner: user,
            access: Access::Owner,
        }
    }

    /// Checks whether this scope permits `capability`. Always succeeds for `Access::Owner`
    /// — the only variant that exists in P4 — since an owner acting in their own workspace
    /// has every capability.
    ///
    /// # Errors
    ///
    /// Returns [`ScopeError::InsufficientAccess`] once `Access::Delegated` exists (plan 03)
    /// and the grant's level is below `capability`. Never returns an error in this phase.
    pub fn require(&self, _capability: Capability) -> Result<(), ScopeError> {
        match self.access {
            Access::Owner => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(n: u128) -> UserId {
        UserId::from(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn own_scope_has_actor_equal_to_owner() {
        let id = user(1);
        let scope = OwnerScope::own(id);

        assert_eq!(scope.actor, id);
        assert_eq!(scope.owner, id);
        assert_eq!(scope.access, Access::Owner);
    }

    #[test]
    fn owner_access_permits_every_capability() {
        let scope = OwnerScope::own(user(1));

        assert_eq!(scope.require(Capability::View), Ok(()));
        assert_eq!(scope.require(Capability::Edit), Ok(()));
        assert_eq!(scope.require(Capability::Publish), Ok(()));
    }

    #[test]
    fn capability_orders_view_below_edit_below_publish() {
        assert!(Capability::View < Capability::Edit);
        assert!(Capability::Edit < Capability::Publish);
    }
}
