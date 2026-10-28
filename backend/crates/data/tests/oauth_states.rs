#![allow(clippy::unwrap_used)]

use chrono::{DateTime, Duration, Utc};
use postit_core::{GrantId, OAuthStateId, PendingConnectionId, PlatformId, UserId};
use postit_data::OwnerScope;
use postit_data::accounts::{GrantKind, GrantRepo, GrantStatus, NewGrant};
use postit_data::oauth_states::{NewOAuthState, OAuthStateRepo, RemoteUserExcept};
use postit_data::pending_connections::{NewPendingConnection, PendingConnectionRepo};
use postit_data::testkit::seed_user;
use postit_vault::{Sealed, Vault};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())])
        .unwrap_or_else(|e| unreachable!("vault: {e}"))
}

fn platform(id: &str) -> PlatformId {
    PlatformId::new(id).unwrap_or_else(|e| unreachable!("platform: {e}"))
}

async fn platforms(pool: &PgPool) {
    for id in ["mock_oauth", "mastodon"] {
        sqlx::query("INSERT INTO platforms (id, display_name) VALUES ($1, $1)")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
}

fn tokens(id: OAuthStateId) -> Sealed<Value> {
    vault()
        .seal(
            &json!({"access_token": "at", "refresh_token": "rt"}),
            &OAuthStateRepo::seal_context(id, "grant_tokens"),
        )
        .unwrap()
}

/// Inserts a state for `user` on `platform_id` with `state_hash`, expiring at `expires_at`.
async fn state(
    conn: &mut PgConnection,
    user: UserId,
    platform_id: &str,
    state_hash: [u8; 32],
    expires_at: DateTime<Utc>,
) -> OAuthStateId {
    let id = OAuthStateId::from(Uuid::now_v7());
    let verifier = vault()
        .seal(
            &json!("verifier"),
            &OAuthStateRepo::seal_context(id, "pkce_verifier"),
        )
        .unwrap();
    OAuthStateRepo::insert(
        conn,
        NewOAuthState {
            id,
            state_hash,
            user,
            platform: platform(platform_id),
            pkce_verifier: Some(verifier),
            return_to: "https://app.test/oauth-done".to_owned(),
            expires_at,
        },
    )
    .await
    .unwrap();
    id
}

/// Inserts a state and stores a pending result on it with `connect` as its `connect_id` hash.
async fn result(
    conn: &mut PgConnection,
    user: UserId,
    platform_id: &str,
    seed: u8,
    remote_user: Option<&str>,
    expires_at: DateTime<Utc>,
) -> OAuthStateId {
    let id = state(conn, user, platform_id, [seed; 32], expires_at).await;
    OAuthStateRepo::store_result(
        conn,
        id,
        &[seed.wrapping_add(100); 32],
        &tokens(id),
        &json!([{"remote_id": "acct-1"}]),
        remote_user,
    )
    .await
    .unwrap();
    id
}

async fn row_count(pool: &PgPool, id: OAuthStateId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM oauth_states WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn oauth_grant(
    conn: &mut PgConnection,
    owner: UserId,
    platform_id: &str,
    remote_user: Option<&str>,
) -> GrantId {
    let id = GrantId::from(Uuid::now_v7());
    let credential = vault()
        .seal(&json!({"access_token": "at"}), &GrantRepo::seal_context(id))
        .unwrap();
    GrantRepo::insert(
        conn,
        &OwnerScope::own(owner),
        NewGrant {
            id,
            platform: platform(platform_id),
            kind: GrantKind::Oauth,
            remote_user_id: remote_user.map(str::to_owned),
            credential,
            connection_config: json!({}),
            expires_at: Some(Utc::now() + Duration::hours(1)),
            reconnect_by: None,
            scopes: vec![],
        },
    )
    .await
    .unwrap();
    id
}

#[sqlx::test]
async fn state_consumed_once(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let id = state(
        &mut c,
        user,
        "mock_oauth",
        [1; 32],
        Utc::now() + Duration::minutes(10),
    )
    .await;
    let now = Utc::now();

    let first = OAuthStateRepo::consume_state(&mut c, &[1; 32], now)
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("first consume"));
    let second = OAuthStateRepo::consume_state(&mut c, &[1; 32], now)
        .await
        .unwrap();

    assert_eq!(first.id, id);
    assert_eq!(first.user, user);
    assert_eq!(first.platform, platform("mock_oauth"));
    assert_eq!(first.return_to, "https://app.test/oauth-done");
    let verifier: Value = vault()
        .open(
            first.pkce_verifier.as_ref().unwrap(),
            &OAuthStateRepo::seal_context(id, "pkce_verifier"),
        )
        .unwrap();
    assert_eq!(verifier, json!("verifier"));
    assert!(first.grant_tokens.is_none());
    assert!(second.is_none());
}

#[sqlx::test]
async fn expired_state_is_returned_for_the_service_to_refuse(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let expires_at = Utc::now() - Duration::minutes(1);
    let id = state(&mut c, user, "mock_oauth", [2; 32], expires_at).await;

    let taken = OAuthStateRepo::consume_state(&mut c, &[2; 32], Utc::now())
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("expired state"));

    assert_eq!(taken.id, id);
    assert!(taken.expires_at < Utc::now());
}

