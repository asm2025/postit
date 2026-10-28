use chrono::{DateTime, Duration, Utc};
use emixdb::dto::Pagination;
use postit_core::{AccountId, AuditEventId, DelegationId, GrantId, PlatformId, UserId};
use postit_data::accounts::{
    Account, AccountRepo, AccountStatus, AccountTokenRepo, AccountUpsert, GrantKind, GrantRepo,
    GrantStatus, NewAccount, NewGrant, RemoteAccount,
};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::delegations::DelegationRepo;
use postit_data::testkit::{self, Callers, callers, seed_user};
use postit_data::{Access, AccountLimit, DataError, Level, OwnerScope, ScopeError};
use postit_vault::Vault;
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())])
        .unwrap_or_else(|e| unreachable!("vault: {e}"))
}

fn platform(id: &str) -> PlatformId {
    PlatformId::new(id).unwrap_or_else(|e| unreachable!("platform id: {e}"))
}

async fn conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    pool.acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"))
}

async fn platforms(pool: &PgPool) {
    for id in ["email", "mastodon"] {
        sqlx::query("INSERT INTO platforms (id, display_name) VALUES ($1, $1)")
            .bind(id)
            .execute(pool)
            .await
            .unwrap_or_else(|e| unreachable!("platform: {e}"));
    }
}

