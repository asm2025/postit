#![allow(clippy::unwrap_used)]

use emixdb::dto::Pagination;
use postit_core::{ContactId, ContactImportId, ContactListId};
use postit_data::contacts::{
    ContactFilter, ContactImportRepo, ContactListRepo, ContactRepo, ContactStatus, ContactUpsert,
    ImportStatus, NewContact, SuppressionReason, SuppressionRepo,
};
use postit_data::email_hash::email_hash;
use postit_data::testkit::{callers, seed_user};
use postit_data::{DataError, OwnerScope, ScopeError};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const KEY: &[u8] = b"test-suppression-key";

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

fn contact_id() -> ContactId {
    ContactId::from_uuid(Uuid::now_v7())
}

fn list_id() -> ContactListId {
    ContactListId::from_uuid(Uuid::now_v7())
}

fn new_contact(email: &str) -> NewContact {
    NewContact {
        id: contact_id(),
        email: email.to_string(),
        first_name: Some("Bob".to_string()),
        last_name: None,
        fields: json!({}),
        tags: vec![],
        locale: Some("en".to_string()),
        consent_source: Some("signup form".to_string()),
        consent_at: None,
    }
}

#[sqlx::test]
async fn upsert_matches_case_insensitively(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();

    let first = ContactRepo::upsert(&mut c, &who.edit, KEY, new_contact("bob@example.com"))
        .await
        .unwrap();
    assert!(matches!(first, ContactUpsert::Created(_)));

    let mut again = new_contact(" BOB@example.com");
    again.first_name = Some("Robert".to_string());
    again.consent_source = None;
    let second = ContactRepo::upsert(&mut c, &who.owner, KEY, again)
        .await
        .unwrap();
    let ContactUpsert::Updated(updated) = second else {
        unreachable!("the second upsert should match")
    };
    assert_eq!(updated.id, first.contact().id);
    assert_eq!(updated.email, "bob@example.com");
    assert_eq!(updated.first_name.as_deref(), Some("Robert"));
    assert_eq!(updated.consent_source.as_deref(), Some("signup form"));
    assert_eq!(ContactRepo::count(&mut c, owner).await.unwrap(), 1);
}

#[sqlx::test]
async fn suppression_hash_matches_contact_hash(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact(" Bob@Example.COM "))
        .await
        .unwrap();

    let (stored,): (Vec<u8>,) = sqlx::query_as("SELECT email_hash FROM contacts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, email_hash(KEY, "bob@example.com").to_vec());

    SuppressionRepo::add(
        &mut c,
        &who.owner,
        KEY,
        "BOB@example.com ",
        SuppressionReason::Unsubscribed,
    )
    .await
    .unwrap();
    let (suppressed,): (Vec<u8>,) = sqlx::query_as("SELECT email_hash FROM suppressions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(suppressed, stored);
    assert!(
        SuppressionRepo::contains(&mut c, &who.view, email_hash(KEY, "bob@example.com"))
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn unsubscribe_suppression_marks_contact(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact("bob@example.com"))
        .await
        .unwrap()
        .contact()
        .id;
    let amy = ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact("amy@example.com"))
        .await
        .unwrap()
        .contact()
        .id;

    SuppressionRepo::add(
        &mut c,
        &who.edit,
        KEY,
        "bob@example.com",
        SuppressionReason::Complained,
    )
    .await
    .unwrap();
    SuppressionRepo::add(
        &mut c,
        &who.edit,
        KEY,
        "amy@example.com",
        SuppressionReason::Manual,
    )
    .await
    .unwrap();

    assert_eq!(
        ContactRepo::get(&mut c, &who.owner, bob)
            .await
            .unwrap()
            .status,
        ContactStatus::Complained
    );
    assert_eq!(
        ContactRepo::get(&mut c, &who.owner, amy)
            .await
            .unwrap()
            .status,
        ContactStatus::Subscribed
    );
}

#[sqlx::test]
async fn adding_a_suppression_twice_is_idempotent(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let hash = email_hash(KEY, "bob@example.com");

    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Manual)
        .await
        .unwrap();
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Bounced)
        .await
        .unwrap();
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Unsubscribed)
        .await
        .unwrap();

    let listed = SuppressionRepo::list(&mut c, &who.view, page())
        .await
        .unwrap();
    assert_eq!(listed.total, 1);
    assert_eq!(listed.data[0].reason, SuppressionReason::Bounced);
    assert_eq!(listed.data[0].email_hash, hash.to_vec());
}

