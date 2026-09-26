use emixdb::dto::Pagination;
use postit_core::{AuditEventId, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::audit_repo::{AuditFilter, AuditRepo};
use sqlx::PgPool;

#[sqlx::test]
async fn list_filters_by_kind(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let actor = UserId::from(uuid::Uuid::now_v7());

    AuditLog::record(
        &mut conn,
        AuditEventId::from(uuid::Uuid::now_v7()),
        AuditEvent::new(AuditEventKind::UserApproved).actor(actor),
    )
    .await
    .unwrap_or_else(|e| unreachable!("record approved: {e}"));
    AuditLog::record(
        &mut conn,
        AuditEventId::from(uuid::Uuid::now_v7()),
        AuditEvent::new(AuditEventKind::UserDisabled).actor(actor),
    )
    .await
    .unwrap_or_else(|e| unreachable!("record disabled: {e}"));

    let filter = AuditFilter {
        kind: Some("user_approved".to_string()),
        ..AuditFilter::default()
    };
    let result = AuditRepo::list(&mut conn, &filter, Pagination::default())
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));

    assert_eq!(result.total, 1);
    assert_eq!(result.data.len(), 1);
    assert_eq!(result.data[0].kind, "user_approved");
}

#[sqlx::test]
async fn list_paginates(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    for _ in 0..15 {
        AuditLog::record(
            &mut conn,
            AuditEventId::from(uuid::Uuid::now_v7()),
            AuditEvent::new(AuditEventKind::UserProvisioned),
        )
        .await
        .unwrap_or_else(|e| unreachable!("record: {e}"));
    }

    let page = AuditRepo::list(
        &mut conn,
        &AuditFilter::default(),
        Pagination {
            page: 2,
            page_size: 10,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("list: {e}"));

    assert_eq!(page.total, 15);
    assert_eq!(page.data.len(), 5);
}