#[sqlx::test]
async fn result_found_without_consuming_then_locked(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let id = result(
        &mut c,
        user,
        "mock_oauth",
        3,
        Some("mock-user"),
        Utc::now() + Duration::minutes(10),
    )
    .await;
    let hash = [103; 32];

    let found = OAuthStateRepo::find_result(&mut c, &hash)
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("found"));
    let again = OAuthStateRepo::find_result(&mut c, &hash).await.unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.connect_id_hash.as_deref(), Some(&hash[..]));
    assert_eq!(found.discovered, Some(json!([{"remote_id": "acct-1"}])));
    assert_eq!(found.remote_user_id.as_deref(), Some("mock-user"));
    assert!(again.is_some());

    let mut tx = pool.begin().await.unwrap();
    let locked = OAuthStateRepo::lock_result(&mut tx, &hash)
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("locked"));
    let opened: Value = vault()
        .open(
            locked.grant_tokens.as_ref().unwrap(),
            &OAuthStateRepo::seal_context(id, "grant_tokens"),
        )
        .unwrap();
    assert_eq!(opened["refresh_token"], "rt");
    OAuthStateRepo::delete(&mut tx, id).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(row_count(&pool, id).await, 0);
}

#[sqlx::test]
async fn discard_hides_the_result_and_keeps_the_tokens(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let id = result(
        &mut c,
        user,
        "mock_oauth",
        4,
        Some("mock-user"),
        Utc::now() + Duration::minutes(10),
    )
    .await;
    let hash = [104; 32];
    let now = Utc::now();

    OAuthStateRepo::discard(&mut c, id, now).await.unwrap();

    assert!(
        OAuthStateRepo::find_result(&mut c, &hash)
            .await
            .unwrap()
            .is_none()
    );
    let mut tx = pool.begin().await.unwrap();
    assert!(
        OAuthStateRepo::lock_result(&mut tx, &hash)
            .await
            .unwrap()
            .is_none()
    );
    tx.rollback().await.unwrap();
    let (expires_at, has_tokens, remote): (DateTime<Utc>, bool, Option<String>) = sqlx::query_as(
        "SELECT expires_at, grant_tokens IS NOT NULL, remote_user_id \
             FROM oauth_states WHERE id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(expires_at <= now);
    assert!(has_tokens);
    assert_eq!(remote.as_deref(), Some("mock-user"));
}

#[sqlx::test]
async fn store_discarded_keeps_tokens_without_a_connect_id(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let id = state(
        &mut c,
        user,
        "mock_oauth",
        [5; 32],
        Utc::now() + Duration::minutes(10),
    )
    .await;
    let now = Utc::now();

    OAuthStateRepo::store_discarded(&mut c, id, &tokens(id), Some("mock-user"), now)
        .await
        .unwrap();

    let taken = OAuthStateRepo::take_expired_result(&mut c, now)
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("discarded row"));
    assert_eq!(taken.id, id);
    assert!(taken.connect_id_hash.is_none());
    assert!(taken.grant_tokens.is_some());
    assert_eq!(taken.remote_user_id.as_deref(), Some("mock-user"));
}

#[sqlx::test]
async fn take_expired_result_returns_each_row_once(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let past = Utc::now() - Duration::minutes(1);
    let a = result(&mut c, user, "mock_oauth", 6, Some("u"), past).await;
    let b = result(&mut c, user, "mock_oauth", 7, Some("u"), past).await;
    let live = result(
        &mut c,
        user,
        "mock_oauth",
        8,
        Some("u"),
        Utc::now() + Duration::minutes(10),
    )
    .await;
    let now = Utc::now();

    let first = OAuthStateRepo::take_expired_result(&mut c, now)
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("first"));
    let second = OAuthStateRepo::take_expired_result(&mut c, now)
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("second"));
    let third = OAuthStateRepo::take_expired_result(&mut c, now)
        .await
        .unwrap();

    let mut taken = vec![first.id, second.id];
    taken.sort_by_key(OAuthStateId::as_uuid);
    let mut expected = vec![a, b];
    expected.sort_by_key(OAuthStateId::as_uuid);
    assert_eq!(taken, expected);
    assert!(third.is_none());
    assert_eq!(row_count(&pool, a).await, 0);
    assert_eq!(row_count(&pool, b).await, 0);
    assert_eq!(row_count(&pool, live).await, 1);
}

