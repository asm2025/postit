mod common;

use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use emixdb::dto::Pagination;
use postit_core::{AccountId, DelegationId, UserId};
use postit_data::accounts::AccountRepo;
use postit_data::delegations::{
    Delegation, DelegationChange, DelegationRepo, EndedReason, NewDelegation,
};
use postit_data::testkit::{callers, seed_user};
use postit_data::users::{UserStatus, UsersRepo};
use postit_data::{Access, AccountLimit, DataError, Level, OwnerScope, ScopeError};
use sqlx::PgPool;
use sqlx::pool::PoolConnection;
use uuid::Uuid;

use common::account;

async fn conn(pool: &PgPool) -> PoolConnection<sqlx::Postgres> {
    pool.acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"))
}

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn new(delegate: UserId, level: Level, accounts: AccountLimit) -> NewDelegation {
    NewDelegation {
        id: DelegationId::from(Uuid::now_v7()),
        delegate_id: delegate,
        level,
        accounts,
        expires_at: None,
    }
}

async fn grant(
    pool: &PgPool,
    owner: UserId,
    delegate: UserId,
    accounts: AccountLimit,
) -> Delegation {
    DelegationRepo::insert(
        &mut *conn(pool).await,
        &OwnerScope::own(owner),
        new(delegate, Level::Edit, accounts),
    )
    .await
    .unwrap_or_else(|e| unreachable!("grant: {e}"))
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
}

/// Waits until another backend in this test's database is blocked on a lock, so a test proves
/// a statement waits instead of only observing that it has not finished yet.
async fn wait_for_lock_wait(pool: &PgPool) {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND pid <> pg_backend_pid() \
             AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("pg_stat_activity: {e}"));
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(StdDuration::from_millis(50)).await;
    }
    unreachable!("no backend ever waited on a lock");
}

async fn grant_and_links(pool: &PgPool, id: DelegationId) -> (i64, i64) {
    let grants = count(
        pool,
        "SELECT count(*) FROM delegations WHERE id = $1",
        id.as_uuid(),
    )
    .await;
    let links = count(
        pool,
        "SELECT count(*) FROM delegation_accounts WHERE delegation_id = $1",
        id.as_uuid(),
    )
    .await;
    (grants, links)
}

fn conflict(result: Result<Delegation, DataError>, code: &str) {
    match result {
        Err(DataError::Conflict(got)) => assert_eq!(got, code),
        other => unreachable!("expected Conflict({code}), got {other:?}"),
    }
}

#[sqlx::test]
async fn insert_all_accounts_and_read_back(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let expires = now() + Duration::days(7);
    let mut input = new(delegate, Level::Publish, AccountLimit::All);
    input.expires_at = Some(expires);
    let id = input.id;

    let d = DelegationRepo::insert(&mut *conn(&pool).await, &OwnerScope::own(owner), input)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));

    assert_eq!(d.id, id);
    assert_eq!(d.owner_id, owner);
    assert_eq!(d.delegate_id, delegate);
    assert_eq!(d.level, Level::Publish);
    assert_eq!(d.accounts, AccountLimit::All);
    assert_eq!(
        d.expires_at.map(|t| t.timestamp_micros()),
        Some(expires.timestamp_micros())
    );
    assert_eq!(d.ended_at, None);
    assert_eq!(d.ended_reason, None);
    assert_eq!(d.revoked_by, None);
    assert_eq!(d.created_by, Some(owner));
    assert_eq!(d.updated_by, Some(owner));
    assert!(d.is_live(now()));
    assert!(!d.is_live(expires + Duration::seconds(1)));

    let locked = DelegationRepo::lock(&mut *conn(&pool).await, &OwnerScope::own(owner), id)
        .await
        .unwrap_or_else(|e| unreachable!("lock: {e}"));
    assert_eq!(locked, d);
}

