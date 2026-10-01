use postit_core::{AuditEventId, UserId};
use postit_data::DataError;
use postit_data::audit::{AuditError, AuditEvent, AuditEventKind, AuditLog};
use postit_data::users::UsersRepo;
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
    for (id, sub) in [(actor, "actor"), (subject, "subject")] {
        UsersRepo::provision(&mut conn, id, "https://issuer.test", sub, sub)
            .await
            .unwrap_or_else(|e| unreachable!("provision: {e}"));
    }

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

#[sqlx::test]
async fn record_fails_when_a_referenced_user_is_missing(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let existing = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, existing, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    let ghost = UserId::from(uuid::Uuid::now_v7());

    let event = AuditEvent::new(AuditEventKind::RoleChanged)
        .actor(existing)
        .detail_user_id("other_user_id", ghost)
        .unwrap_or_else(|e| unreachable!("detail: {e}"));
    let result = AuditLog::record(&mut conn, AuditEventId::from(uuid::Uuid::now_v7()), event).await;

    assert!(
        matches!(result, Err(DataError::Conflict(_))),
        "got {result:?}"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn an_open_audit_write_blocks_deleting_its_referenced_user(pool: PgPool) {
    let mut setup = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut setup, user, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    drop(setup);

    let mut audit_tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    AuditLog::record(
        &mut audit_tx,
        AuditEventId::from(uuid::Uuid::now_v7()),
        AuditEvent::new(AuditEventKind::UserEnabled).subject(user),
    )
    .await
    .unwrap_or_else(|e| unreachable!("record: {e}"));

    let delete_pool = pool.clone();
    let delete = tokio::spawn(async move {
        let mut conn = delete_pool
            .acquire()
            .await
            .unwrap_or_else(|e| unreachable!("acquire: {e}"));
        UsersRepo::delete(&mut conn, user).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !delete.is_finished(),
        "DELETE must wait for the audit transaction's key-share lock"
    );

    audit_tx
        .commit()
        .await
        .unwrap_or_else(|e| unreachable!("commit: {e}"));
    let deleted = delete
        .await
        .unwrap_or_else(|e| unreachable!("join: {e}"))
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    assert!(deleted);
}

#[test]
fn audit_event_kind_parses_strictly() {
    for kind in AuditEventKind::ALL {
        assert_eq!(kind.as_str().parse::<AuditEventKind>().ok(), Some(kind));
    }
    assert!("user_exploded".parse::<AuditEventKind>().is_err());
}