#[sqlx::test]
async fn take_results_of_takes_only_that_users_rows(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let later = Utc::now() + Duration::minutes(10);
    let mine = result(&mut c, user, "mock_oauth", 9, Some("u"), later).await;
    let tokenless = state(&mut c, user, "mock_oauth", [10; 32], later).await;
    let theirs = result(&mut c, other, "mock_oauth", 11, Some("u"), later).await;

    let taken = OAuthStateRepo::take_results_of(&mut c, user).await.unwrap();

    assert_eq!(taken.iter().map(|s| s.id).collect::<Vec<_>>(), vec![mine]);
    assert_eq!(row_count(&pool, mine).await, 0);
    assert_eq!(row_count(&pool, tokenless).await, 1);
    assert_eq!(row_count(&pool, theirs).await, 1);
}

#[sqlx::test]
async fn delete_unconsumed_of_leaves_consumed_states_results_and_other_users(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let later = Utc::now() + Duration::minutes(10);
    let unconsumed = state(&mut c, user, "mock_oauth", [30; 32], later).await;
    let consumed = state(&mut c, user, "mock_oauth", [31; 32], later).await;
    OAuthStateRepo::consume_state(&mut c, &[31; 32], Utc::now())
        .await
        .unwrap()
        .unwrap();
    let mine = result(&mut c, user, "mock_oauth", 32, Some("u"), later).await;
    let theirs = state(&mut c, other, "mock_oauth", [33; 32], later).await;

    let deleted = OAuthStateRepo::delete_unconsumed_of(&mut c, user)
        .await
        .unwrap();

    assert_eq!(deleted, 1);
    assert_eq!(row_count(&pool, unconsumed).await, 0);
    assert_eq!(row_count(&pool, consumed).await, 1);
    assert_eq!(row_count(&pool, mine).await, 1);
    assert_eq!(row_count(&pool, theirs).await, 1);
    // The deleted state can no longer be consumed by a callback.
    assert!(
        OAuthStateRepo::consume_state(&mut c, &[30; 32], Utc::now())
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn purge_removes_only_expired_tokenless_states_and_pending_connections(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let past = Utc::now() - Duration::minutes(1);
    let later = Utc::now() + Duration::minutes(10);
    let expired_plain = state(&mut c, user, "mock_oauth", [12; 32], past).await;
    let live_plain = state(&mut c, user, "mock_oauth", [13; 32], later).await;
    let expired_with_tokens = result(&mut c, user, "mock_oauth", 14, Some("u"), past).await;
    for (seed, expires_at) in [(15_u8, past), (16_u8, later)] {
        let id = PendingConnectionId::from(Uuid::now_v7());
        let credential = vault()
            .seal(
                &json!({"token": "secret"}),
                &PendingConnectionRepo::seal_context(id),
            )
            .unwrap();
        PendingConnectionRepo::insert(
            &mut c,
            &OwnerScope::own(user),
            NewPendingConnection {
                id,
                platform: platform("mastodon"),
                connect_id_hash: [seed; 32],
                credential,
                connection_config: json!({}),
                discovered: json!([]),
                expires_at,
            },
        )
        .await
        .unwrap();
    }

    let purged = OAuthStateRepo::purge_expired(&mut c, Utc::now())
        .await
        .unwrap();

    assert_eq!(purged, 2);
    assert_eq!(row_count(&pool, expired_plain).await, 0);
    assert_eq!(row_count(&pool, live_plain).await, 1);
    assert_eq!(row_count(&pool, expired_with_tokens).await, 1);
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pending, 1);
    assert!(
        PendingConnectionRepo::find(&mut c, &[16; 32])
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        PendingConnectionRepo::find(&mut c, &[15; 32])
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn pending_connection_find_does_not_consume(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let id = PendingConnectionId::from(Uuid::now_v7());
    let credential = vault()
        .seal(
            &json!({"token": "secret"}),
            &PendingConnectionRepo::seal_context(id),
        )
        .unwrap();
    PendingConnectionRepo::insert(
        &mut c,
        &OwnerScope::own(user),
        NewPendingConnection {
            id,
            platform: platform("mastodon"),
            connect_id_hash: [17; 32],
            credential,
            connection_config: json!({}),
            discovered: json!([{"remote_id": "a"}]),
            expires_at: Utc::now() + Duration::minutes(10),
        },
    )
    .await
    .unwrap();

    let found = PendingConnectionRepo::find(&mut c, &[17; 32])
        .await
        .unwrap()
        .unwrap_or_else(|| unreachable!("found"));
    let taken = PendingConnectionRepo::take(&mut c, &[17; 32])
        .await
        .unwrap();

    assert_eq!(found.id, id);
    assert_eq!(found.owner, user);
    assert_eq!(found.discovered, json!([{"remote_id": "a"}]));
    assert!(taken.is_some());
}

#[sqlx::test]
async fn remote_user_in_use_counts_active_grants_and_live_results(pool: PgPool) {
    platforms(&pool).await;
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let p = platform("mock_oauth");
    let now = Utc::now();
    let in_use = |remote: &'static str, except: RemoteUserExcept| {
        let p = p.clone();
        let pool = pool.clone();
        async move {
            let mut c = pool.acquire().await.unwrap();
            OAuthStateRepo::remote_user_in_use(&mut c, &p, remote, except, now)
                .await
                .unwrap()
        }
    };

    // Nothing holds the remote user yet.
    assert!(!in_use("grant-user", RemoteUserExcept::Nothing).await);

    // Another owner's active grant counts; the excepted grant does not.
    let active = oauth_grant(&mut c, other, "mock_oauth", Some("grant-user")).await;
    assert!(in_use("grant-user", RemoteUserExcept::Nothing).await);
    assert!(
        in_use(
            "grant-user",
            RemoteUserExcept::Grant(GrantId::from(Uuid::now_v7()))
        )
        .await
    );
    assert!(!in_use("grant-user", RemoteUserExcept::Grant(active)).await);

    // Revoked and needs_reconnect grants do not count.
    for status in [GrantStatus::Revoked, GrantStatus::NeedsReconnect] {
        GrantRepo::set_status(&mut c, &OwnerScope::own(other), active, status)
            .await
            .unwrap();
        assert!(
            !in_use("grant-user", RemoteUserExcept::Nothing).await,
            "{status:?}"
        );
    }

    // A grant of another platform does not count.
    oauth_grant(&mut c, owner, "mastodon", Some("grant-user")).await;
    assert!(!in_use("grant-user", RemoteUserExcept::Nothing).await);

    // A live, undiscarded result counts; the excepted row does not.
    let live = result(
        &mut c,
        owner,
        "mock_oauth",
        20,
        Some("result-user"),
        now + Duration::minutes(10),
    )
    .await;
    assert!(in_use("result-user", RemoteUserExcept::Nothing).await);
    assert!(!in_use("result-user", RemoteUserExcept::State(live)).await);

    // Once discarded it no longer counts.
    OAuthStateRepo::discard(&mut c, live, now).await.unwrap();
    assert!(!in_use("result-user", RemoteUserExcept::Nothing).await);

    // An expired result does not count either.
    result(
        &mut c,
        owner,
        "mock_oauth",
        21,
        Some("expired-user"),
        now - Duration::minutes(1),
    )
    .await;
    assert!(!in_use("expired-user", RemoteUserExcept::Nothing).await);

    // A live result on another platform does not count.
    result(
        &mut c,
        owner,
        "mastodon",
        22,
        Some("other-platform-user"),
        now + Duration::minutes(10),
    )
    .await;
    assert!(!in_use("other-platform-user", RemoteUserExcept::Nothing).await);

    // A row holding tokens without a connect_id (stored discarded) does not count, even before
    // it expires: only the connect_id_hash predicate excludes it.
    let discarded = state(
        &mut c,
        owner,
        "mock_oauth",
        [23; 32],
        now + Duration::minutes(10),
    )
    .await;
    OAuthStateRepo::store_discarded(
        &mut c,
        discarded,
        &tokens(discarded),
        Some("discarded-user"),
        now + Duration::minutes(10),
    )
    .await
    .unwrap();
    assert!(!in_use("discarded-user", RemoteUserExcept::Nothing).await);
}

#[sqlx::test]
async fn discard_results_of_remote_user_touches_only_that_users_live_results(pool: PgPool) {
    platforms(&pool).await;
    let user = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let now = Utc::now();
    let later = now + Duration::minutes(10);
    let target = result(&mut c, user, "mock_oauth", 30, Some("gone"), later).await;
    let other_user = result(&mut c, user, "mock_oauth", 31, Some("kept"), later).await;
    let other_platform = result(&mut c, user, "mastodon", 32, Some("gone"), later).await;

    let discarded = OAuthStateRepo::discard_results_of_remote_user(
        &mut c,
        &platform("mock_oauth"),
        "gone",
        now,
    )
    .await
    .unwrap();

    assert_eq!(discarded, 1);
    assert!(
        OAuthStateRepo::find_result(&mut c, &[130; 32])
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        row_count(&pool, target).await,
        1,
        "kept for the purge to revoke"
    );
    for (seed, id) in [(131_u8, other_user), (132_u8, other_platform)] {
        let found = OAuthStateRepo::find_result(&mut c, &[seed; 32])
            .await
            .unwrap()
            .unwrap_or_else(|| unreachable!("result {seed}"));
        assert_eq!(found.id, id);
    }
}