#[sqlx::test]
async fn insert_limited_stores_accounts_sorted(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let c = account(&pool, owner, "c").await;

    let d = grant(
        &pool,
        owner,
        delegate,
        AccountLimit::Only(vec![c, a, b, a].into()),
    )
    .await;

    let mut sorted = vec![a, b, c];
    sorted.sort_by_key(AccountId::as_uuid);
    assert_eq!(d.accounts, AccountLimit::Only(sorted.clone().into()));
    let stored = count(
        &pool,
        "SELECT count(*) FROM delegation_accounts WHERE delegation_id = $1",
        d.id.as_uuid(),
    )
    .await;
    assert_eq!(stored, 3);
    let locked = DelegationRepo::lock(&mut *conn(&pool).await, &OwnerScope::own(owner), d.id)
        .await
        .unwrap_or_else(|e| unreachable!("lock: {e}"));
    assert_eq!(locked.accounts, AccountLimit::Only(sorted.into()));
}

#[sqlx::test]
async fn insert_empty_limit_is_allowed(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;

    let d = grant(&pool, owner, delegate, AccountLimit::only([])).await;

    assert_eq!(d.accounts, AccountLimit::only([]));
    let locked = DelegationRepo::lock(&mut *conn(&pool).await, &OwnerScope::own(owner), d.id)
        .await
        .unwrap_or_else(|e| unreachable!("lock: {e}"));
    assert_eq!(locked.accounts, AccountLimit::only([]));
}

#[sqlx::test]
async fn insert_refuses_self(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;

    let result = DelegationRepo::insert(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        new(owner, Level::View, AccountLimit::All),
    )
    .await;

    conflict(result, "self_delegation");
}

#[sqlx::test]
async fn insert_refuses_another_owners_account(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let mine = account(&pool, owner, "mine").await;
    let theirs = account(&pool, other, "theirs").await;

    let result = DelegationRepo::insert(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        new(delegate, Level::View, AccountLimit::only([mine, theirs])),
    )
    .await;

    conflict(result, "account_unavailable");
    let left = count(
        &pool,
        "SELECT count(*) FROM delegations WHERE owner_id = $1",
        owner.as_uuid(),
    )
    .await;
    assert_eq!(left, 0);
}

#[sqlx::test]
async fn composite_fk_rejects_cross_owner_account(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let theirs = account(&pool, other, "theirs").await;
    let d = grant(&pool, owner, delegate, AccountLimit::only([])).await;

    let err = sqlx::query(
        "INSERT INTO delegation_accounts (owner_id, delegation_id, account_id) VALUES ($1, $2, $3)",
    )
    .bind(owner.as_uuid())
    .bind(d.id.as_uuid())
    .bind(theirs.as_uuid())
    .execute(&pool)
    .await
    .err()
    .unwrap_or_else(|| unreachable!("another owner's account was accepted"));

    assert!(
        err.as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_foreign_key_violation),
        "{err}"
    );
}

#[sqlx::test]
async fn insert_refuses_disconnected_account(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    AccountRepo::disconnect(&mut *conn(&pool).await, &OwnerScope::own(owner), a)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    let result = DelegationRepo::insert(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        new(delegate, Level::View, AccountLimit::only([a])),
    )
    .await;

    conflict(result, "account_unavailable");
}

#[sqlx::test]
async fn insert_waits_for_concurrent_disconnect(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let scope = OwnerScope::own(owner);

    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    AccountRepo::disconnect(&mut tx, &scope, a)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    let inserting = tokio::spawn({
        let pool = pool.clone();
        let scope = scope.clone();
        async move {
            let mut c = conn(&pool).await;
            DelegationRepo::insert(
                &mut c,
                &scope,
                new(delegate, Level::View, AccountLimit::only([a])),
            )
            .await
        }
    });
    wait_for_lock_wait(&pool).await;
    assert!(
        !inserting.is_finished(),
        "insert did not wait for the disconnect"
    );

    tx.commit()
        .await
        .unwrap_or_else(|e| unreachable!("commit: {e}"));
    let result = inserting
        .await
        .unwrap_or_else(|e| unreachable!("join: {e}"));

    conflict(result, "account_unavailable");
    let rows = count(
        &pool,
        "SELECT count(*) FROM delegation_accounts WHERE account_id = $1",
        a.as_uuid(),
    )
    .await;
    assert_eq!(rows, 0);
}