async fn grant(conn: &mut PgConnection, scope: &OwnerScope, platform_id: &str) -> GrantId {
    let id = GrantId::from(Uuid::now_v7());
    let credential = vault()
        .seal(&json!({"token": "secret"}), &GrantRepo::seal_context(id))
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    GrantRepo::insert(
        conn,
        scope,
        NewGrant {
            id,
            platform: platform(platform_id),
            kind: GrantKind::Credentials,
            remote_user_id: None,
            credential,
            connection_config: json!({}),
            expires_at: None,
            reconnect_by: None,
            scopes: vec![],
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("grant: {e}"));
    id
}

fn new_account(grant: GrantId, platform_id: &str, remote: &str) -> NewAccount {
    NewAccount {
        id: AccountId::from(Uuid::now_v7()),
        grant_id: grant,
        platform: platform(platform_id),
        remote_account_id: remote.to_string(),
        display_name: remote.to_string(),
        avatar_url: None,
        scopes: vec!["write".to_string()],
        test_account: false,
    }
}

async fn connect(
    conn: &mut PgConnection,
    scope: &OwnerScope,
    grant: GrantId,
    platform_id: &str,
    remote: &str,
) -> Account {
    AccountRepo::insert_or_reactivate(conn, scope, new_account(grant, platform_id, remote))
        .await
        .unwrap_or_else(|e| unreachable!("connect {remote}: {e}"))
        .into_account()
}

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

async fn owner_with_callers(
    pool: &PgPool,
    visible: &[AccountId],
) -> (OwnerScope, Callers, GrantId) {
    platforms(pool).await;
    let owner = seed_user(pool, "active").await;
    let callers = callers(pool, owner, visible).await;
    let mut c = conn(pool).await;
    let grant = grant(&mut c, &callers.owner, "mastodon").await;
    (OwnerScope::own(owner), callers, grant)
}

#[sqlx::test]
async fn isolation_matrix(pool: PgPool) {
    platforms(&pool).await;
    let owner_id = seed_user(&pool, "active").await;
    let owner = OwnerScope::own(owner_id);
    let mut c = conn(&pool).await;
    let g = grant(&mut c, &owner, "mastodon").await;
    let a_id = connect(&mut c, &owner, g, "mastodon", "a").await.id;
    let b = connect(&mut c, &owner, g, "mastodon", "b").await;
    // The limited caller's grant names account A, so the accounts exist first.
    let who = callers(&pool, owner_id, &[a_id]).await;

    for (name, scope) in [
        ("owner", &who.owner),
        ("view", &who.view),
        ("edit", &who.edit),
        ("publish", &who.publish),
    ] {
        let listed = AccountRepo::list(&mut c, scope, page(), None, false)
            .await
            .unwrap_or_else(|e| unreachable!("{name} list: {e}"));
        assert_eq!(listed.total, 2, "{name}");
        AccountRepo::get(&mut c, scope, b.id)
            .await
            .unwrap_or_else(|e| unreachable!("{name} get: {e}"));
    }

    let listed = AccountRepo::list(&mut c, &who.limited, page(), None, false)
        .await
        .unwrap_or_else(|e| unreachable!("limited list: {e}"));
    assert_eq!(listed.total, 1);
    assert_eq!(listed.data[0].id, a_id);
    assert!(matches!(
        AccountRepo::get(&mut c, &who.limited, b.id).await,
        Err(DataError::NotFound)
    ));

    let stranger = AccountRepo::list(&mut c, &who.stranger, page(), None, false)
        .await
        .unwrap_or_else(|e| unreachable!("stranger list: {e}"));
    assert_eq!(stranger.total, 0);
    assert!(matches!(
        AccountRepo::get(&mut c, &who.stranger, b.id).await,
        Err(DataError::NotFound)
    ));

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let refused = AccountRepo::insert_or_reactivate(
            &mut c,
            scope,
            new_account(g, "mastodon", "from-delegate"),
        )
        .await;
        assert!(matches!(
            refused,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            AccountRepo::disconnect(&mut c, scope, b.id).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            GrantRepo::set_status(&mut c, scope, g, GrantStatus::Revoked).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        GrantRepo::get(&mut c, scope, g)
            .await
            .unwrap_or_else(|e| unreachable!("delegate grant read: {e}"));
    }
    assert!(matches!(
        GrantRepo::get(&mut c, &who.stranger, g).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn empty_limit_sees_no_accounts(pool: PgPool) {
    let (owner, who, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;

    let listed = AccountRepo::list(&mut c, &who.limited, page(), None, false)
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    assert_eq!(listed.total, 0);
    assert!(listed.data.is_empty());
    assert!(matches!(
        AccountRepo::get(&mut c, &who.limited, a.id).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn connected_elsewhere_hides_owner(pool: PgPool) {
    let (owner1, _, g1) = owner_with_callers(&pool, &[]).await;
    let owner2 = OwnerScope::own(seed_user(&pool, "active").await);
    let mut c = conn(&pool).await;
    let g2 = grant(&mut c, &owner2, "mastodon").await;
    connect(&mut c, &owner1, g1, "mastodon", "@a@example.social").await;

    let err = AccountRepo::insert_or_reactivate(
        &mut c,
        &owner2,
        new_account(g2, "mastodon", "@a@example.social"),
    )
    .await
    .err()
    .unwrap_or_else(|| unreachable!("a second owner connected the same account"));

    assert!(matches!(err, DataError::AccountConnectedElsewhere));
    let shown = err.to_string();
    assert!(!shown.contains(&owner1.owner.to_string()), "{shown}");
}

#[sqlx::test]
async fn disconnected_remote_can_be_taken_by_another_owner(pool: PgPool) {
    let (owner1, _, g1) = owner_with_callers(&pool, &[]).await;
    let owner2 = OwnerScope::own(seed_user(&pool, "active").await);
    let mut c = conn(&pool).await;
    let g2 = grant(&mut c, &owner2, "mastodon").await;
    let first = connect(&mut c, &owner1, g1, "mastodon", "@a@example.social").await;
    AccountRepo::disconnect(&mut c, &owner1, first.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    let taken = AccountRepo::insert_or_reactivate(
        &mut c,
        &owner2,
        new_account(g2, "mastodon", "@a@example.social"),
    )
    .await
    .unwrap_or_else(|e| unreachable!("take over: {e}"));

    assert!(matches!(taken, AccountUpsert::Created(_)));
    let old = AccountRepo::get(&mut c, &owner1, first.id)
        .await
        .unwrap_or_else(|e| unreachable!("old row: {e}"));
    assert_eq!(old.status, AccountStatus::Disconnected);
    assert_eq!(old.grant_id, None);
}

#[sqlx::test]
async fn same_owner_reconnect_reactivates(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let first = connect(&mut c, &owner, g, "mastodon", "a").await;
    AccountRepo::disconnect(&mut c, &owner, first.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));
    let g2 = grant(&mut c, &owner, "mastodon").await;

    let again = AccountRepo::insert_or_reactivate(&mut c, &owner, new_account(g2, "mastodon", "a"))
        .await
        .unwrap_or_else(|e| unreachable!("reconnect: {e}"));

    let AccountUpsert::Reactivated(account) = again else {
        unreachable!("expected Reactivated");
    };
    assert_eq!(account.id, first.id);
    assert_eq!(account.status, AccountStatus::Active);
    assert_eq!(account.grant_id, Some(g2));
}

#[sqlx::test]
async fn active_account_on_a_new_grant_is_moved(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let first = connect(&mut c, &owner, g, "mastodon", "a").await;
    let g2 = grant(&mut c, &owner, "mastodon").await;

    let again = AccountRepo::insert_or_reactivate(&mut c, &owner, new_account(g2, "mastodon", "a"))
        .await
        .unwrap_or_else(|e| unreachable!("move: {e}"));

    let AccountUpsert::Moved { account, .. } = again else {
        unreachable!("expected Moved");
    };
    assert_eq!(account.id, first.id);
    assert_eq!(account.grant_id, Some(g2));
    assert!(
        GrantRepo::delete_if_unused(&mut c, &owner, g)
            .await
            .unwrap_or_else(|e| unreachable!("delete old grant: {e}"))
    );
}

#[sqlx::test]
async fn one_email_account_per_owner(pool: PgPool) {
    platforms(&pool).await;
    let owner = OwnerScope::own(seed_user(&pool, "active").await);
    let mut c = conn(&pool).await;
    let g = grant(&mut c, &owner, "email").await;
    connect(&mut c, &owner, g, "email", "smtp-a").await;

    let second =
        AccountRepo::insert_or_reactivate(&mut c, &owner, new_account(g, "email", "smtp-b")).await;

    assert!(matches!(second, Err(DataError::SendingMethodExists)));
}

#[sqlx::test]
async fn grant_status_spreads_to_accounts(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    let b = connect(&mut c, &owner, g, "mastodon", "b").await;
    let gone = connect(&mut c, &owner, g, "mastodon", "gone").await;
    AccountRepo::disconnect(&mut c, &owner, gone.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    GrantRepo::set_status(&mut c, &owner, g, GrantStatus::NeedsReconnect)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    for id in [a.id, b.id] {
        let account = AccountRepo::get(&mut c, &owner, id)
            .await
            .unwrap_or_else(|e| unreachable!("get: {e}"));
        assert_eq!(account.status, AccountStatus::NeedsReconnect);
    }
    let gone = AccountRepo::get(&mut c, &owner, gone.id)
        .await
        .unwrap_or_else(|e| unreachable!("get gone: {e}"));
    assert_eq!(gone.status, AccountStatus::Disconnected);
    let stored = GrantRepo::get(&mut c, &owner, g)
        .await
        .unwrap_or_else(|e| unreachable!("get grant: {e}"));
    assert_eq!(stored.status, GrantStatus::NeedsReconnect);
}

#[sqlx::test]
async fn disconnect_cleans_links_and_pauses_empty_recurrence(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    let b = connect(&mut c, &owner, g, "mastodon", "b").await;
    let owner_id = owner.owner.as_uuid();
    let (post, group, only_a, a_and_b) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    let other_post = Uuid::now_v7();
    for id in [post, other_post] {
        sqlx::query("INSERT INTO posts (id, owner_id) VALUES ($1, $2)")
            .bind(id)
            .bind(owner_id)
            .execute(&mut *c)
            .await
            .unwrap_or_else(|e| unreachable!("post: {e}"));
    }
    sqlx::query("INSERT INTO post_overrides (post_id, account_id, owner_id) VALUES ($1, $2, $3)")
        .bind(post)
        .bind(a.id.as_uuid())
        .bind(owner_id)
        .execute(&mut *c)
        .await
        .unwrap_or_else(|e| unreachable!("override: {e}"));
    sqlx::query("INSERT INTO post_targets (post_id, account_id, owner_id) VALUES ($1, $2, $3)")
        .bind(post)
        .bind(a.id.as_uuid())
        .bind(owner_id)
        .execute(&mut *c)
        .await
        .unwrap_or_else(|e| unreachable!("target: {e}"));
    sqlx::query("INSERT INTO target_groups (id, owner_id, name) VALUES ($1, $2, 'All')")
        .bind(group)
        .bind(owner_id)
        .execute(&mut *c)
        .await
        .unwrap_or_else(|e| unreachable!("group: {e}"));
    for account in [a.id, b.id] {
        sqlx::query(
            "INSERT INTO target_group_accounts (group_id, account_id, owner_id) VALUES ($1, $2, $3)",
        )
        .bind(group)
        .bind(account.as_uuid())
        .bind(owner_id)
        .execute(&mut *c)
        .await
        .unwrap_or_else(|e| unreachable!("group member: {e}"));
    }
    for (recurrence, on_post, accounts) in [
        (only_a, post, vec![a.id]),
        (a_and_b, other_post, vec![a.id, b.id]),
    ] {
        sqlx::query(
            "INSERT INTO recurrences (id, owner_id, post_id, rule, timezone)
             VALUES ($1, $2, $3, 'FREQ=DAILY', 'UTC')",
        )
        .bind(recurrence)
        .bind(owner_id)
        .bind(on_post)
        .execute(&mut *c)
        .await
        .unwrap_or_else(|e| unreachable!("recurrence: {e}"));
        for account in accounts {
            sqlx::query(
                "INSERT INTO recurrence_targets (recurrence_id, account_id, owner_id)
                 VALUES ($1, $2, $3)",
            )
            .bind(recurrence)
            .bind(account.as_uuid())
            .bind(owner_id)
            .execute(&mut *c)
            .await
            .unwrap_or_else(|e| unreachable!("recurrence target: {e}"));
        }
    }
    let token = vault()
        .seal(&json!({"t": 1}), &AccountTokenRepo::seal_context(a.id))
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    AccountTokenRepo::upsert(&mut c, &owner, a.id, &token, None)
        .await
        .unwrap_or_else(|e| unreachable!("token: {e}"));

    let done = AccountRepo::disconnect(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    assert_eq!(done.paused_recurrences.len(), 1);
    assert_eq!(done.paused_recurrences[0].as_uuid(), only_a);
    assert!(!done.grant_now_unused, "account b still uses the grant");
    let left: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM post_overrides WHERE account_id = $1),
                (SELECT COUNT(*) FROM post_targets WHERE account_id = $1),
                (SELECT COUNT(*) FROM account_tokens WHERE account_id = $1),
                (SELECT COUNT(*) FROM recurrence_targets WHERE account_id = $1),
                (SELECT COUNT(*) FROM target_group_accounts WHERE account_id = $1)",
    )
    .bind(a.id.as_uuid())
    .fetch_one(&mut *c)
    .await
    .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(left, (0, 0, 0, 0, 0));
    let state = |id: Uuid| {
        sqlx::query_as::<_, (bool, Option<String>)>(
            "SELECT paused, paused_reason FROM recurrences WHERE id = $1",
        )
        .bind(id)
    };
    assert_eq!(
        state(only_a)
            .fetch_one(&mut *c)
            .await
            .unwrap_or_else(|e| unreachable!("state: {e}")),
        (true, Some("no_targets".to_string()))
    );
    assert_eq!(
        state(a_and_b)
            .fetch_one(&mut *c)
            .await
            .unwrap_or_else(|e| unreachable!("state: {e}")),
        (false, None)
    );
    let account = AccountRepo::get(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));
    assert_eq!(account.status, AccountStatus::Disconnected);
}

/// The account limit of the owner's grant `id` as stored, and whether it still resolves for
/// `delegate` with that same limit.
async fn limit_of(
    c: &mut PgConnection,
    owner: &OwnerScope,
    delegate: UserId,
    id: DelegationId,
) -> AccountLimit {
    let stored = DelegationRepo::lock(c, owner, id)
        .await
        .unwrap_or_else(|e| unreachable!("lock grant: {e}"));
    assert!(stored.is_live(Utc::now()), "the grant ended");
    let resolved = DelegationRepo::resolve(c, delegate, owner.owner, Utc::now())
        .await
        .unwrap_or_else(|e| unreachable!("resolve: {e}"))
        .unwrap_or_else(|| unreachable!("the grant no longer resolves"));
    let Access::Delegated { accounts, .. } = resolved.access else {
        unreachable!("resolved to the owner's own access");
    };
    assert_eq!(accounts, stored.accounts);
    stored.accounts
}

#[sqlx::test]
async fn disconnect_removes_account_from_every_limit(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    let b = connect(&mut c, &owner, g, "mastodon", "b").await;
    let first = seed_user(&pool, "active").await;
    let second = seed_user(&pool, "active").await;
    let only_a = testkit::grant(
        &pool,
        owner.owner,
        first,
        Level::Edit,
        AccountLimit::only([a.id]),
    )
    .await;
    let a_and_b = testkit::grant(
        &pool,
        owner.owner,
        second,
        Level::View,
        AccountLimit::only([a.id, b.id]),
    )
    .await;

    AccountRepo::disconnect(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    assert_eq!(
        limit_of(&mut c, &owner, first, only_a).await,
        AccountLimit::only([])
    );
    assert_eq!(
        limit_of(&mut c, &owner, second, a_and_b).await,
        AccountLimit::only([b.id])
    );
}

#[sqlx::test]
async fn reconnect_does_not_restore_limit(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    let delegate = seed_user(&pool, "active").await;
    let d = testkit::grant(
        &pool,
        owner.owner,
        delegate,
        Level::Publish,
        AccountLimit::only([a.id]),
    )
    .await;
    AccountRepo::disconnect(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    let again = connect(&mut c, &owner, g, "mastodon", "a").await;

    assert_eq!(again.id, a.id, "reconnecting reuses the account ID");
    assert_eq!(again.status, AccountStatus::Active);
    assert_eq!(
        limit_of(&mut c, &owner, delegate, d).await,
        AccountLimit::only([])
    );
}

#[sqlx::test]
async fn grant_shared_by_two_accounts_reports_unused_only_after_last(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    let b = connect(&mut c, &owner, g, "mastodon", "b").await;
    assert_eq!(
        GrantRepo::account_count(&mut c, &owner, g)
            .await
            .unwrap_or_else(|e| unreachable!("count: {e}")),
        2
    );

    let first = AccountRepo::disconnect(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect a: {e}"));
    assert!(!first.grant_now_unused);
    assert!(
        !GrantRepo::delete_if_unused(&mut c, &owner, g)
            .await
            .unwrap_or_else(|e| unreachable!("delete early: {e}"))
    );

    let last = AccountRepo::disconnect(&mut c, &owner, b.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect b: {e}"));
    assert!(last.grant_now_unused);
    assert!(
        GrantRepo::delete_if_unused(&mut c, &owner, g)
            .await
            .unwrap_or_else(|e| unreachable!("delete: {e}"))
    );
    assert!(matches!(
        GrantRepo::get(&mut c, &owner, g).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn token_round_trips_sealed_and_respects_the_limit(pool: PgPool) {
    platforms(&pool).await;
    let owner_id = seed_user(&pool, "active").await;
    let hidden = AccountId::from(Uuid::now_v7());
    let who = callers(&pool, owner_id, &[]).await;
    let mut c = conn(&pool).await;
    let g = grant(&mut c, &who.owner, "mastodon").await;
    let mut new = new_account(g, "mastodon", "a");
    new.id = hidden;
    AccountRepo::insert_or_reactivate(&mut c, &who.owner, new)
        .await
        .unwrap_or_else(|e| unreachable!("connect: {e}"));
    let expires = Utc::now();
    let sealed = vault()
        .seal(
            &json!({"access": "x"}),
            &AccountTokenRepo::seal_context(hidden),
        )
        .unwrap_or_else(|e| unreachable!("seal: {e}"));

    AccountTokenRepo::upsert(&mut c, &who.owner, hidden, &sealed, Some(expires))
        .await
        .unwrap_or_else(|e| unreachable!("upsert: {e}"));
    let replacement = vault()
        .seal(
            &json!({"access": "y"}),
            &AccountTokenRepo::seal_context(hidden),
        )
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    AccountTokenRepo::upsert(&mut c, &who.owner, hidden, &replacement, None)
        .await
        .unwrap_or_else(|e| unreachable!("replace: {e}"));

    let (stored, at) = AccountTokenRepo::get(&mut c, &who.owner, hidden)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"))
        .unwrap_or_else(|| unreachable!("token should exist"));
    assert_eq!(stored, replacement);
    assert_eq!(at, None);
    let opened: serde_json::Value = vault()
        .open(&stored, &AccountTokenRepo::seal_context(hidden))
        .unwrap_or_else(|e| unreachable!("open: {e}"));
    assert_eq!(opened, json!({"access": "y"}));
    assert!(
        AccountTokenRepo::get(&mut c, &who.limited, hidden)
            .await
            .unwrap_or_else(|e| unreachable!("limited get: {e}"))
            .is_none()
    );
    let missing = AccountTokenRepo::upsert(
        &mut c,
        &who.owner,
        AccountId::from(Uuid::now_v7()),
        &replacement,
        None,
    )
    .await;
    assert!(matches!(missing, Err(DataError::NotFound)));
}

#[sqlx::test]
async fn mark_checked_and_set_status_write_through(pool: PgPool) {
    let (owner, who, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    let at = Utc::now();

    AccountRepo::mark_checked(&mut c, &owner, a.id, at)
        .await
        .unwrap_or_else(|e| unreachable!("mark_checked: {e}"));
    AccountRepo::set_status(&mut c, &owner, a.id, AccountStatus::Revoked)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    let stored = AccountRepo::get(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));
    assert_eq!(stored.status, AccountStatus::Revoked);
    assert!(stored.last_checked_at.is_some());
    assert!(matches!(
        AccountRepo::set_status(&mut c, &owner, a.id, AccountStatus::Disconnected).await,
        Err(DataError::Conflict(_))
    ));
    assert!(matches!(
        AccountRepo::set_status(&mut c, &who.edit, a.id, AccountStatus::Active).await,
        Err(DataError::Scope(ScopeError::OwnerOnly))
    ));
}

#[sqlx::test]
async fn reconnect_keeps_display_name_and_test_flag(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let first = connect(&mut c, &owner, g, "mastodon", "a").await;
    AccountRepo::update_settings(
        &mut c,
        &owner,
        first.id,
        Some(true),
        Some("My account".to_string()),
    )
    .await
    .unwrap_or_else(|e| unreachable!("update: {e}"));
    let g2 = grant(&mut c, &owner, "mastodon").await;

    let again = AccountRepo::insert_or_reactivate(&mut c, &owner, new_account(g2, "mastodon", "a"))
        .await
        .unwrap_or_else(|e| unreachable!("move: {e}"));

    let AccountUpsert::Moved {
        account,
        previous_grant,
    } = again
    else {
        unreachable!("expected Moved");
    };
    assert_eq!(previous_grant, g);
    assert_eq!(account.display_name, "My account");
    assert!(account.test_account);
    assert_eq!(account.grant_id, Some(g2));
}

#[sqlx::test]
async fn update_settings_changes_name_and_flag(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let first = connect(&mut c, &owner, g, "mastodon", "a").await;

    let only_flag = AccountRepo::update_settings(&mut c, &owner, first.id, Some(true), None)
        .await
        .unwrap_or_else(|e| unreachable!("flag: {e}"));
    assert!(only_flag.test_account);
    assert_eq!(only_flag.display_name, "a");

    let both = AccountRepo::update_settings(&mut c, &owner, first.id, None, Some("B".to_string()))
        .await
        .unwrap_or_else(|e| unreachable!("name: {e}"));
    assert!(both.test_account);
    assert_eq!(both.display_name, "B");
}

#[sqlx::test]
async fn update_settings_refuses_disconnected(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let first = connect(&mut c, &owner, g, "mastodon", "a").await;
    AccountRepo::disconnect(&mut c, &owner, first.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    let result = AccountRepo::update_settings(&mut c, &owner, first.id, Some(true), None).await;

    assert!(matches!(result, Err(DataError::NotFound)));
}

#[sqlx::test]
async fn update_settings_is_owner_only(pool: PgPool) {
    let (owner, who, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let first = connect(&mut c, &owner, g, "mastodon", "a").await;

    for scope in [&who.view, &who.edit, &who.publish, &who.limited] {
        let result = AccountRepo::update_settings(&mut c, scope, first.id, Some(true), None).await;
        assert!(matches!(
            result,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
    }
    let result =
        AccountRepo::update_settings(&mut c, &who.stranger, first.id, Some(true), None).await;
    assert!(matches!(result, Err(DataError::NotFound)));
}

#[sqlx::test]
async fn list_hides_disconnected_unless_asked(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    connect(&mut c, &owner, g, "mastodon", "b").await;
    AccountRepo::disconnect(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    let hidden = AccountRepo::list(&mut c, &owner, page(), None, false)
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    let all = AccountRepo::list(&mut c, &owner, page(), None, true)
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));

    assert_eq!((hidden.total, hidden.data.len()), (1, 1));
    assert_eq!((all.total, all.data.len()), (2, 2));
}

#[sqlx::test]
async fn ids_on_grant_skips_disconnected(pool: PgPool) {
    let (owner, _, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    let b = connect(&mut c, &owner, g, "mastodon", "b").await;
    AccountRepo::disconnect(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    let ids = AccountRepo::ids_on_grant(&mut c, &owner, g)
        .await
        .unwrap_or_else(|e| unreachable!("ids: {e}"));

    assert_eq!(ids, vec![b.id]);
}

#[sqlx::test]
async fn remote_account_tells_free_own_and_elsewhere(pool: PgPool) {
    let (owner, who, g) = owner_with_callers(&pool, &[]).await;
    let mut c = conn(&pool).await;
    let a = connect(&mut c, &owner, g, "mastodon", "a").await;
    let mastodon = platform("mastodon");

    let own = AccountRepo::remote_account(&mut c, &owner, &mastodon, "a")
        .await
        .unwrap_or_else(|e| unreachable!("own: {e}"));
    let free = AccountRepo::remote_account(&mut c, &owner, &mastodon, "zzz")
        .await
        .unwrap_or_else(|e| unreachable!("free: {e}"));
    // The stranger sees the owner's account only as "elsewhere".
    let elsewhere = AccountRepo::remote_account(&mut c, &who.stranger, &mastodon, "a")
        .await
        .unwrap_or_else(|e| unreachable!("elsewhere: {e}"));
    AccountRepo::disconnect(&mut c, &owner, a.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));
    let after = AccountRepo::remote_account(&mut c, &owner, &mastodon, "a")
        .await
        .unwrap_or_else(|e| unreachable!("after: {e}"));
    let freed = AccountRepo::remote_account(&mut c, &who.stranger, &mastodon, "a")
        .await
        .unwrap_or_else(|e| unreachable!("freed: {e}"));

    assert_eq!(
        own,
        RemoteAccount::Own {
            account: a.id,
            active: true
        }
    );
    assert_eq!(free, RemoteAccount::Free);
    assert_eq!(elsewhere, RemoteAccount::Elsewhere);
    assert_eq!(
        after,
        RemoteAccount::Own {
            account: a.id,
            active: false
        }
    );
    assert_eq!(freed, RemoteAccount::Free);
    let refused = AccountRepo::remote_account(&mut c, &who.view, &mastodon, "a").await;
    assert!(matches!(
        refused,
        Err(DataError::Scope(ScopeError::OwnerOnly))
    ));
}

// ---- B7: system-side grant calls, account tokens, and the grant lock. ----

/// An OAuth grant of `owner` on mastodon for `remote_user`, expiring at `expires_at`.
async fn oauth_grant(
    conn: &mut PgConnection,
    owner: UserId,
    remote_user: Option<&str>,
    expires_at: Option<DateTime<Utc>>,
) -> GrantId {
    let id = GrantId::from(Uuid::now_v7());
    let credential = vault()
        .seal(
            &json!({"access_token": "at", "refresh_token": "rt"}),
            &GrantRepo::seal_context(id),
        )
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    GrantRepo::insert(
        conn,
        &OwnerScope::own(owner),
        NewGrant {
            id,
            platform: platform("mastodon"),
            kind: GrantKind::Oauth,
            remote_user_id: remote_user.map(str::to_owned),
            credential,
            connection_config: json!({}),
            expires_at,
            reconnect_by: None,
            scopes: vec![],
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("oauth grant: {e}"));
    id
}

/// Stores an access token on `account`.
async fn token(conn: &mut PgConnection, owner: UserId, account: AccountId) {
    let sealed = vault()
        .seal(
            &json!({"access": "x"}),
            &AccountTokenRepo::seal_context(account),
        )
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    AccountTokenRepo::upsert(conn, &OwnerScope::own(owner), account, &sealed, None)
        .await
        .unwrap_or_else(|e| unreachable!("token: {e}"));
}

async fn has_token(pool: &PgPool, account: AccountId) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM account_tokens WHERE account_id = $1)")
        .bind(account.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("token count: {e}"))
}

async fn grant_row(pool: &PgPool, id: GrantId) -> (String, bool) {
    sqlx::query_as("SELECT status, credential IS NOT NULL FROM account_grants WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("grant row: {e}"))
}

async fn account_status(pool: &PgPool, id: AccountId) -> String {
    sqlx::query_scalar("SELECT status FROM connected_accounts WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("account status: {e}"))
}

async fn begin(pool: &PgPool) -> sqlx::Transaction<'static, sqlx::Postgres> {
    pool.begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"))
}

async fn commit(tx: sqlx::Transaction<'static, sqlx::Postgres>) {
    tx.commit()
        .await
        .unwrap_or_else(|e| unreachable!("commit: {e}"));
}

#[sqlx::test]
async fn revoke_by_remote_user_clears_credentials_and_tokens(pool: PgPool) {
    platforms(&pool).await;
    let owner1 = seed_user(&pool, "active").await;
    let owner2 = seed_user(&pool, "active").await;
    let mut c = conn(&pool).await;
    let g1 = oauth_grant(&mut c, owner1, Some("mock-user"), None).await;
    let g2 = oauth_grant(&mut c, owner2, Some("mock-user"), None).await;
    let g3 = oauth_grant(&mut c, owner1, Some("other-user"), None).await;
    let a1 = connect(&mut c, &OwnerScope::own(owner1), g1, "mastodon", "a1").await;
    let a2 = connect(&mut c, &OwnerScope::own(owner2), g2, "mastodon", "a2").await;
    let a3 = connect(&mut c, &OwnerScope::own(owner1), g3, "mastodon", "a3").await;
    for (owner, account) in [(owner1, a1.id), (owner2, a2.id), (owner1, a3.id)] {
        token(&mut c, owner, account).await;
    }

    let mut tx = begin(&pool).await;
    let mut revoked = GrantRepo::revoke_by_remote_user(&mut tx, &platform("mastodon"), "mock-user")
        .await
        .unwrap_or_else(|e| unreachable!("revoke: {e}"));
    commit(tx).await;

    revoked.sort_by_key(|(_, a)| a.as_uuid());
    let mut expected = vec![(owner1, a1.id), (owner2, a2.id)];
    expected.sort_by_key(|(_, a)| a.as_uuid());
    assert_eq!(revoked, expected);
    for g in [g1, g2] {
        assert_eq!(grant_row(&pool, g).await, ("revoked".to_owned(), false));
    }
    for a in [a1.id, a2.id] {
        assert_eq!(account_status(&pool, a).await, "revoked");
        assert!(!has_token(&pool, a).await);
    }
    assert_eq!(grant_row(&pool, g3).await, ("active".to_owned(), true));
    assert_eq!(account_status(&pool, a3.id).await, "active");
    assert!(has_token(&pool, a3.id).await);
    let read = GrantRepo::get(&mut c, &OwnerScope::own(owner1), g1)
        .await
        .unwrap_or_else(|e| unreachable!("get revoked: {e}"));
    assert!(read.credential.is_none());
    assert_eq!(read.status, GrantStatus::Revoked);
}

#[sqlx::test]
async fn mark_needs_reconnect_skips_disconnected_accounts(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = conn(&pool).await;
    let g = oauth_grant(&mut c, owner, Some("mock-user"), None).await;
    let kept = connect(&mut c, &scope, g, "mastodon", "kept").await;
    let gone = connect(&mut c, &scope, g, "mastodon", "gone").await;
    AccountRepo::disconnect(&mut c, &scope, gone.id)
        .await
        .unwrap_or_else(|e| unreachable!("disconnect: {e}"));

    let mut tx = begin(&pool).await;
    let marked = GrantRepo::mark_needs_reconnect(&mut tx, g)
        .await
        .unwrap_or_else(|e| unreachable!("mark: {e}"));
    commit(tx).await;

    assert_eq!(marked, vec![(owner, kept.id)]);
    assert_eq!(grant_row(&pool, g).await.0, "needs_reconnect");
    assert_eq!(account_status(&pool, kept.id).await, "needs_reconnect");
    assert_eq!(account_status(&pool, gone.id).await, "disconnected");
    let updated_by: Option<Uuid> =
        sqlx::query_scalar("SELECT updated_by FROM account_grants WHERE id = $1")
            .bind(g.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("updated_by: {e}"));
    assert_eq!(updated_by, None);
}

#[sqlx::test]
async fn due_for_refresh_selects_active_oauth_within_window(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = conn(&pool).await;
    let now = Utc::now();
    let soon = oauth_grant(&mut c, owner, Some("u1"), Some(now + Duration::minutes(5))).await;
    let past = oauth_grant(&mut c, owner, Some("u2"), Some(now - Duration::minutes(5))).await;
    oauth_grant(&mut c, owner, Some("u3"), Some(now + Duration::hours(2))).await;
    oauth_grant(&mut c, owner, Some("u4"), None).await;
    let broken = oauth_grant(&mut c, owner, Some("u5"), Some(now)).await;
    GrantRepo::set_status(&mut c, &scope, broken, GrantStatus::NeedsReconnect)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));
    let credentials = grant(&mut c, &scope, "mastodon").await;
    sqlx::query("UPDATE account_grants SET expires_at = $2 WHERE id = $1")
        .bind(credentials.as_uuid())
        .bind(now)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("expire credentials grant: {e}"));

    let due = GrantRepo::due_for_refresh(&mut c, now + Duration::minutes(15), None, 100)
        .await
        .unwrap_or_else(|e| unreachable!("due: {e}"));

    let mut expected = vec![soon, past];
    expected.sort_by_key(GrantId::as_uuid);
    assert_eq!(due, expected);
}

#[sqlx::test]
async fn due_for_refresh_pages_by_id(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let mut c = conn(&pool).await;
    let now = Utc::now();
    let mut all = Vec::new();
    for remote in ["u1", "u2", "u3"] {
        all.push(oauth_grant(&mut c, owner, Some(remote), Some(now)).await);
    }
    all.sort_by_key(GrantId::as_uuid);
    let before = now + Duration::minutes(15);

    let first = GrantRepo::due_for_refresh(&mut c, before, None, 2)
        .await
        .unwrap_or_else(|e| unreachable!("first page: {e}"));
    let second = GrantRepo::due_for_refresh(&mut c, before, first.last().copied(), 2)
        .await
        .unwrap_or_else(|e| unreachable!("second page: {e}"));
    let third = GrantRepo::due_for_refresh(&mut c, before, second.last().copied(), 2)
        .await
        .unwrap_or_else(|e| unreachable!("third page: {e}"));

    assert_eq!(first, all[..2].to_vec());
    assert_eq!(second, all[2..].to_vec());
    assert!(third.is_empty());
}

#[sqlx::test]
async fn store_tokens_and_reconnect_by_passed(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = conn(&pool).await;
    let now = Utc::now();
    let g = oauth_grant(&mut c, owner, Some("u1"), Some(now)).await;
    let fresh = oauth_grant(&mut c, owner, Some("u2"), None).await;
    for (id, at) in [
        (g, now - Duration::minutes(1)),
        (fresh, now + Duration::days(3)),
    ] {
        sqlx::query("UPDATE account_grants SET reconnect_by = $2 WHERE id = $1")
            .bind(id.as_uuid())
            .bind(at)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("reconnect_by: {e}"));
    }
    let rotated = vault()
        .seal(&json!({"access_token": "new"}), &GrantRepo::seal_context(g))
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    let new_expiry =
        DateTime::from_timestamp(now.timestamp() + 3600, 0).unwrap_or_else(|| unreachable!("time"));

    let mut tx = begin(&pool).await;
    GrantRepo::store_tokens(&mut tx, g, &rotated, Some(new_expiry))
        .await
        .unwrap_or_else(|e| unreachable!("store: {e}"));
    commit(tx).await;
    let stored = GrantRepo::get(&mut c, &scope, g)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));
    assert_eq!(stored.credential.as_ref(), Some(&rotated));
    assert_eq!(stored.expires_at, Some(new_expiry));

    let passed = GrantRepo::reconnect_by_passed(&mut c, now)
        .await
        .unwrap_or_else(|e| unreachable!("passed: {e}"));
    assert_eq!(passed, vec![g]);
    let mut tx = begin(&pool).await;
    GrantRepo::set_reconnect_warned(&mut tx, g, "expired")
        .await
        .unwrap_or_else(|e| unreachable!("warned: {e}"));
    commit(tx).await;
    assert!(
        GrantRepo::reconnect_by_passed(&mut c, now)
            .await
            .unwrap_or_else(|e| unreachable!("passed again: {e}"))
            .is_empty()
    );
}

#[sqlx::test]
async fn set_revoked_clears_the_credential(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let mut c = conn(&pool).await;
    let g = oauth_grant(&mut c, owner, Some("mock-user"), None).await;
    let a = connect(&mut c, &OwnerScope::own(owner), g, "mastodon", "a").await;
    token(&mut c, owner, a.id).await;

    let mut tx = begin(&pool).await;
    GrantRepo::set_revoked(&mut tx, g)
        .await
        .unwrap_or_else(|e| unreachable!("set_revoked: {e}"));
    commit(tx).await;

    assert_eq!(grant_row(&pool, g).await, ("revoked".to_owned(), false));
    assert_eq!(account_status(&pool, a.id).await, "revoked");
    assert!(!has_token(&pool, a.id).await);
}

#[sqlx::test]
async fn unrevoked_oauth_grants_include_needs_reconnect(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = conn(&pool).await;
    let active = oauth_grant(&mut c, owner, Some("u1"), None).await;
    let broken = oauth_grant(&mut c, owner, Some("u2"), None).await;
    let revoked = oauth_grant(&mut c, owner, Some("u3"), None).await;
    grant(&mut c, &scope, "mastodon").await;
    oauth_grant(&mut c, other, Some("u4"), None).await;
    GrantRepo::set_status(&mut c, &scope, broken, GrantStatus::NeedsReconnect)
        .await
        .unwrap_or_else(|e| unreachable!("needs_reconnect: {e}"));
    GrantRepo::set_status(&mut c, &scope, revoked, GrantStatus::Revoked)
        .await
        .unwrap_or_else(|e| unreachable!("revoked: {e}"));

    let mut found: Vec<GrantId> = GrantRepo::unrevoked_oauth_grants_of(&mut c, owner)
        .await
        .unwrap_or_else(|e| unreachable!("unrevoked: {e}"))
        .into_iter()
        .map(|g| g.id)
        .collect();

    found.sort_by_key(GrantId::as_uuid);
    let mut expected = vec![active, broken];
    expected.sort_by_key(GrantId::as_uuid);
    assert_eq!(found, expected);
}

#[sqlx::test]
async fn delete_for_accounts_removes_only_those_tokens(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let scope = OwnerScope::own(owner);
    let mut c = conn(&pool).await;
    let g = oauth_grant(&mut c, owner, Some("u1"), None).await;
    let g_other = oauth_grant(&mut c, other, Some("u2"), None).await;
    let a = connect(&mut c, &scope, g, "mastodon", "a").await;
    let b = connect(&mut c, &scope, g, "mastodon", "b").await;
    let theirs = connect(&mut c, &OwnerScope::own(other), g_other, "mastodon", "t").await;
    token(&mut c, owner, a.id).await;
    token(&mut c, owner, b.id).await;
    token(&mut c, other, theirs.id).await;

    let mut tx = begin(&pool).await;
    let deleted = AccountTokenRepo::delete_for_accounts(&mut tx, &scope, &[a.id, theirs.id])
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    commit(tx).await;

    assert_eq!(deleted, 1);
    assert!(!has_token(&pool, a.id).await);
    assert!(has_token(&pool, b.id).await);
    assert!(has_token(&pool, theirs.id).await);
}

#[sqlx::test]
async fn grant_lock_serializes_two_transactions(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let mut c = conn(&pool).await;
    let g = oauth_grant(&mut c, owner, Some("u1"), None).await;
    drop(c);

    let mut first = begin(&pool).await;
    let (locked_owner, locked) = GrantRepo::lock_for_refresh(&mut first, g)
        .await
        .unwrap_or_else(|e| unreachable!("first lock: {e}"))
        .unwrap_or_else(|| unreachable!("grant"));
    assert_eq!(locked_owner, owner);
    assert_eq!(locked.id, g);

    let second_pool = pool.clone();
    let mut second = tokio::spawn(async move {
        let mut tx = begin(&second_pool).await;
        let got = GrantRepo::lock_for_refresh(&mut tx, g)
            .await
            .unwrap_or_else(|e| unreachable!("second lock: {e}"));
        commit(tx).await;
        got.map(|(o, grant)| (o, grant.credential))
    });

    let blocked = tokio::time::timeout(std::time::Duration::from_millis(500), &mut second).await;
    assert!(blocked.is_err(), "the second lock did not wait");

    let rotated = vault()
        .seal(
            &json!({"access_token": "rotated"}),
            &GrantRepo::seal_context(g),
        )
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    GrantRepo::store_tokens(&mut first, g, &rotated, None)
        .await
        .unwrap_or_else(|e| unreachable!("store: {e}"));
    commit(first).await;

    let (second_owner, credential) =
        tokio::time::timeout(std::time::Duration::from_secs(10), second)
            .await
            .unwrap_or_else(|_| unreachable!("the second lock never got through"))
            .unwrap_or_else(|e| unreachable!("join: {e}"))
            .unwrap_or_else(|| unreachable!("grant"));
    assert_eq!(second_owner, owner);
    assert_eq!(credential, Some(rotated));
}

#[sqlx::test]
async fn lock_for_refresh_of_a_missing_grant_is_none(pool: PgPool) {
    let mut tx = begin(&pool).await;
    let got = GrantRepo::lock_for_refresh(&mut tx, GrantId::from(Uuid::now_v7()))
        .await
        .unwrap_or_else(|e| unreachable!("lock: {e}"));
    assert!(got.is_none());
}

#[test]
fn audit_kind_account_revoked_by_platform_round_trips() {
    let kind = AuditEventKind::AccountRevokedByPlatform;
    assert_eq!(kind.as_str(), "account_revoked_by_platform");
    assert_eq!(
        "account_revoked_by_platform".parse::<AuditEventKind>().ok(),
        Some(kind)
    );
    assert!(AuditEventKind::ALL.contains(&kind));
}

#[sqlx::test]
async fn audit_kind_account_revoked_by_platform_is_stored(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let mut c = conn(&pool).await;
    let event = AuditEvent::new(AuditEventKind::AccountRevokedByPlatform)
        .owner(owner)
        .detail("platform", "mastodon")
        .unwrap_or_else(|e| unreachable!("detail: {e:?}"));
    AuditLog::record(&mut c, AuditEventId::from(Uuid::now_v7()), event)
        .await
        .unwrap_or_else(|e| unreachable!("record: {e}"));
    let kind: String = sqlx::query_scalar("SELECT kind FROM audit_events WHERE owner_id = $1")
        .bind(owner.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("read back: {e}"));
    assert_eq!(kind, "account_revoked_by_platform");
}
