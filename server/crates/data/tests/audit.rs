use postit_core::{AuditEventId, UserId};
use postit_data::audit::{AuditError, AuditEvent, AuditEventKind, AuditLog};
use sqlx::PgPool;

#[test]
fn detail_user_id_rejects_a_key_not_ending_in_user_id() {
    let result = AuditEvent::new(AuditEventKind::UserProvisioned)
        .detail_user_id("actor", UserId::from(uuid::Uuid::now_v7()));
    let Err(err) = result else {
        unreachable!("expected KeyMustEndUserId");
    };
    assert_eq!(err, AuditError::KeyMustEndUserId("actor".to_string()));
}

#[test]
fn detail_rejects_a_key_ending_in_user_id() {
    let result =
        AuditEvent::new(AuditEventKind::UserProvisioned).detail("delegate_user_id", "not-a-uuid");
    let Err(err) = result else {
        unreachable!("expected KeyMustNotEndUserId");
    };
    assert_eq!(
        err,
        AuditError::KeyMustNotEndUserId("delegate_user_id".to_string())
    );
}

#[sqlx::test]
async fn record_writes_kind_actor_and_details(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let event_id = AuditEventId::from(uuid::Uuid::now_v7());
    let actor = UserId::from(uuid::Uuid::now_v7());
    let subject = UserId::from(uuid::Uuid::now_v7());

    let event = AuditEvent::new(AuditEventKind::RoleChanged)
        .actor(actor)
        .subject(subject)
        .detail("new_role", "admin")
        .unwrap_or_else(|e| unreachable!("detail: {e:?}"));

    AuditLog::record(&mut conn, event_id, event)
        .await
        .unwrap_or_else(|e| unreachable!("record: {e}"));

    let row: (
        String,
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT kind, actor_user_id, subject_user_id, details FROM audit_events WHERE id = $1",
    )
    .bind(event_id.as_uuid())
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("select: {e}"));

    assert_eq!(row.0, "role_changed");
    assert_eq!(row.1, Some(actor.as_uuid()));
    assert_eq!(row.2, Some(subject.as_uuid()));
    assert_eq!(
        row.3.get("new_role").and_then(|v| v.as_str()),
        Some("admin")
    );
}