#[sqlx::test]
async fn update_waits_for_concurrent_disconnect(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let scope = OwnerScope::own(owner);
    let d = grant(&pool, owner, delegate, AccountLimit::only([a])).await;

    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    // The first step of `disconnect`: lock account `a` FOR UPDATE. Taking it here, before
    // `update` starts, lets the real `disconnect` below run its link-row delete while `update`
    // is waiting.
    sqlx::query("SELECT 1 FROM connected_accounts WHERE id = $1 FOR UPDATE")
        .bind(a.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap_or_else(|e| unreachable!("lock a: {e}"));

    // Keeps `a` in the limit and adds `b`. It must block on the account row before touching
    // any link row.
    let updating = tokio::spawn({
        let pool = pool.clone();
        let scope = scope.clone();
        async move {
            let mut c = conn(&pool).await;
            DelegationRepo::update(
                &mut c,
                &scope,
                d.id,
                DelegationChange {
                    level: None,
                    accounts: Some(AccountLimit::only([a, b])),
                    expires_at: None,
                },
            )
            .await
        }
    });
    wait_for_lock_wait(&pool).await;
    assert!(
        !updating.is_finished(),
        "update did not wait for the disconnect"
    );

    // The rest of `disconnect` marks `a` disconnected and removes it from every limit. Had
    // `update` deleted the old link rows before locking `a`, it would hold the (grant, a) row
    // while waiting on `a`, and this would deadlock (40P01).
    AccountRepo::disconnect(&mut tx, &scope, a)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect (a deadlock fails here): {e}"));
    tx.commit()
        .await
        .unwrap_or_else(|e| unreachable!("commit: {e}"));
    let result = updating.await.unwrap_or_else(|e| unreachable!("join: {e}"));

    conflict(result, "account_unavailable");
    assert_eq!(grant_and_links(&pool, d.id).await, (1, 0));
}

#[sqlx::test]
async fn second_live_grant_for_pair_is_delegation_exists(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    grant(&pool, owner, delegate, AccountLimit::All).await;

    let result = DelegationRepo::insert(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        new(delegate, Level::View, AccountLimit::All),
    )
    .await;

    conflict(result, "delegation_exists");
}

#[sqlx::test]
async fn ended_grant_frees_the_pair(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let first = grant(&pool, owner, delegate, AccountLimit::All).await;
    let at = now();

    DelegationRepo::end(
        &mut *conn(&pool).await,
        first.id,
        EndedReason::Revoked,
        Some(owner),
        at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("end: {e}"));
    // Ending twice is a no-op that keeps the first values.
    DelegationRepo::end(
        &mut *conn(&pool).await,
        first.id,
        EndedReason::Left,
        None,
        at + Duration::hours(1),
    )
    .await
    .unwrap_or_else(|e| unreachable!("end again: {e}"));

    let ended = DelegationRepo::lock(&mut *conn(&pool).await, &OwnerScope::own(owner), first.id)
        .await
        .unwrap_or_else(|e| unreachable!("lock: {e}"));
    assert_eq!(
        ended.ended_at.map(|t| t.timestamp_micros()),
        Some(at.timestamp_micros())
    );
    assert_eq!(ended.ended_reason, Some(EndedReason::Revoked));
    assert_eq!(ended.revoked_by, Some(owner));
    assert!(!ended.is_live(at));

    let live = DelegationRepo::lock_live_for_pair(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        delegate,
    )
    .await
    .unwrap_or_else(|e| unreachable!("lock pair: {e}"));
    assert_eq!(live, None);

    let second = grant(&pool, owner, delegate, AccountLimit::All).await;
    let live = DelegationRepo::lock_live_for_pair(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        delegate,
    )
    .await
    .unwrap_or_else(|e| unreachable!("lock pair: {e}"));
    assert_eq!(live.map(|d| d.id), Some(second.id));
}

#[sqlx::test]
async fn update_replaces_account_limit_and_sets_updated_by(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let d = grant(&pool, owner, delegate, AccountLimit::only([a])).await;

    let changed = DelegationRepo::update(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        d.id,
        DelegationChange {
            level: Some(Level::Publish),
            accounts: Some(AccountLimit::only([b])),
            expires_at: None,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("update: {e}"));

    assert_eq!(changed.level, Level::Publish);
    assert_eq!(changed.accounts, AccountLimit::only([b]));
    assert_eq!(changed.updated_by, Some(owner));
    let stored: Vec<Uuid> =
        sqlx::query_scalar("SELECT account_id FROM delegation_accounts WHERE delegation_id = $1")
            .bind(d.id.as_uuid())
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("rows: {e}"));
    assert_eq!(stored, vec![b.as_uuid()]);

    let all = DelegationRepo::update(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        d.id,
        DelegationChange {
            level: None,
            accounts: Some(AccountLimit::All),
            expires_at: None,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("update to all: {e}"));
    assert_eq!(all.accounts, AccountLimit::All);
    assert_eq!(all.level, Level::Publish);
    let rows = count(
        &pool,
        "SELECT count(*) FROM delegation_accounts WHERE delegation_id = $1",
        d.id.as_uuid(),
    )
    .await;
    assert_eq!(rows, 0);

    let refused = DelegationRepo::update(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        d.id,
        DelegationChange {
            level: None,
            accounts: Some(AccountLimit::only([account(&pool, delegate, "x").await])),
            expires_at: None,
        },
    )
    .await;
    conflict(refused, "account_unavailable");
}

#[sqlx::test]
async fn update_clears_expiry(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let mut input = new(delegate, Level::View, AccountLimit::All);
    input.expires_at = Some(now() + Duration::days(1));
    let d = DelegationRepo::insert(&mut *conn(&pool).await, &OwnerScope::own(owner), input)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let kept = DelegationRepo::update(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        d.id,
        DelegationChange {
            level: Some(Level::Edit),
            accounts: None,
            expires_at: None,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("update: {e}"));
    assert_eq!(kept.expires_at, d.expires_at);

    let cleared = DelegationRepo::update(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        d.id,
        DelegationChange {
            level: None,
            accounts: None,
            expires_at: Some(None),
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("clear: {e}"));
    assert_eq!(cleared.expires_at, None);
    assert_eq!(cleared.level, Level::Edit);
}

#[sqlx::test]
async fn delegated_scope_cannot_insert_update_lock_or_list(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let d = grant(&pool, owner, delegate, AccountLimit::All).await;
    let c = callers(&pool, owner, &[]).await;
    let scope = &c.publish;
    let mut cn = conn(&pool).await;
    let owner_only = |r: Result<(), DataError>| {
        assert!(
            matches!(r, Err(DataError::Scope(ScopeError::OwnerOnly))),
            "{r:?}"
        );
    };

    owner_only(
        DelegationRepo::insert(
            &mut cn,
            scope,
            new(
                seed_user(&pool, "active").await,
                Level::View,
                AccountLimit::All,
            ),
        )
        .await
        .map(|_| ()),
    );
    owner_only(
        DelegationRepo::update(
            &mut cn,
            scope,
            d.id,
            DelegationChange {
                level: Some(Level::Publish),
                accounts: None,
                expires_at: None,
            },
        )
        .await
        .map(|_| ()),
    );
    owner_only(DelegationRepo::lock(&mut cn, scope, d.id).await.map(|_| ()));
    owner_only(
        DelegationRepo::lock_live_for_pair(&mut cn, scope, delegate)
            .await
            .map(|_| ()),
    );
    owner_only(
        DelegationRepo::list_granted(&mut cn, scope, true, page())
            .await
            .map(|_| ()),
    );
}

#[sqlx::test]
async fn lock_is_not_found_for_another_owner(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let d = grant(&pool, owner, delegate, AccountLimit::All).await;

    let result = DelegationRepo::lock(&mut *conn(&pool).await, &OwnerScope::own(other), d.id).await;
    assert!(matches!(result, Err(DataError::NotFound)), "{result:?}");

    let result = DelegationRepo::update(
        &mut *conn(&pool).await,
        &OwnerScope::own(other),
        d.id,
        DelegationChange {
            level: Some(Level::Publish),
            accounts: None,
            expires_at: None,
        },
    )
    .await;
    assert!(matches!(result, Err(DataError::NotFound)), "{result:?}");

    // The delegate side finds it by delegate, and nobody else does.
    let received = DelegationRepo::lock_received(&mut *conn(&pool).await, delegate, d.id)
        .await
        .unwrap_or_else(|e| unreachable!("lock received: {e}"));
    assert_eq!(received.id, d.id);
    let result = DelegationRepo::lock_received(&mut *conn(&pool).await, other, d.id).await;
    assert!(matches!(result, Err(DataError::NotFound)), "{result:?}");
}

#[sqlx::test]
async fn list_granted_hides_ended_unless_asked(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let first = seed_user(&pool, "active").await;
    let second = seed_user(&pool, "disabled").await;
    let ended = grant(&pool, owner, first, AccountLimit::All).await;
    DelegationRepo::end(
        &mut *conn(&pool).await,
        ended.id,
        EndedReason::Left,
        None,
        now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("end: {e}"));
    let live = grant(&pool, owner, second, AccountLimit::All).await;
    // Another owner's grant never shows.
    let stranger = seed_user(&pool, "active").await;
    grant(&pool, stranger, owner, AccountLimit::All).await;

    let scope = OwnerScope::own(owner);
    let shown = DelegationRepo::list_granted(&mut *conn(&pool).await, &scope, false, page())
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    assert_eq!(shown.total, 1);
    assert_eq!(
        shown
            .data
            .iter()
            .map(|v| v.delegation.id)
            .collect::<Vec<_>>(),
        vec![live.id]
    );
    assert_eq!(shown.data[0].other_display_name, "Test User");
    assert_eq!(shown.data[0].other_status, UserStatus::Disabled);

    let all = DelegationRepo::list_granted(&mut *conn(&pool).await, &scope, true, page())
        .await
        .unwrap_or_else(|e| unreachable!("list all: {e}"));
    assert_eq!(all.total, 2);
    assert_eq!(
        all.data.iter().map(|v| v.delegation.id).collect::<Vec<_>>(),
        vec![live.id, ended.id]
    );
    assert_eq!(all.data[1].delegation.ended_reason, Some(EndedReason::Left));
}

#[sqlx::test]
async fn list_received_shows_only_live_grants_with_owner_name_and_status(pool: PgPool) {
    let delegate = seed_user(&pool, "active").await;
    let active_owner = seed_user(&pool, "active").await;
    let disabled_owner = seed_user(&pool, "disabled").await;
    let ended_owner = seed_user(&pool, "active").await;
    sqlx::query("UPDATE users SET display_name = 'Owner A' WHERE id = $1")
        .bind(active_owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("rename: {e}"));

    let a = grant(&pool, active_owner, delegate, AccountLimit::All).await;
    let b = grant(&pool, disabled_owner, delegate, AccountLimit::only([])).await;
    let ended = grant(&pool, ended_owner, delegate, AccountLimit::All).await;
    DelegationRepo::end(
        &mut *conn(&pool).await,
        ended.id,
        EndedReason::Revoked,
        Some(ended_owner),
        now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("end: {e}"));
    // A grant the delegate gave away is not a received one.
    grant(&pool, delegate, active_owner, AccountLimit::All).await;

    let received = DelegationRepo::list_received(&mut *conn(&pool).await, delegate, page())
        .await
        .unwrap_or_else(|e| unreachable!("received: {e}"));

    assert_eq!(received.total, 2);
    let ids: Vec<_> = received.data.iter().map(|v| v.delegation.id).collect();
    assert_eq!(ids, vec![b.id, a.id]);
    assert_eq!(received.data[0].other_status, UserStatus::Disabled);
    assert_eq!(received.data[0].delegation.accounts, AccountLimit::only([]));
    assert_eq!(received.data[1].other_display_name, "Owner A");
    assert_eq!(received.data[1].other_status, UserStatus::Active);
}

#[sqlx::test]
async fn update_on_ended_grant_is_not_found(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let d = grant(&pool, owner, delegate, AccountLimit::All).await;
    DelegationRepo::end(
        &mut *conn(&pool).await,
        d.id,
        EndedReason::Revoked,
        Some(owner),
        now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("end: {e}"));

    let result = DelegationRepo::update(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        d.id,
        DelegationChange {
            level: Some(Level::Publish),
            accounts: None,
            expires_at: None,
        },
    )
    .await;

    assert!(matches!(result, Err(DataError::NotFound)), "{result:?}");
}

#[sqlx::test]
async fn deleting_the_revoker_keeps_the_grant_and_nulls_revoked_by(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let revoker = seed_user(&pool, "active").await;
    let d = grant(&pool, owner, delegate, AccountLimit::All).await;
    DelegationRepo::end(
        &mut *conn(&pool).await,
        d.id,
        EndedReason::Revoked,
        Some(revoker),
        now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("end: {e}"));

    assert!(
        UsersRepo::delete(&mut *conn(&pool).await, revoker)
            .await
            .unwrap_or_else(|e| unreachable!("delete revoker: {e}"))
    );

    let ended = DelegationRepo::lock(&mut *conn(&pool).await, &OwnerScope::own(owner), d.id)
        .await
        .unwrap_or_else(|e| unreachable!("lock: {e}"));
    assert_eq!(ended.revoked_by, None);
    assert_eq!(ended.ended_reason, Some(EndedReason::Revoked));
}

#[sqlx::test]
async fn deleting_owner_or_delegate_cascades_grants(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, other, "b").await;
    let by_owner = grant(&pool, owner, delegate, AccountLimit::only([a])).await;
    let to_owner = grant(&pool, other, owner, AccountLimit::All).await;
    let unrelated = grant(&pool, other, delegate, AccountLimit::only([b])).await;
    assert_eq!(grant_and_links(&pool, by_owner.id).await, (1, 1));

    // Deleting the owner removes the grants they gave, with their account rows, and the
    // grants they hold as a delegate; other owners' grants stay.
    assert!(
        UsersRepo::delete(&mut *conn(&pool).await, owner)
            .await
            .unwrap_or_else(|e| unreachable!("delete owner: {e}"))
    );
    assert_eq!(grant_and_links(&pool, by_owner.id).await, (0, 0));
    assert_eq!(grant_and_links(&pool, to_owner.id).await, (0, 0));
    assert_eq!(grant_and_links(&pool, unrelated.id).await, (1, 1));

    // Deleting the delegate removes the grants they hold, with their account rows.
    assert!(
        UsersRepo::delete(&mut *conn(&pool).await, delegate)
            .await
            .unwrap_or_else(|e| unreachable!("delete delegate: {e}"))
    );
    assert_eq!(grant_and_links(&pool, unrelated.id).await, (0, 0));
    let left = count(
        &pool,
        "SELECT count(*) FROM connected_accounts WHERE owner_id = $1",
        other.as_uuid(),
    )
    .await;
    assert_eq!(left, 1);
}

#[test]
fn levels_and_reasons_round_trip() {
    for level in [Level::View, Level::Edit, Level::Publish] {
        assert_eq!(level.as_str().parse::<Level>().ok(), Some(level));
    }
    assert!("admin".parse::<Level>().is_err());
    for reason in [
        EndedReason::Revoked,
        EndedReason::Left,
        EndedReason::Expired,
    ] {
        assert_eq!(reason.as_str().parse::<EndedReason>().ok(), Some(reason));
    }
    assert_eq!(EndedReason::Expired.as_str(), "expired");
}

async fn resolve(
    pool: &PgPool,
    actor: UserId,
    owner: UserId,
    at: DateTime<Utc>,
) -> Option<OwnerScope> {
    DelegationRepo::resolve(&mut *conn(pool).await, actor, owner, at)
        .await
        .unwrap_or_else(|e| unreachable!("resolve: {e}"))
}

/// The grant, level, and limit a resolved scope carries.
fn access(scope: &OwnerScope) -> (DelegationId, Level, AccountLimit) {
    match &scope.access {
        Access::Delegated {
            delegation,
            level,
            accounts,
        } => (*delegation, *level, accounts.clone()),
        Access::Owner => unreachable!("resolved to the owner's own access"),
    }
}

async fn set_status(pool: &PgPool, user: UserId, status: &str) {
    sqlx::query("UPDATE users SET status = $2 WHERE id = $1")
        .bind(user.as_uuid())
        .bind(status)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("set status: {e}"));
}

async fn grant_expiring(
    pool: &PgPool,
    owner: UserId,
    delegate: UserId,
    expires_at: DateTime<Utc>,
) -> Delegation {
    let mut input = new(delegate, Level::View, AccountLimit::All);
    input.expires_at = Some(expires_at);
    DelegationRepo::insert(&mut *conn(pool).await, &OwnerScope::own(owner), input)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"))
}

#[sqlx::test]
async fn resolve_valid_grant_returns_level_and_limit(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let everything = seed_user(&pool, "active").await;
    let limited = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let c = account(&pool, owner, "c").await;
    let all = grant(&pool, owner, everything, AccountLimit::All).await;
    let some = grant(&pool, owner, limited, AccountLimit::only([c, a])).await;

    let scope = resolve(&pool, everything, owner, now())
        .await
        .unwrap_or_else(|| unreachable!("all-accounts grant did not resolve"));
    assert_eq!(scope.actor, everything);
    assert_eq!(scope.owner, owner);
    assert_eq!(access(&scope), (all.id, Level::Edit, AccountLimit::All));

    let scope = resolve(&pool, limited, owner, now())
        .await
        .unwrap_or_else(|| unreachable!("limited grant did not resolve"));
    let mut sorted = vec![a, c];
    sorted.sort_by_key(AccountId::as_uuid);
    let (id, level, accounts) = access(&scope);
    assert_eq!((id, level), (some.id, Level::Edit));
    let AccountLimit::Only(ids) = accounts else {
        unreachable!("limited grant resolved to every account");
    };
    assert_eq!(ids.to_vec(), sorted);
    assert!(!ids.contains(&b));
}

#[sqlx::test]
async fn resolve_none_for_revoked_left_and_expired(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let revoked = seed_user(&pool, "active").await;
    let left = seed_user(&pool, "active").await;
    let expired = seed_user(&pool, "active").await;
    let at = now();
    for (delegate, reason) in [(revoked, EndedReason::Revoked), (left, EndedReason::Left)] {
        let d = grant(&pool, owner, delegate, AccountLimit::All).await;
        assert!(resolve(&pool, delegate, owner, at).await.is_some());
        DelegationRepo::end(&mut *conn(&pool).await, d.id, reason, None, at)
            .await
            .unwrap_or_else(|e| unreachable!("end: {e}"));
        assert!(
            resolve(&pool, delegate, owner, at).await.is_none(),
            "{reason:?}"
        );
    }

    let expires = at - Duration::seconds(1);
    let d = grant_expiring(&pool, owner, expired, expires).await;
    assert_eq!(d.ended_at, None);
    assert!(resolve(&pool, expired, owner, at).await.is_none());
    // Live up to its expiry, not at it.
    assert!(
        resolve(&pool, expired, owner, expires - Duration::seconds(1))
            .await
            .is_some()
    );
    assert!(resolve(&pool, expired, owner, expires).await.is_none());
}

#[sqlx::test]
async fn resolve_none_when_owner_or_delegate_disabled_pending_or_deleting(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    grant(&pool, owner, delegate, AccountLimit::All).await;

    for status in ["disabled", "pending", "deleting"] {
        for user in [owner, delegate] {
            set_status(&pool, user, status).await;
            assert!(
                resolve(&pool, delegate, owner, now()).await.is_none(),
                "{status}"
            );
            set_status(&pool, user, "active").await;
            assert!(
                resolve(&pool, delegate, owner, now()).await.is_some(),
                "{status} restored"
            );
        }
    }
}

#[sqlx::test]
async fn resolve_none_for_self_and_unknown_owner(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    grant(&pool, owner, delegate, AccountLimit::All).await;

    assert!(resolve(&pool, owner, owner, now()).await.is_none());
    assert!(
        resolve(&pool, delegate, UserId::from(Uuid::now_v7()), now())
            .await
            .is_none()
    );
    // A grant runs one way only.
    assert!(resolve(&pool, owner, delegate, now()).await.is_none());
}

#[sqlx::test]
async fn resolve_reflects_level_change_immediately(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let d = grant(&pool, owner, delegate, AccountLimit::All).await;

    DelegationRepo::update(
        &mut *conn(&pool).await,
        &OwnerScope::own(owner),
        d.id,
        DelegationChange {
            level: Some(Level::View),
            accounts: Some(AccountLimit::only([a])),
            expires_at: None,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("update: {e}"));

    let scope = resolve(&pool, delegate, owner, now())
        .await
        .unwrap_or_else(|| unreachable!("changed grant did not resolve"));
    assert_eq!(access(&scope), (d.id, Level::View, AccountLimit::only([a])));
}

#[sqlx::test]
async fn resolve_limited_grant_with_no_accounts_is_empty_only(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let delegate = seed_user(&pool, "active").await;
    account(&pool, owner, "a").await;
    let d = grant(&pool, owner, delegate, AccountLimit::only([])).await;

    let scope = resolve(&pool, delegate, owner, now())
        .await
        .unwrap_or_else(|| unreachable!("empty limit did not resolve"));
    assert_eq!(access(&scope), (d.id, Level::Edit, AccountLimit::only([])));
}

#[sqlx::test]
async fn take_expired_ends_due_grants_once(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let at = now();
    let mut due = Vec::new();
    for minutes in [3, 2, 1] {
        let delegate = seed_user(&pool, "active").await;
        due.push(
            grant_expiring(&pool, owner, delegate, at - Duration::minutes(minutes))
                .await
                .id,
        );
    }
    let at_now = grant_expiring(&pool, owner, seed_user(&pool, "active").await, at).await;
    due.push(at_now.id);
    let future = grant_expiring(
        &pool,
        owner,
        seed_user(&pool, "active").await,
        at + Duration::minutes(1),
    )
    .await;
    let forever = grant(
        &pool,
        owner,
        seed_user(&pool, "active").await,
        AccountLimit::All,
    )
    .await;

    // The first call holds its batch locked while the second runs on another connection: the
    // second skips those rows instead of waiting for them or ending them again.
    let mut first = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    let taken_first = DelegationRepo::take_expired(&mut first, at, 2)
        .await
        .unwrap_or_else(|e| unreachable!("first take: {e}"));
    let taken_second = DelegationRepo::take_expired(&mut *conn(&pool).await, at, 10)
        .await
        .unwrap_or_else(|e| unreachable!("second take: {e}"));
    first
        .commit()
        .await
        .unwrap_or_else(|e| unreachable!("commit: {e}"));

    // The oldest expiries go first.
    assert_eq!(
        taken_first.iter().map(|d| d.id).collect::<Vec<_>>(),
        due[..2].to_vec()
    );
    let mut ended: Vec<DelegationId> = taken_first
        .iter()
        .chain(&taken_second)
        .map(|d| d.id)
        .collect();
    ended.sort_by_key(DelegationId::as_uuid);
    let mut expected = due.clone();
    expected.sort_by_key(DelegationId::as_uuid);
    assert_eq!(ended, expected);
    for d in taken_first.iter().chain(&taken_second) {
        assert_eq!(
            d.ended_at.map(|t| t.timestamp_micros()),
            Some(at.timestamp_micros())
        );
        assert_eq!(d.ended_reason, Some(EndedReason::Expired));
        assert_eq!(d.revoked_by, None);
    }

    let again = DelegationRepo::take_expired(&mut *conn(&pool).await, at, 10)
        .await
        .unwrap_or_else(|e| unreachable!("third take: {e}"));
    assert!(again.is_empty(), "{again:?}");
    let owner_scope = OwnerScope::own(owner);
    for id in [future.id, forever.id] {
        let untouched = DelegationRepo::lock(&mut *conn(&pool).await, &owner_scope, id)
            .await
            .unwrap_or_else(|e| unreachable!("lock: {e}"));
        assert_eq!(untouched.ended_at, None);
    }
}
