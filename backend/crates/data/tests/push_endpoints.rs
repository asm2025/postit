#![allow(clippy::unwrap_used)]

use chrono::{DateTime, Utc};
use postit_core::{PushEndpointId, UserId};
use postit_data::DataError;
use postit_data::push_endpoints::{NewPushEndpoint, PushEndpointRepo, PushPlatform};
use postit_data::testkit::seed_user;
use postit_vault::{Sealed, Vault};
use sqlx::PgPool;
use uuid::Uuid;

fn vault() -> Vault {
    Vault::from_keys([(1, b"0123456789abcdef0123456789abcdef-test-key".to_vec())]).unwrap()
}

const URL: &str = "https://push.example.test/send/very-secret-endpoint-token";

fn sealed(id: PushEndpointId, column: &'static str, value: &str) -> Sealed<String> {
    vault()
        .seal(
            &value.to_owned(),
            &PushEndpointRepo::seal_context(id, column),
        )
        .unwrap()
}

fn endpoint(user: UserId, id: PushEndpointId, hash: u8, label: &str) -> NewPushEndpoint {
    NewPushEndpoint {
        id,
        user,
        endpoint: sealed(id, "endpoint", URL),
        endpoint_hash: [hash; 32],
        p256dh: sealed(id, "p256dh", "key-material"),
        auth: sealed(id, "auth", "auth-secret"),
        platform: PushPlatform::Web,
        device_label: label.to_owned(),
    }
}

fn fresh() -> PushEndpointId {
    PushEndpointId::from(Uuid::now_v7())
}

#[sqlx::test]
async fn push_upsert_replaces_same_endpoint_keeps_id(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let mut conn = pool.acquire().await.unwrap();
    let first = fresh();
    let kept = PushEndpointRepo::upsert(&mut conn, &endpoint(user, first, 7, "Laptop"))
        .await
        .unwrap();
    assert_eq!(kept, first);

    // The caller finds the stored ID and seals with it.
    let existing = PushEndpointRepo::find_id(&mut conn, user, &[7; 32])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(existing, first);
    let mut again = endpoint(user, existing, 7, "Work laptop");
    again.platform = PushPlatform::Android;
    again.p256dh = sealed(existing, "p256dh", "new-key");
    let second = PushEndpointRepo::upsert(&mut conn, &again).await.unwrap();

    assert_eq!(second, first);
    let rows = PushEndpointRepo::list_for_user(&mut conn, user)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].device_label.as_deref(), Some("Work laptop"));
    assert_eq!(rows[0].platform, PushPlatform::Android);
    let key: String = vault()
        .open(
            &rows[0].p256dh,
            &PushEndpointRepo::seal_context(first, "p256dh"),
        )
        .unwrap();
    assert_eq!(key, "new-key");
    assert_eq!(open(&rows[0].endpoint, first, "endpoint"), URL);
    assert_eq!(open(&rows[0].auth, first, "auth"), "auth-secret");
}

#[sqlx::test]
async fn push_endpoints_are_sealed(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let mut conn = pool.acquire().await.unwrap();
    let id = fresh();
    PushEndpointRepo::upsert(&mut conn, &endpoint(user, id, 1, "Phone"))
        .await
        .unwrap();

    let raw: Vec<u8> = sqlx::query_scalar("SELECT endpoint FROM push_endpoints WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();

    assert!(
        !raw.windows(URL.len()).any(|w| w == URL.as_bytes()),
        "the endpoint URL is stored in the clear"
    );
    let row = PushEndpointRepo::get(&mut conn, id).await.unwrap().unwrap();
    let opened: String = vault()
        .open(
            &row.endpoint,
            &PushEndpointRepo::seal_context(id, "endpoint"),
        )
        .unwrap();
    assert_eq!(opened, URL);
}

#[sqlx::test]
async fn another_users_endpoint_is_not_found_or_deleted(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let stranger = seed_user(&pool, "active").await;
    let mut conn = pool.acquire().await.unwrap();
    let id = fresh();
    PushEndpointRepo::upsert(&mut conn, &endpoint(owner, id, 2, "Tablet"))
        .await
        .unwrap();

    assert!(
        PushEndpointRepo::list_for_user(&mut conn, stranger)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !PushEndpointRepo::delete(&mut conn, stranger, id)
            .await
            .unwrap()
    );
    assert_eq!(
        PushEndpointRepo::list_for_user(&mut conn, owner)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        PushEndpointRepo::delete(&mut conn, owner, id)
            .await
            .unwrap()
    );
    assert!(
        PushEndpointRepo::get(&mut conn, id)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn delete_by_id_and_mark_success(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let mut conn = pool.acquire().await.unwrap();
    let id = fresh();
    PushEndpointRepo::upsert(&mut conn, &endpoint(user, id, 3, "Phone"))
        .await
        .unwrap();
    let at = DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap();

    PushEndpointRepo::mark_success(&mut conn, id, at)
        .await
        .unwrap();
    let row = PushEndpointRepo::get(&mut conn, id).await.unwrap().unwrap();
    assert_eq!(row.last_success_at, Some(at));

    PushEndpointRepo::delete_by_id(&mut conn, id).await.unwrap();
    assert!(
        PushEndpointRepo::get(&mut conn, id)
            .await
            .unwrap()
            .is_none()
    );
}

fn open(value: &Sealed<String>, id: PushEndpointId, column: &'static str) -> String {
    vault()
        .open(value, &PushEndpointRepo::seal_context(id, column))
        .unwrap()
}

#[sqlx::test]
async fn upsert_with_a_mismatched_id_is_rejected_and_changes_nothing(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let mut conn = pool.acquire().await.unwrap();
    let stored = fresh();
    PushEndpointRepo::upsert(&mut conn, &endpoint(user, stored, 9, "Phone"))
        .await
        .unwrap();

    // Sealed under a fresh ID although the device is stored under `stored`.
    let mut wrong = endpoint(user, fresh(), 9, "Hijack");
    wrong.p256dh = sealed(wrong.id, "p256dh", "other-key");
    let result = PushEndpointRepo::upsert(&mut conn, &wrong).await;

    assert!(
        matches!(&result, Err(DataError::Conflict(code)) if code == "push_endpoint_id_mismatch"),
        "{result:?}"
    );
    let rows = PushEndpointRepo::list_for_user(&mut conn, user)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, stored);
    assert_eq!(rows[0].device_label.as_deref(), Some("Phone"));
    assert_eq!(open(&rows[0].endpoint, stored, "endpoint"), URL);
    assert_eq!(open(&rows[0].p256dh, stored, "p256dh"), "key-material");
    assert_eq!(open(&rows[0].auth, stored, "auth"), "auth-secret");
}
