//! Helpers for repository tests: seeded users, grants, and one scope per kind of caller.

use chrono::Utc;
use postit_core::{AccountId, DelegationId, UserId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::delegations::{DelegationRepo, NewDelegation};
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

/// Grants `delegate` access to `owner`'s workspace at `level` on `accounts`, with no expiry,
/// through [`DelegationRepo::insert`] with the owner's scope. `accounts` must be the owner's
/// connected accounts.
///
/// # Panics
///
/// If the insert fails; this is a test helper.
pub async fn grant(
    pool: &PgPool,
    owner: UserId,
    delegate: UserId,
    level: Level,
    accounts: AccountLimit,
) -> DelegationId {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("grant: acquire: {e}"));
    let id = DelegationId::from(Uuid::now_v7());
    DelegationRepo::insert(
        &mut conn,
        &OwnerScope::own(owner),
        NewDelegation {
            id,
            delegate_id: delegate,
            level,
            accounts,
            expires_at: None,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("grant: {e}"));
    id
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

/// Seeds four active delegate users, each with a real grant from `owner`, and a stranger, and
/// returns a scope for each caller. Each delegate's scope comes from
/// [`DelegationRepo::resolve`], as a request acting through the grant would get it. `owner`
/// must be active, and `limited_accounts` must be the owner's connected accounts.
///
/// # Panics
///
/// If a seed insert fails or a grant does not resolve; this is a test helper.
pub async fn callers(pool: &PgPool, owner: UserId, limited_accounts: &[AccountId]) -> Callers {
    let delegate = |level, accounts| async move {
        let actor = seed_user(pool, "active").await;
        grant(pool, owner, actor, level, accounts).await;
        let mut conn = pool
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("callers: acquire: {e}"));
        DelegationRepo::resolve(&mut conn, actor, owner, Utc::now())
            .await
            .unwrap_or_else(|e| unreachable!("callers: resolve: {e}"))
            .unwrap_or_else(|| unreachable!("callers: the {level:?} grant did not resolve"))
    };
    Callers {
        owner: OwnerScope::own(owner),
        view: delegate(Level::View, AccountLimit::All).await,
        edit: delegate(Level::Edit, AccountLimit::All).await,
        publish: delegate(Level::Publish, AccountLimit::All).await,
        limited: delegate(
            Level::Publish,
            AccountLimit::only(limited_accounts.iter().copied()),
        )
        .await,
        stranger: OwnerScope::own(seed_user(pool, "active").await),
    }
}
