#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use emixdb::dto::Pagination;
use postit_config::ContactsSettings;
use postit_contacts::{ContactService, ContactsDeps, EmailEventApply, NewContactInput};
use postit_core::{EmailEventKind, SystemClock, SystemIdGenerator, UserId};
use postit_data::OwnerScope;
use postit_data::contacts::{ContactStatus, SuppressionReason};
use postit_data::email_hash::email_hash;
use postit_data::testkit::seed_user;
use postit_jobs::{Job, JobQueue, Queue};
use sqlx::PgPool;
use uuid::Uuid;

const KEY: &[u8] = b"contacts-test-key";

fn service(pool: &PgPool) -> ContactService {
    ContactService::new(ContactsDeps {
        pool: pool.clone(),
        clock: Arc::new(SystemClock),
        ids: Arc::new(SystemIdGenerator),
        queue: JobQueue::new(
            pool.clone(),
            Arc::new(SystemIdGenerator),
            Arc::new(SystemClock),
        ),
        signing_key: Arc::from(KEY),
        settings: ContactsSettings {
            csv_max_bytes: 1024 * 1024,
            csv_max_rows: 1000,
        },
    })
}

async fn owner(pool: &PgPool) -> OwnerScope {
    let owner = seed_user(pool, "active").await;
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 100, 0, 1000000)",
    )
    .bind(owner.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    OwnerScope::own(owner)
}

async fn add_contact(svc: &ContactService, scope: &OwnerScope, email: &str) {
    svc.create(
        scope,
        NewContactInput {
            email: email.to_string(),
            first_name: None,
            last_name: None,
            fields: None,
            tags: None,
            locale: None,
            consent_source: "test".to_string(),
            consent_at: None,
        },
    )
    .await
    .unwrap();
}

/// A `direct_sends` row for `email`, without the delivery chain above it.
async fn seed_send(pool: &PgPool, owner: UserId, email: &str) -> Uuid {
    let id = Uuid::now_v7();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SET session_replication_role = replica")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO direct_sends (id, owner_id, delivery_id, email_hash, state)
         VALUES ($1, $2, $3, $4, 'sent')",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(Uuid::now_v7())
    .bind(email_hash(KEY, email).to_vec())
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("SET session_replication_role = DEFAULT")
        .execute(&mut *conn)
        .await
        .unwrap();
    id
}

fn apply(scope: &OwnerScope, send: Uuid, kind: EmailEventKind) -> EmailEventApply {
    EmailEventApply {
        owner: scope.owner.as_uuid(),
        send,
        kind,
    }
}

async fn event_count(pool: &PgPool, send: Uuid, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM email_events WHERE direct_send_id = $1 AND kind = $2")
        .bind(send)
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn the_job_is_named_and_queued() {
    assert_eq!(EmailEventApply::JOB_TYPE, "email_event_apply");
    assert_eq!(EmailEventApply::QUEUE, Queue::Contacts);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bounce_event_suppresses(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    add_contact(&svc, &scope, "a@example.com").await;
    let send = seed_send(&pool, scope.owner, "a@example.com").await;

    svc.apply_event(apply(&scope, send, EmailEventKind::Bounced))
        .await
        .unwrap();

    let all = svc
        .suppressions(
            &scope,
            Pagination {
                page: 1,
                page_size: 10,
            },
        )
        .await
        .unwrap()
        .data;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].reason, SuppressionReason::Bounced);
    let contact = svc.get(&scope, all[0].contact_id.unwrap()).await.unwrap();
    assert_eq!(contact.status, ContactStatus::Bounced);
    assert_eq!(event_count(&pool, send, "bounced").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn complaint_event_suppresses_as_complained(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    add_contact(&svc, &scope, "a@example.com").await;
    let send = seed_send(&pool, scope.owner, "a@example.com").await;

    svc.apply_event(apply(&scope, send, EmailEventKind::Complained))
        .await
        .unwrap();

    let all = svc
        .suppressions(
            &scope,
            Pagination {
                page: 1,
                page_size: 10,
            },
        )
        .await
        .unwrap()
        .data;
    assert_eq!(all[0].reason, SuppressionReason::Complained);
    let contact = svc.get(&scope, all[0].contact_id.unwrap()).await.unwrap();
    assert_eq!(contact.status, ContactStatus::Complained);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn event_for_purged_send_is_harmless(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    svc.apply_event(apply(&scope, Uuid::now_v7(), EmailEventKind::Bounced))
        .await
        .unwrap();
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM email_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    let suppressions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM suppressions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((events, suppressions), (0, 0));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn event_for_another_owners_send_is_harmless(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    let other = owner(&pool).await;
    let send = seed_send(&pool, other.owner, "a@example.com").await;
    svc.apply_event(apply(&scope, send, EmailEventKind::Bounced))
        .await
        .unwrap();
    assert_eq!(event_count(&pool, send, "bounced").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn apply_twice_records_one_event(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    let send = seed_send(&pool, scope.owner, "a@example.com").await;
    for _ in 0..2 {
        svc.apply_event(apply(&scope, send, EmailEventKind::Delivered))
            .await
            .unwrap();
    }
    assert_eq!(event_count(&pool, send, "delivered").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn repeat_clicks_keep_every_row(pool: PgPool) {
    let svc = service(&pool);
    let scope = owner(&pool).await;
    let send = seed_send(&pool, scope.owner, "a@example.com").await;
    for kind in [
        EmailEventKind::Clicked,
        EmailEventKind::Clicked,
        EmailEventKind::Opened,
        EmailEventKind::Opened,
    ] {
        svc.apply_event(apply(&scope, send, kind)).await.unwrap();
    }
    assert_eq!(event_count(&pool, send, "clicked").await, 2);
    assert_eq!(event_count(&pool, send, "opened").await, 2);
}