#[sqlx::test]
async fn suppression_survives_contact_delete(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact("bob@example.com"))
        .await
        .unwrap()
        .contact()
        .id;
    SuppressionRepo::add(
        &mut c,
        &who.owner,
        KEY,
        "bob@example.com",
        SuppressionReason::Unsubscribed,
    )
    .await
    .unwrap();

    ContactRepo::delete(&mut c, &who.edit, bob).await.unwrap();
    assert!(
        SuppressionRepo::contains(&mut c, &who.owner, email_hash(KEY, "bob@example.com"))
            .await
            .unwrap()
    );

    let again = ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact("Bob@example.com"))
        .await
        .unwrap();
    assert!(matches!(again, ContactUpsert::Created(_)));
    assert_eq!(again.contact().status, ContactStatus::Unsubscribed);
    assert!(matches!(
        ContactRepo::set_status(
            &mut c,
            &who.owner,
            again.contact().id,
            ContactStatus::Subscribed
        )
        .await,
        Err(DataError::Conflict(reason)) if reason == "suppressed"
    ));
}

#[sqlx::test]
async fn removing_suppression_is_owner_only(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let hash = email_hash(KEY, "bob@example.com");
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Unsubscribed)
        .await
        .unwrap();

    for delegate in [&who.view, &who.edit, &who.publish, &who.limited] {
        assert!(matches!(
            SuppressionRepo::remove(&mut c, delegate, hash).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
    }
    assert!(matches!(
        SuppressionRepo::remove(&mut c, &who.stranger, hash).await,
        Err(DataError::NotFound)
    ));
    SuppressionRepo::remove(&mut c, &who.owner, hash)
        .await
        .unwrap();
    assert!(
        !SuppressionRepo::contains(&mut c, &who.owner, hash)
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn suppression_list_exposes_no_address(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    SuppressionRepo::add(
        &mut c,
        &who.owner,
        KEY,
        "bob@example.com",
        SuppressionReason::Unsubscribed,
    )
    .await
    .unwrap();

    let (leaks,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM suppressions s WHERE s::text ILIKE '%bob%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(leaks, 0);
    let listed = SuppressionRepo::list(&mut c, &who.limited, page())
        .await
        .unwrap();
    assert_eq!(listed.data.len(), 1);
    assert_eq!(listed.data[0].email_hash.len(), 32);
}

#[sqlx::test]
async fn list_member_of_other_owner_rejected(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let theirs = callers(&pool, other, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let mine = ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact("mine@example.com"))
        .await
        .unwrap()
        .contact()
        .id;
    let stray = ContactRepo::upsert(
        &mut c,
        &theirs.owner,
        KEY,
        new_contact("theirs@example.com"),
    )
    .await
    .unwrap()
    .contact()
    .id;
    let list = ContactListRepo::create(&mut c, &who.owner, list_id(), "News")
        .await
        .unwrap();

    assert!(matches!(
        ContactListRepo::add_members(&mut c, &who.owner, list.id, &[mine, stray]).await,
        Err(DataError::NotFound)
    ));
    assert_eq!(
        ContactListRepo::get(&mut c, &who.owner, list.id)
            .await
            .unwrap()
            .members,
        0
    );

    ContactListRepo::add_members(&mut c, &who.edit, list.id, &[mine, mine])
        .await
        .unwrap();
    ContactListRepo::add_members(&mut c, &who.edit, list.id, &[mine])
        .await
        .unwrap();
    assert_eq!(
        ContactListRepo::get(&mut c, &who.owner, list.id)
            .await
            .unwrap()
            .members,
        1
    );
    ContactListRepo::remove_members(&mut c, &who.edit, list.id, &[mine, stray])
        .await
        .unwrap();
    assert_eq!(
        ContactListRepo::get(&mut c, &who.owner, list.id)
            .await
            .unwrap()
            .members,
        0
    );
    assert!(matches!(
        ContactListRepo::add_members(&mut c, &who.stranger, list.id, &[]).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn contacts_isolation_matrix(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();

    assert!(matches!(
        ContactRepo::upsert(&mut c, &who.view, KEY, new_contact("bob@example.com")).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    let bob = ContactRepo::upsert(&mut c, &who.edit, KEY, new_contact("bob@example.com"))
        .await
        .unwrap()
        .contact()
        .id;
    let list = ContactListRepo::create(&mut c, &who.edit, list_id(), "News")
        .await
        .unwrap();

    for scope in [&who.owner, &who.view, &who.edit, &who.publish, &who.limited] {
        assert_eq!(
            ContactRepo::get(&mut c, scope, bob).await.unwrap().email,
            "bob@example.com"
        );
        assert_eq!(
            ContactRepo::list(&mut c, scope, page(), ContactFilter::default())
                .await
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            ContactListRepo::list(&mut c, scope, page())
                .await
                .unwrap()
                .total,
            1
        );
    }
    assert!(matches!(
        ContactRepo::set_status(&mut c, &who.view, bob, ContactStatus::Bounced).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        ContactListRepo::rename(&mut c, &who.view, list.id, "x").await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        ContactRepo::get(&mut c, &who.stranger, bob).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        ContactRepo::set_status(&mut c, &who.stranger, bob, ContactStatus::Bounced).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        ContactRepo::delete(&mut c, &who.stranger, bob).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        ContactListRepo::get(&mut c, &who.stranger, list.id).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        ContactListRepo::delete(&mut c, &who.stranger, list.id).await,
        Err(DataError::NotFound)
    ));
    assert_eq!(
        ContactRepo::list(&mut c, &who.stranger, page(), ContactFilter::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        SuppressionRepo::list(&mut c, &who.stranger, page())
            .await
            .unwrap()
            .total,
        0
    );
}

async fn ids(pool: &PgPool, scope: &OwnerScope, filter: ContactFilter) -> (u64, Vec<ContactId>) {
    let mut c = pool.acquire().await.unwrap();
    let set = ContactRepo::list(&mut c, scope, page(), filter)
        .await
        .unwrap();
    (
        set.total,
        set.data.into_iter().map(|contact| contact.id).collect(),
    )
}

#[sqlx::test]
async fn list_filters_by_list_tag_and_status(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let mut vip = new_contact("vip@example.com");
    vip.tags = vec!["vip".to_string(), "news".to_string()];
    let vip = ContactRepo::upsert(&mut c, &who.owner, KEY, vip)
        .await
        .unwrap()
        .contact()
        .id;
    let plain = ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact("plain@example.com"))
        .await
        .unwrap()
        .contact()
        .id;
    let list = ContactListRepo::create(&mut c, &who.owner, list_id(), "News")
        .await
        .unwrap();
    ContactListRepo::add_members(&mut c, &who.owner, list.id, &[plain])
        .await
        .unwrap();
    ContactRepo::set_status(&mut c, &who.owner, vip, ContactStatus::Bounced)
        .await
        .unwrap();

    assert_eq!(
        ids(
            &pool,
            &who.owner,
            ContactFilter {
                list_id: Some(list.id),
                ..ContactFilter::default()
            }
        )
        .await,
        (1, vec![plain])
    );
    assert_eq!(
        ids(
            &pool,
            &who.owner,
            ContactFilter {
                tag: Some("vip".to_string()),
                ..ContactFilter::default()
            }
        )
        .await,
        (1, vec![vip])
    );
    assert_eq!(
        ids(
            &pool,
            &who.owner,
            ContactFilter {
                status: Some(ContactStatus::Subscribed),
                ..ContactFilter::default()
            }
        )
        .await,
        (1, vec![plain])
    );
    assert_eq!(ids(&pool, &who.owner, ContactFilter::default()).await.0, 2);
}

#[sqlx::test]
async fn contact_import_records_counts_without_addresses(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let id = ContactImportId::from_uuid(Uuid::now_v7());

    assert!(matches!(
        ContactImportRepo::create(&mut c, &who.view, id, 10).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    let started = ContactImportRepo::create(&mut c, &who.edit, id, 10)
        .await
        .unwrap();
    assert_eq!(started.status, ImportStatus::Processing);
    assert_eq!(started.rows_total, 10);
    assert_eq!(started.attested_by, Some(who.edit.actor));

    let report = json!([{"row": 3, "reason": "invalid_email"}]);
    let done = ContactImportRepo::finish(&mut c, &who.edit, id, 9, 1, report.clone())
        .await
        .unwrap();
    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!((done.rows_ok, done.rows_rejected), (9, 1));
    assert_eq!(done.report, report);
    assert!(matches!(
        ContactImportRepo::finish(&mut c, &who.edit, id, 9, 1, report).await,
        Err(DataError::Conflict(reason)) if reason == "import_finished"
    ));

    assert_eq!(
        ContactImportRepo::list(&mut c, &who.limited, page())
            .await
            .unwrap()
            .total,
        1
    );
    assert!(matches!(
        ContactImportRepo::get(&mut c, &who.stranger, id).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        ContactImportRepo::finish(&mut c, &who.stranger, id, 0, 0, json!([])).await,
        Err(DataError::NotFound)
    ));
}
