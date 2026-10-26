use std::sync::Arc;

use postit_core::{AccountId, DelegationId, UserId};
use uuid::Uuid;

/// The three delegation levels from plan 01, ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// May read.
    View,
    /// May read and change.
    Edit,
    /// May read, change, and publish.
    Publish,
}

impl Level {
    /// The value stored in the `level` column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Edit => "edit",
            Self::Publish => "publish",
        }
    }
}

impl std::str::FromStr for Level {
    type Err = crate::users::ParseEnumError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "view" => Ok(Self::View),
            "edit" => Ok(Self::Edit),
            "publish" => Ok(Self::Publish),
            other => Err(crate::users::ParseEnumError(other.to_string())),
        }
    }
}

/// Which of the owner's connected accounts a delegation reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountLimit {
    /// Every account.
    All,
    /// Only these accounts, sorted and deduplicated. Empty means none.
    Only(Arc<[AccountId]>),
}

impl AccountLimit {
    /// A limit to `ids`, sorted and deduplicated.
    #[must_use]
    pub fn only(ids: impl IntoIterator<Item = AccountId>) -> Self {
        let mut ids: Vec<AccountId> = ids.into_iter().collect();
        ids.sort_by_key(AccountId::as_uuid);
        ids.dedup();
        Self::Only(ids.into())
    }
}

/// What a caller may do to owned rows in the workspace named by [`OwnerScope::owner`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// The workspace owner acting in their own workspace.
    Owner,
    /// A delegate acting under a grant.
    Delegated {
        /// The grant being used.
        delegation: DelegationId,
        /// What the grant allows.
        level: Level,
        /// Which accounts the grant reaches.
        accounts: AccountLimit,
    },
}

/// What an action needs. `OwnerOnly` is refused for every delegate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// May read.
    View,
    /// May read and change.
    Edit,
    /// May read, change, and publish.
    Publish,
    /// Only the workspace owner.
    OwnerOnly,
}

/// Why a scope check failed.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScopeError {
    /// The delegation grant is below the required level.
    #[error("delegation grant does not allow this action")]
    InsufficientAccess,
    /// The action is reserved for the workspace owner.
    #[error("only the workspace owner may do this")]
    OwnerOnly,
}

/// Who is acting (`actor`), whose workspace they're acting in (`owner`), and at what level
/// (`access`). `actor == owner` for the caller's own workspace; they differ for a delegate.
#[derive(Debug, Clone)]
pub struct OwnerScope {
    /// The user performing the action.
    pub actor: UserId,
    /// The user whose workspace the action targets.
    pub owner: UserId,
    /// The actor's access in that workspace.
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

    /// The scope of `actor` acting in `owner`'s workspace under a grant.
    #[must_use]
    pub fn delegated(
        actor: UserId,
        owner: UserId,
        delegation: DelegationId,
        level: Level,
        accounts: AccountLimit,
    ) -> Self {
        Self {
            actor,
            owner,
            access: Access::Delegated {
                delegation,
                level,
                accounts,
            },
        }
    }

    /// Checks whether this scope permits `capability`.
    ///
    /// # Errors
    ///
    /// [`ScopeError::OwnerOnly`] for a delegate when `capability` is `OwnerOnly`, and
    /// [`ScopeError::InsufficientAccess`] when the grant's level is below `capability`.
    pub fn require(&self, capability: Capability) -> Result<(), ScopeError> {
        let Access::Delegated { level, .. } = &self.access else {
            return Ok(());
        };
        let needed = match capability {
            Capability::View => Level::View,
            Capability::Edit => Level::Edit,
            Capability::Publish => Level::Publish,
            Capability::OwnerOnly => return Err(ScopeError::OwnerOnly),
        };
        if *level >= needed {
            Ok(())
        } else {
            Err(ScopeError::InsufficientAccess)
        }
    }

    /// The account IDs a query must be limited to: `None` means no limit, `Some(vec![])`
    /// means no account at all. Bind it as `($n::uuid[] IS NULL OR account_id = ANY($n))`.
    #[must_use]
    pub fn account_filter(&self) -> Option<Vec<Uuid>> {
        match &self.access {
            Access::Delegated {
                accounts: AccountLimit::Only(ids),
                ..
            } => Some(ids.iter().map(AccountId::as_uuid).collect()),
            Access::Owner | Access::Delegated { .. } => None,
        }
    }

    /// Whether this scope reaches `account`.
    #[must_use]
    pub fn allows_account(&self, account: AccountId) -> bool {
        match &self.access {
            Access::Delegated {
                accounts: AccountLimit::Only(ids),
                ..
            } => ids.contains(&account),
            Access::Owner | Access::Delegated { .. } => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(n: u128) -> UserId {
        UserId::from(Uuid::from_u128(n))
    }

    fn account(n: u128) -> AccountId {
        AccountId::from(Uuid::from_u128(n))
    }

    fn delegate(level: Level, accounts: AccountLimit) -> OwnerScope {
        OwnerScope::delegated(
            user(2),
            user(1),
            DelegationId::from(Uuid::from_u128(9)),
            level,
            accounts,
        )
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
        assert_eq!(scope.require(Capability::OwnerOnly), Ok(()));
    }

    #[test]
    fn capability_orders_view_below_edit_below_publish() {
        assert!(Capability::View < Capability::Edit);
        assert!(Capability::Edit < Capability::Publish);
    }

    #[test]
    fn delegated_levels_gate_capabilities() {
        let view = delegate(Level::View, AccountLimit::All);
        assert_eq!(view.require(Capability::View), Ok(()));
        assert_eq!(
            view.require(Capability::Edit),
            Err(ScopeError::InsufficientAccess)
        );
        let edit = delegate(Level::Edit, AccountLimit::All);
        assert_eq!(edit.require(Capability::Edit), Ok(()));
        assert_eq!(
            edit.require(Capability::Publish),
            Err(ScopeError::InsufficientAccess)
        );
        let publish = delegate(Level::Publish, AccountLimit::All);
        assert_eq!(publish.require(Capability::Publish), Ok(()));
    }

    #[test]
    fn owner_only_refused_for_every_delegate() {
        for level in [Level::View, Level::Edit, Level::Publish] {
            let scope = delegate(level, AccountLimit::All);
            assert_eq!(
                scope.require(Capability::OwnerOnly),
                Err(ScopeError::OwnerOnly)
            );
        }
    }

    #[test]
    fn account_filter_shapes() {
        let (a, b) = (account(1), account(2));
        assert_eq!(OwnerScope::own(user(1)).account_filter(), None);
        assert_eq!(
            delegate(Level::View, AccountLimit::All).account_filter(),
            None
        );
        assert_eq!(
            delegate(Level::View, AccountLimit::only([b, a, a])).account_filter(),
            Some(vec![a.as_uuid(), b.as_uuid()])
        );
        assert_eq!(
            delegate(Level::View, AccountLimit::only([])).account_filter(),
            Some(vec![])
        );
    }

    #[test]
    fn allows_account_respects_limit() {
        let (a, b) = (account(1), account(2));
        assert!(OwnerScope::own(user(1)).allows_account(a));
        assert!(delegate(Level::View, AccountLimit::All).allows_account(a));
        let limited = delegate(Level::View, AccountLimit::only([a]));
        assert!(limited.allows_account(a));
        assert!(!limited.allows_account(b));
        assert!(!delegate(Level::View, AccountLimit::only([])).allows_account(a));
    }
}
