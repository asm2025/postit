#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use emixdb::dto::Pagination;
use postit_config::ContactsSettings;
use postit_contacts::{
    ContactError, ContactPatch, ContactService, ContactsDeps, NewContactInput, SettableStatus,
    parse_address,
};
use postit_core::{ContactId, SuppressionId, SystemClock, SystemIdGenerator, UserId};
use postit_data::contacts::{ContactFilter, SuppressionReason, SuppressionRepo};
use postit_data::testkit::{callers, seed_user};
use postit_data::{Capability, OwnerScope};
use postit_jobs::JobQueue;
use sqlx::PgPool;

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

/// `seed_user` skips provisioning, so it has no limits row; this adds one.
async fn set_cap(pool: &PgPool, user: UserId, cap: i32) {
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 100, $2, 1000000)
         ON CONFLICT (user_id) DO UPDATE SET contacts_cap = EXCLUDED.contacts_cap",
    )
    .bind(user.as_uuid())
    .bind(cap)
    .execute(pool)
    .await
    .unwrap();
}

fn input(email: &str) -> NewContactInput {
    NewContactInput {
        email: email.to_string(),
        first_name: None,
        last_name: None,
        fields: None,
        tags: None,
        locale: None,
        consent_source: "signup form".to_string(),
        consent_at: None,
    }
}

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

