//! Helpers for repository tests: seeded users and one scope per kind of caller.

use postit_core::{AccountId, DelegationId, UserId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{AccountLimit, Level, OwnerScope};

/// Inserts a user with `status` directly, bypassing provisioning. `status` must be one the
/// `users` table accepts.
///
/// # Panics
///
/// If the insert fails; this is a test helper.
pub async fn seed_user(pool: &PgPool, status: &str) -> UserId {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status) \
         VALUES ($1, 'https://issuer.test', $2, 'Test User', 'member', $3)",
    )
    .bind(id)
    .bind(id.to_string())
    .bind(status)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("seed_user: {e}"));
    UserId::from(id)
}

/// One scope per kind of caller against the same workspace.
#[derive(Debug, Clone)]
pub struct Callers {
    /// The workspace owner.
    pub owner: OwnerScope,
    /// A delegate at `view`, reaching every account.
    pub view: OwnerScope,
    /// A delegate at `edit`, reaching every account.
    pub edit: OwnerScope,
    /// A delegate at `publish`, reaching every account.
    pub publish: OwnerScope,
    /// A delegate at `publish`, limited to the accounts passed to [`callers`].
    pub limited: OwnerScope,
    /// An active user with no relationship to the owner, acting in their own workspace.
    pub stranger: OwnerScope,
}

/// Seeds four delegate users and a stranger and returns a scope for each caller. The
/// delegation IDs are fresh and have no row until the `delegations` table's repository
/// creates one.
///
/// # Panics
///
/// If a seed insert fails; this is a test helper.
pub async fn callers(pool: &PgPool, owner: UserId, other_accounts: &[AccountId]) -> Callers {
    let delegate = |actor, level, accounts| {
        OwnerScope::delegated(
            actor,
            owner,
            DelegationId::from(Uuid::now_v7()),
            level,
            accounts,
        )
    };
    Callers {
        owner: OwnerScope::own(owner),
        view: delegate(
            seed_user(pool, "active").await,
            Level::View,
            AccountLimit::All,
        ),
        edit: delegate(
            seed_user(pool, "active").await,
            Level::Edit,
            AccountLimit::All,
        ),
        publish: delegate(
            seed_user(pool, "active").await,
            Level::Publish,
            AccountLimit::All,
        ),
        limited: delegate(
            seed_user(pool, "active").await,
            Level::Publish,
            AccountLimit::only(other_accounts.iter().copied()),
        ),
        stranger: OwnerScope::own(seed_user(pool, "active").await),
    }
}
