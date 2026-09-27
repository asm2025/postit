use postit_core::{AuditEventId, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::pseudonym::pseudonym_for;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

const KEY: &[u8] = b"test-pseudonym-key";

type AuditRow = (
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
    Value,
);

async fn record(pool: &PgPool, event: AuditEvent) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    AuditLog::record(&mut conn, AuditEventId::from(Uuid::now_v7()), event)
        .await
        .unwrap_or_else(|e| unreachable!("record: {e}"));
}

#[test]
fn pseudonym_is_uuid_v8_rfc4122_variant_and_stable() {
    let user = UserId::from(Uuid::now_v7());
    let a = pseudonym_for(KEY, user);
    let b = pseudonym_for(KEY, user);
    assert_eq!(a, b);
    assert_eq!(a.get_version_num(), 8);
    assert_eq!(a.get_variant(), uuid::Variant::RFC4122);
    assert_ne!(a, pseudonym_for(b"other-key", user));
}

#[test]
fn pseudonym_is_not_a_plain_hash() {
    let user = UserId::from(Uuid::now_v7());
    let digest = Sha256::digest(user.as_uuid().as_bytes());
    let mut plain = [0u8; 16];
    plain.copy_from_slice(&digest[..16]);
    assert_ne!(pseudonym_for(KEY, user).as_bytes(), &plain);
}

#[sqlx::test]
async fn every_reference_is_replaced_and_actor_ip_cleared(pool: PgPool) {
    let gone = UserId::from(Uuid::now_v7());
    let other = UserId::from(Uuid::now_v7());
    let ip: std::net::IpAddr = "203.0.113.7"
        .parse()
        .unwrap_or_else(|e| unreachable!("ip: {e}"));

    record(
        &pool,
        AuditEvent::new(AuditEventKind::RoleChanged)
            .actor(gone)
            .subject(other)
            .ip(ip),
    )
    .await;
    record(
        &pool,
        AuditEvent::new(AuditEventKind::UserApproved)
            .actor(other)
            .subject(gone)
            .ip(ip),
    )
    .await;
    record(
        &pool,
        AuditEvent::new(AuditEventKind::UserDisabled).owner(gone),
    )
    .await;
    record(
        &pool,
        AuditEvent::new(AuditEventKind::UserEnabled)
            .actor(other)
            .detail_user_id("delegate_user_id", gone)
            .unwrap_or_else(|e| unreachable!("detail: {e}"))
            .detail_user_id("grantor_user_id", other)
            .unwrap_or_else(|e| unreachable!("detail: {e}"))
            .detail("note", "keep")
            .unwrap_or_else(|e| unreachable!("detail: {e}")),
    )
    .await;

    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let touched = AuditLog::pseudonymize_user(&mut conn, gone, KEY)
        .await
        .unwrap_or_else(|e| unreachable!("pseudonymize: {e}"));
    assert!(touched >= 4);

    let raw = gone.as_uuid();
    let pseudo = pseudonym_for(KEY, gone);
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE actor_user_id = $1 OR owner_id = $1 OR subject_user_id = $1
            OR details::text LIKE '%' || $1::text || '%'",
    )
    .bind(raw)
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("count raw: {e}"));
    assert_eq!(remaining, 0);

    let rows: Vec<AuditRow> = sqlx::query_as(
        "SELECT actor_user_id, owner_id, subject_user_id, ip, details FROM audit_events ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("select: {e}"));

    // RoleChanged: gone was the actor → pseudonym + ip cleared.
    assert_eq!(rows[0].0, Some(pseudo));
    assert_eq!(rows[0].3, None);
    // UserApproved: gone was the subject, other was the actor → ip kept.
    assert_eq!(rows[1].2, Some(pseudo));
    assert_eq!(rows[1].3.as_deref(), Some("203.0.113.7"));
    // UserDisabled: owner.
    assert_eq!(rows[2].1, Some(pseudo));
    // UserEnabled: details key replaced, others untouched.
    assert_eq!(
        rows[3].4["delegate_user_id"],
        Value::String(pseudo.to_string())
    );
    assert_eq!(
        rows[3].4["grantor_user_id"],
        Value::String(other.as_uuid().to_string())
    );
    assert_eq!(rows[3].4["note"], Value::String("keep".into()));

    let again = AuditLog::pseudonymize_user(&mut conn, gone, KEY)
        .await
        .unwrap_or_else(|e| unreachable!("second run: {e}"));
    assert_eq!(again, 0);
}