async fn audit_count(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE kind = $1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn parse_address_normalizes_and_refuses() {
    assert_eq!(
        parse_address("  Bob@Example.COM ").as_deref(),
        Some("bob@example.com")
    );
    for bad in [
        "",
        "bob",
        "bob@",
        "@example.com",
        "a b@example.com",
        "Bob <bob@example.com>",
    ] {
        assert_eq!(parse_address(bad), None, "{bad:?}");
    }
    let long = format!("{}@example.com", "a".repeat(250));
    assert_eq!(parse_address(&long), None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_requires_consent_source(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 0).await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool);

    let mut blank = input("bob@example.com");
    blank.consent_source = "   ".to_string();
    assert!(matches!(
        svc.create(&scope, blank).await,
        Err(ContactError::ConsentRequired)
    ));

    let ok = svc.create(&scope, input("bob@example.com")).await.unwrap();
    assert_eq!(ok.consent_source.as_deref(), Some("signup form"));
    assert!(ok.consent_at.is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_refuses_invalid_address(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 0).await;
    let svc = service(&pool);
    assert!(matches!(
        svc.create(&OwnerScope::own(owner), input("not an address"))
            .await,
        Err(ContactError::InvalidEmail)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cap_blocks_new_contact(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 2).await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool);

    svc.create(&scope, input("a@example.com")).await.unwrap();
    svc.create(&scope, input("b@example.com")).await.unwrap();
    assert!(matches!(
        svc.create(&scope, input("c@example.com")).await,
        Err(ContactError::Limit)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cap_zero_is_unlimited(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 0).await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool);
    for n in 0..5 {
        svc.create(&scope, input(&format!("u{n}@example.com")))
            .await
            .unwrap();
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn update_of_existing_ignores_cap(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 1).await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool);

    let first = svc.create(&scope, input("a@example.com")).await.unwrap();
    // At the cap: a new address is refused, the same address in another case is not.
    let mut again = input(" A@Example.com ");
    again.first_name = Some("Ann".to_string());
    let updated = svc.create(&scope, again).await.unwrap();
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.first_name.as_deref(), Some("Ann"));
    assert!(matches!(
        svc.create(&scope, input("b@example.com")).await,
        Err(ContactError::Limit)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_creates_never_pass_cap(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 3).await;
    let scope = OwnerScope::own(owner);
    let svc = Arc::new(service(&pool));
    svc.create(&scope, input("a@example.com")).await.unwrap();
    svc.create(&scope, input("b@example.com")).await.unwrap();

    // One slot left, two writers.
    let tasks: Vec<_> = ["c@example.com", "d@example.com"]
        .into_iter()
        .map(|email| {
            let svc = Arc::clone(&svc);
            let scope = scope.clone();
            tokio::spawn(async move { svc.create(&scope, input(email)).await })
        })
        .collect();
    let mut ok = 0;
    let mut limited = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => ok += 1,
            Err(ContactError::Limit) => limited += 1,
            Err(other) => panic!("unexpected error: {other}"),
        }
    }
    assert_eq!((ok, limited), (1, 1));
    let total = svc
        .list(&scope, page(), ContactFilter::default())
        .await
        .unwrap();
    assert_eq!(total.total, 3);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn update_changes_fields_and_status(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 0).await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool);
    let contact = svc.create(&scope, input("a@example.com")).await.unwrap();

    let patched = svc
        .update(
            &scope,
            contact.id,
            ContactPatch {
                first_name: Some(Some("Ann".to_string())),
                status: Some(SettableStatus::Unsubscribed),
                ..ContactPatch::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(patched.first_name.as_deref(), Some("Ann"));
    assert_eq!(patched.email, "a@example.com");
    assert_eq!(patched.status.as_str(), "unsubscribed");

    assert!(matches!(
        svc.update(
            &scope,
            contact.id,
            ContactPatch {
                consent_source: Some(" ".to_string()),
                ..ContactPatch::default()
            },
        )
        .await,
        Err(ContactError::ConsentRequired)
    ));
    assert!(matches!(
        svc.update(
            &scope,
            ContactId::from_uuid(uuid::Uuid::now_v7()),
            ContactPatch::default()
        )
        .await,
        Err(ContactError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn view_delegate_cannot_create_or_update(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 0).await;
    let who = callers(&pool, owner, &[]).await;
    let svc = service(&pool);
    let contact = svc
        .create(&who.owner, input("a@example.com"))
        .await
        .unwrap();

    assert!(matches!(
        svc.create(&who.view, input("b@example.com")).await,
        Err(ContactError::Forbidden(Capability::Edit))
    ));
    assert!(matches!(
        svc.update(
            &who.view,
            contact.id,
            ContactPatch {
                first_name: Some(Some("Ann".to_string())),
                ..ContactPatch::default()
            },
        )
        .await,
        Err(ContactError::Forbidden(Capability::Edit))
    ));
    let unchanged = svc.get(&who.owner, contact.id).await.unwrap();
    assert_eq!(unchanged.first_name, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn update_resubscribing_suppressed_address_is_refused(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 0).await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool);
    let contact = svc.create(&scope, input("a@example.com")).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    SuppressionRepo::add(
        &mut conn,
        &scope,
        KEY,
        "a@example.com",
        SuppressionReason::Unsubscribed,
    )
    .await
    .unwrap();
    drop(conn);

    assert!(matches!(
        svc.update(
            &scope,
            contact.id,
            ContactPatch {
                status: Some(SettableStatus::Subscribed),
                ..ContactPatch::default()
            },
        )
        .await,
        Err(ContactError::Suppressed)
    ));
    let still = svc.get(&scope, contact.id).await.unwrap();
    assert_eq!(still.status.as_str(), "unsubscribed");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_list_audits(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 0).await;
    let scope = OwnerScope::own(owner);
    let svc = service(&pool);
    let list = svc.create_list(&scope, "News").await.unwrap();
    let contact = svc.create(&scope, input("a@example.com")).await.unwrap();
    svc.add_member(&scope, list.id, &[contact.id])
        .await
        .unwrap();

    svc.delete_list(&scope, list.id).await.unwrap();

    assert_eq!(audit_count(&pool, "contact_list_deleted").await, 1);
    let details: serde_json::Value =
        sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = 'contact_list_deleted'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(details["members"], 1);
    assert_eq!(details["list_id"], list.id.to_string());
    assert!(!details.to_string().contains("example.com"));
    // The contact stays.
    svc.get(&scope, contact.id).await.unwrap();
    assert!(matches!(
        svc.delete_list(&scope, list.id).await,
        Err(ContactError::NotFound)
    ));
    assert_eq!(audit_count(&pool, "contact_list_deleted").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn remove_suppression_audits(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    set_cap(&pool, owner, 0).await;
    let who = callers(&pool, owner, &[]).await;
    let svc = service(&pool);
    let mut conn = pool.acquire().await.unwrap();
    SuppressionRepo::add(
        &mut conn,
        &who.owner,
        KEY,
        "a@example.com",
        SuppressionReason::Bounced,
    )
    .await
    .unwrap();
    SuppressionRepo::add(
        &mut conn,
        &who.owner,
        KEY,
        "b@example.com",
        SuppressionReason::Manual,
    )
    .await
    .unwrap();
    drop(conn);
    let entries = svc.suppressions(&who.owner, page()).await.unwrap().data;
    let bounce = entries
        .iter()
        .find(|s| s.reason == SuppressionReason::Bounced)
        .unwrap()
        .id;
    let manual = entries
        .iter()
        .find(|s| s.reason == SuppressionReason::Manual)
        .unwrap()
        .id;

    // A delegate may not remove one.
    assert!(matches!(
        svc.remove_suppression(&who.edit, bounce).await,
        Err(ContactError::Forbidden(Capability::OwnerOnly))
    ));
    assert!(matches!(
        svc.remove_suppression(&who.owner, manual).await,
        Err(ContactError::NotRemovable)
    ));
    assert_eq!(audit_count(&pool, "suppression_removed").await, 0);

    svc.remove_suppression(&who.owner, bounce).await.unwrap();
    assert_eq!(audit_count(&pool, "suppression_removed").await, 1);
    let details: serde_json::Value =
        sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = 'suppression_removed'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(details["suppression_id"], bounce.to_string());
    assert!(!details.to_string().contains("example.com"));
    assert!(matches!(
        svc.remove_suppression(&who.owner, SuppressionId::from_uuid(uuid::Uuid::now_v7()))
            .await,
        Err(ContactError::NotFound)
    ));
}
