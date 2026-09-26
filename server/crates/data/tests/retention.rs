use chrono::{Duration, Utc};
use postit_core::{AuditEventId, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::idempotency::IdempotencyRepo;
use postit_data::retention::{purge_audit_events, purge_expired_idempotency_keys};
use postit_data::users::UsersRepo;
use sqlx::PgPool;

#[sqlx::test]
async fn purge_audit_events_clears_old_ip_and_deletes_very_old_rows(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let event_id = AuditEventId::from(uuid::Uuid::now_v7());
    let event = AuditEvent::new(AuditEventKind::UserProvisioned).ip("203.0.113.5"
        .parse()
        .unwrap_or_else(|e| unreachable!("parse ip: {e}")));
    AuditLog::record(&mut conn, event_id, event)
        .await
        .unwrap_or_else(|e| unreachable!("record: {e}"));

    sqlx::query!(
        "UPDATE audit_events SET at = now() - interval '100 days' WHERE id = $1",
        event_id.as_uuid(),
    )
    .execute(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("backdate: {e}"));

    let (ip_cleared, deleted) =
        purge_audit_events(&mut conn, Duration::days(90), Duration::days(730))
            .await
            .unwrap_or_else(|e| unreachable!("purge: {e}"));

    assert_eq!(ip_cleared, 1);
    assert_eq!(deleted, 0);

    let ip: Option<String> = sqlx::query_scalar("SELECT ip FROM audit_events WHERE id = $1")
        .bind(event_id.as_uuid())
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|e| unreachable!("select: {e}"));
    assert!(ip.is_none());
}

#[sqlx::test]
async fn purge_expired_idempotency_keys_deletes_only_expired_rows(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, user_id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));

    IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user_id,
        user_id,
        "expired-key",
        "POST /posts",
        "hash-1",
        Utc::now() - Duration::hours(1),
    )
    .await
    .unwrap_or_else(|e| unreachable!("begin expired: {e}"));
    IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user_id,
        user_id,
        "live-key",
        "POST /posts",
        "hash-2",
        Utc::now() + Duration::hours(24),
    )
    .await
    .unwrap_or_else(|e| unreachable!("begin live: {e}"));

    let deleted = purge_expired_idempotency_keys(&mut conn)
        .await
        .unwrap_or_else(|e| unreachable!("purge: {e}"));

    assert_eq!(deleted, 1);
    let live = IdempotencyRepo::find(&mut conn, user_id, user_id, "live-key")
        .await
        .unwrap_or_else(|e| unreachable!("find live: {e}"));
    assert!(live.is_some());
    let expired = IdempotencyRepo::find(&mut conn, user_id, user_id, "expired-key")
        .await
        .unwrap_or_else(|e| unreachable!("find expired: {e}"));
    assert!(expired.is_none());
}
