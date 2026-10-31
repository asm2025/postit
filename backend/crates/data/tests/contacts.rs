#![allow(clippy::unwrap_used)]

use emixdb::dto::Pagination;
use postit_core::{ContactId, ContactImportId, ContactListId};
use postit_data::contacts::{
    ContactChanges, ContactFilter, ContactImportRepo, ContactListRepo, ContactRepo, ContactStatus,
    ContactUpsert, ImportRejection, ImportStatus, NewContact, StagedRow, Suppression,
    SuppressionReason, SuppressionRepo,
};
use postit_data::email_hash::email_hash;
use postit_data::retention;
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

/// A JSON object literal as the map a contact's fields are typed as.
fn object(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match value {
        serde_json::Value::Object(map) => map,
        other => unreachable!("not an object: {other}"),
    }
}

fn new_contact(email: &str) -> NewContact {
    NewContact {
        id: contact_id(),
        email: email.to_string(),
        first_name: Some("Bob".to_string()),
        last_name: None,
        fields: Some(object(json!({}))),
        tags: Some(vec![]),
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
    assert_eq!(listed.data[0].reason, SuppressionReason::Unsubscribed);
    assert!(listed.data[0].contact_id.is_none());
    assert!(listed.data[0].email.is_none());
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
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Bounced)
        .await
        .unwrap();
    let id = SuppressionRepo::list(&mut c, &who.owner, page())
        .await
        .unwrap()
        .data[0]
        .id;

    for delegate in [&who.view, &who.edit, &who.publish, &who.limited] {
        assert!(matches!(
            SuppressionRepo::remove(&mut c, delegate, id).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
    }
    assert!(matches!(
        SuppressionRepo::remove(&mut c, &who.stranger, id).await,
        Err(DataError::NotFound)
    ));
    SuppressionRepo::remove(&mut c, &who.owner, id)
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
        ContactRepo::set_status(&mut c, &who.view, bob, ContactStatus::Unsubscribed).await,
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
        ContactRepo::set_status(&mut c, &who.stranger, bob, ContactStatus::Unsubscribed).await,
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
    vip.tags = Some(vec!["vip".to_string(), "news".to_string()]);
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
    SuppressionRepo::add(
        &mut c,
        &who.owner,
        KEY,
        "vip@example.com",
        SuppressionReason::Bounced,
    )
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

    // Contact import is owner-only (plan 01): no delegate level reaches it.
    for delegate in [&who.view, &who.edit, &who.publish, &who.limited] {
        assert!(matches!(
            ContactImportRepo::create(&mut c, delegate, id, 10, None).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
    }
    let started = ContactImportRepo::create(&mut c, &who.owner, id, 10, None)
        .await
        .unwrap();
    assert_eq!(started.status, ImportStatus::Processing);
    assert_eq!(started.rows_total, 10);
    assert_eq!(started.attested_by, Some(who.owner.actor));

    for delegate in [&who.view, &who.edit, &who.publish, &who.limited] {
        assert!(matches!(
            ContactImportRepo::stage_rows(&mut c, delegate, id, vec![]).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            ContactImportRepo::set_total(&mut c, delegate, id, 3).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            ContactImportRepo::get(&mut c, delegate, id).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
        assert!(matches!(
            ContactImportRepo::list(&mut c, delegate, page()).await,
            Err(DataError::Scope(ScopeError::OwnerOnly))
        ));
    }
    let reject = ImportRejection {
        row_no: 3,
        code: "invalid_email",
    };
    ContactImportRepo::progress(&mut c, who.owner.owner, id, 9, &[reject])
        .await
        .unwrap();
    let done = ContactImportRepo::complete(&mut c, who.owner.owner, id)
        .await
        .unwrap();
    assert_eq!(done.status, ImportStatus::Completed);
    assert_eq!((done.rows_ok, done.rows_rejected), (9, 1));
    assert_eq!(
        done.report,
        json!({"entries": [{"row": 3, "code": "invalid_email"}], "totals": {"invalid_email": 1}})
    );
    assert!(matches!(
        ContactImportRepo::complete(&mut c, who.owner.owner, id).await,
        Err(DataError::Conflict(reason)) if reason == "import_finished"
    ));
    assert!(matches!(
        ContactImportRepo::stage_rows(&mut c, &who.owner, id, vec![staged(1)]).await,
        Err(DataError::Conflict(reason)) if reason == "import_finished"
    ));
    assert!(matches!(
        ContactImportRepo::stage_rows(&mut c, &who.owner, id, vec![]).await,
        Err(DataError::Conflict(reason)) if reason == "import_finished"
    ));

    assert_eq!(
        ContactImportRepo::list(&mut c, &who.owner, page())
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
        ContactImportRepo::complete(&mut c, who.stranger.owner, id).await,
        Err(DataError::NotFound)
    ));
}

fn hash_of(email: &str) -> [u8; 32] {
    email_hash(KEY, email)
}

async fn status_of(pool: &PgPool, scope: &OwnerScope, id: ContactId) -> ContactStatus {
    let mut c = pool.acquire().await.unwrap();
    ContactRepo::get(&mut c, scope, id).await.unwrap().status
}

async fn entries(pool: &PgPool, scope: &OwnerScope) -> Vec<Suppression> {
    let mut c = pool.acquire().await.unwrap();
    SuppressionRepo::list(&mut c, scope, page())
        .await
        .unwrap()
        .data
}

async fn add_contact(pool: &PgPool, scope: &OwnerScope, email: &str) -> ContactId {
    let mut c = pool.acquire().await.unwrap();
    ContactRepo::upsert(&mut c, scope, KEY, new_contact(email))
        .await
        .unwrap()
        .contact()
        .id
}

#[sqlx::test]
async fn manual_suppression_imports_as_unsubscribed(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    SuppressionRepo::add_hash(
        &mut c,
        &who.owner,
        hash_of("bob@example.com"),
        SuppressionReason::Manual,
    )
    .await
    .unwrap();
    let bob = ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact("bob@example.com"))
        .await
        .unwrap();
    assert_eq!(bob.contact().status, ContactStatus::Unsubscribed);
}

#[sqlx::test]
async fn owner_unsubscribe_survives_delete_and_readd(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    ContactRepo::set_status(&mut c, &who.edit, bob, ContactStatus::Unsubscribed)
        .await
        .unwrap();
    let listed = entries(&pool, &who.owner).await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].reason, SuppressionReason::Manual);

    ContactRepo::delete(&mut c, &who.edit, bob).await.unwrap();
    let again = ContactRepo::upsert(&mut c, &who.owner, KEY, new_contact("bob@example.com"))
        .await
        .unwrap();
    assert_eq!(again.contact().status, ContactStatus::Unsubscribed);
}

#[sqlx::test]
async fn owner_resubscribe_removes_manual_entry(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    ContactRepo::set_status(&mut c, &who.owner, bob, ContactStatus::Unsubscribed)
        .await
        .unwrap();
    let back = ContactRepo::set_status(&mut c, &who.owner, bob, ContactStatus::Subscribed)
        .await
        .unwrap();
    assert_eq!(back.status, ContactStatus::Subscribed);
    assert!(entries(&pool, &who.owner).await.is_empty());
}

#[sqlx::test]
async fn delegate_cannot_resubscribe_manual(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    ContactRepo::set_status(&mut c, &who.edit, bob, ContactStatus::Unsubscribed)
        .await
        .unwrap();
    assert!(matches!(
        ContactRepo::set_status(&mut c, &who.edit, bob, ContactStatus::Subscribed).await,
        Err(DataError::Scope(ScopeError::OwnerOnly))
    ));
    assert_eq!(
        status_of(&pool, &who.owner, bob).await,
        ContactStatus::Unsubscribed
    );
    assert_eq!(entries(&pool, &who.owner).await.len(), 1);
}

#[sqlx::test]
async fn resubscribe_with_unsubscribed_suppression_conflicts(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    SuppressionRepo::add_hash(
        &mut c,
        &who.owner,
        hash_of("bob@example.com"),
        SuppressionReason::Unsubscribed,
    )
    .await
    .unwrap();
    assert!(matches!(
        ContactRepo::set_status(&mut c, &who.owner, bob, ContactStatus::Subscribed).await,
        Err(DataError::Conflict(reason)) if reason == "suppressed"
    ));
}

#[sqlx::test]
async fn unsubscribe_of_bounced_contact_refused(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    SuppressionRepo::add_hash(
        &mut c,
        &who.owner,
        hash_of("bob@example.com"),
        SuppressionReason::Bounced,
    )
    .await
    .unwrap();
    for target in [
        ContactStatus::Unsubscribed,
        ContactStatus::Bounced,
        ContactStatus::Complained,
    ] {
        assert!(matches!(
            ContactRepo::set_status(&mut c, &who.owner, bob, target).await,
            Err(DataError::Conflict(reason)) if reason == "status_not_settable"
        ));
    }
}

#[sqlx::test]
async fn remove_refuses_non_bounced(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    for (email, reason) in [
        ("a@example.com", SuppressionReason::Unsubscribed),
        ("b@example.com", SuppressionReason::Complained),
        ("c@example.com", SuppressionReason::Manual),
    ] {
        SuppressionRepo::add_hash(&mut c, &who.owner, hash_of(email), reason)
            .await
            .unwrap();
    }
    for entry in entries(&pool, &who.owner).await {
        assert!(matches!(
            SuppressionRepo::remove(&mut c, &who.owner, entry.id).await,
            Err(DataError::Conflict(reason)) if reason == "suppression_not_removable"
        ));
    }
    assert_eq!(entries(&pool, &who.owner).await.len(), 3);
}

#[sqlx::test]
async fn remove_bounced_resubscribes_contact(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    SuppressionRepo::add_hash(
        &mut c,
        &who.owner,
        hash_of("bob@example.com"),
        SuppressionReason::Bounced,
    )
    .await
    .unwrap();
    assert_eq!(
        status_of(&pool, &who.owner, bob).await,
        ContactStatus::Bounced
    );
    let id = entries(&pool, &who.owner).await[0].id;
    SuppressionRepo::remove(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert_eq!(
        status_of(&pool, &who.owner, bob).await,
        ContactStatus::Subscribed
    );
    assert!(entries(&pool, &who.owner).await.is_empty());
}

#[sqlx::test]
async fn remove_bounced_keeps_owner_unsubscribe(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    ContactRepo::set_status(&mut c, &who.owner, bob, ContactStatus::Unsubscribed)
        .await
        .unwrap();
    SuppressionRepo::add_hash(
        &mut c,
        &who.owner,
        hash_of("bob@example.com"),
        SuppressionReason::Bounced,
    )
    .await
    .unwrap();
    let listed = entries(&pool, &who.owner).await;
    assert_eq!(listed[0].reason, SuppressionReason::Bounced);
    SuppressionRepo::remove(&mut c, &who.owner, listed[0].id)
        .await
        .unwrap();
    let after = entries(&pool, &who.owner).await;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].reason, SuppressionReason::Manual);
    assert_eq!(
        status_of(&pool, &who.owner, bob).await,
        ContactStatus::Unsubscribed
    );
}

#[sqlx::test]
async fn remove_bounced_keeps_owner_unsubscribe_after_delete(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    ContactRepo::set_status(&mut c, &who.owner, bob, ContactStatus::Unsubscribed)
        .await
        .unwrap();
    SuppressionRepo::add_hash(
        &mut c,
        &who.owner,
        hash_of("bob@example.com"),
        SuppressionReason::Bounced,
    )
    .await
    .unwrap();
    ContactRepo::delete(&mut c, &who.owner, bob).await.unwrap();
    let listed = entries(&pool, &who.owner).await;
    assert_eq!(listed[0].reason, SuppressionReason::Bounced);
    SuppressionRepo::remove(&mut c, &who.owner, listed[0].id)
        .await
        .unwrap();
    let after = entries(&pool, &who.owner).await;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].reason, SuppressionReason::Manual);
    let again = add_contact(&pool, &who.owner, "bob@example.com").await;
    assert_eq!(
        status_of(&pool, &who.owner, again).await,
        ContactStatus::Unsubscribed
    );
}

#[sqlx::test]
async fn remove_bounced_without_owner_unsubscribe_deletes_entry(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    SuppressionRepo::add_hash(
        &mut c,
        &who.owner,
        hash_of("bob@example.com"),
        SuppressionReason::Bounced,
    )
    .await
    .unwrap();
    ContactRepo::delete(&mut c, &who.owner, bob).await.unwrap();
    let id = entries(&pool, &who.owner).await[0].id;
    SuppressionRepo::remove(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert!(entries(&pool, &who.owner).await.is_empty());
    let again = add_contact(&pool, &who.owner, "bob@example.com").await;
    assert_eq!(
        status_of(&pool, &who.owner, again).await,
        ContactStatus::Subscribed
    );
}

#[sqlx::test]
async fn suppression_list_joins_live_contact(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    SuppressionRepo::add_hash(
        &mut c,
        &who.owner,
        hash_of("bob@example.com"),
        SuppressionReason::Unsubscribed,
    )
    .await
    .unwrap();
    let listed = entries(&pool, &who.view).await;
    assert_eq!(listed[0].contact_id, Some(bob));
    assert_eq!(listed[0].email.as_deref(), Some("bob@example.com"));

    ContactRepo::delete(&mut c, &who.owner, bob).await.unwrap();
    let listed = entries(&pool, &who.view).await;
    assert_eq!(listed.len(), 1);
    assert!(listed[0].contact_id.is_none());
    assert!(listed[0].email.is_none());
}

#[sqlx::test]
async fn unsubscribe_replaces_bounced_entry(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    let hash = hash_of("bob@example.com");
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Bounced)
        .await
        .unwrap();
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Unsubscribed)
        .await
        .unwrap();
    assert_eq!(
        status_of(&pool, &who.owner, bob).await,
        ContactStatus::Unsubscribed
    );
    let listed = entries(&pool, &who.owner).await;
    assert_eq!(listed[0].reason, SuppressionReason::Unsubscribed);
    assert!(matches!(
        SuppressionRepo::remove(&mut c, &who.owner, listed[0].id).await,
        Err(DataError::Conflict(reason)) if reason == "suppression_not_removable"
    ));
}

#[sqlx::test]
async fn complaint_replaces_bounced_entry(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    let hash = hash_of("bob@example.com");
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Bounced)
        .await
        .unwrap();
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Complained)
        .await
        .unwrap();
    assert_eq!(
        status_of(&pool, &who.owner, bob).await,
        ContactStatus::Complained
    );
    assert_eq!(
        entries(&pool, &who.owner).await[0].reason,
        SuppressionReason::Complained
    );
    // Equal rank: a later unsubscribe does not replace the complaint.
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Unsubscribed)
        .await
        .unwrap();
    assert_eq!(
        entries(&pool, &who.owner).await[0].reason,
        SuppressionReason::Complained
    );
}

#[sqlx::test]
async fn bounce_never_replaces_unsubscribe(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;
    let hash = hash_of("bob@example.com");
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Unsubscribed)
        .await
        .unwrap();
    SuppressionRepo::add_hash(&mut c, &who.owner, hash, SuppressionReason::Bounced)
        .await
        .unwrap();
    assert_eq!(
        entries(&pool, &who.owner).await[0].reason,
        SuppressionReason::Unsubscribed
    );
    assert_eq!(
        status_of(&pool, &who.owner, bob).await,
        ContactStatus::Unsubscribed
    );
}

#[sqlx::test]
async fn upsert_none_keeps_existing_values(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let mut first = new_contact("bob@example.com");
    first.last_name = Some("Smith".to_string());
    first.fields = Some(object(json!({"plan": "pro"})));
    first.tags = Some(vec!["vip".to_string()]);
    first.locale = Some("ar".to_string());
    ContactRepo::upsert(&mut c, &who.owner, KEY, first)
        .await
        .unwrap();

    let mut bare = new_contact("bob@example.com");
    bare.first_name = None;
    bare.last_name = None;
    bare.fields = None;
    bare.tags = None;
    bare.locale = None;
    let ContactUpsert::Updated(kept) = ContactRepo::upsert(&mut c, &who.owner, KEY, bare)
        .await
        .unwrap()
    else {
        unreachable!("the address exists")
    };
    assert_eq!(kept.first_name.as_deref(), Some("Bob"));
    assert_eq!(kept.last_name.as_deref(), Some("Smith"));
    assert_eq!(kept.fields, json!({"plan": "pro"}));
    assert_eq!(kept.tags, vec!["vip".to_string()]);
    assert_eq!(kept.locale.as_deref(), Some("ar"));

    let mut fresh = new_contact("new@example.com");
    fresh.fields = None;
    fresh.tags = None;
    let created = ContactRepo::upsert(&mut c, &who.owner, KEY, fresh)
        .await
        .unwrap();
    assert_eq!(created.contact().fields, json!({}));
    assert!(created.contact().tags.is_empty());
}

#[sqlx::test]
async fn upsert_merges_fields_into_existing(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let mut first = new_contact("bob@example.com");
    first.fields = Some(object(json!({"plan": "pro", "city": "Cairo"})));
    ContactRepo::upsert(&mut c, &who.owner, KEY, first)
        .await
        .unwrap();

    let mut again = new_contact("bob@example.com");
    again.fields = Some(object(json!({"city": "Giza", "team": "red"})));
    let merged = ContactRepo::upsert(&mut c, &who.owner, KEY, again)
        .await
        .unwrap();
    assert_eq!(
        merged.contact().fields,
        json!({"plan": "pro", "city": "Giza", "team": "red"})
    );
}

#[sqlx::test]
async fn update_changes_only_given_columns(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let bob = add_contact(&pool, &who.owner, "bob@example.com").await;

    let changed = ContactRepo::update(
        &mut c,
        &who.edit,
        bob,
        ContactChanges {
            last_name: Some(Some("Jones".to_string())),
            locale: Some(None),
            tags: Some(vec!["a".to_string()]),
            ..ContactChanges::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(changed.first_name.as_deref(), Some("Bob"));
    assert_eq!(changed.last_name.as_deref(), Some("Jones"));
    assert_eq!(changed.locale, None);
    assert_eq!(changed.tags, vec!["a".to_string()]);
    assert_eq!(changed.fields, json!({}));
    assert_eq!(changed.consent_source.as_deref(), Some("signup form"));

    assert!(matches!(
        ContactRepo::update(&mut c, &who.view, bob, ContactChanges::default()).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        ContactRepo::update(&mut c, &who.stranger, bob, ContactChanges::default()).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn exists_matches_by_hash(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    add_contact(&pool, &who.owner, "bob@example.com").await;
    assert!(
        ContactRepo::exists(&mut c, owner, hash_of("bob@example.com"))
            .await
            .unwrap()
    );
    assert!(
        !ContactRepo::exists(&mut c, owner, hash_of("amy@example.com"))
            .await
            .unwrap()
    );
    assert!(
        !ContactRepo::exists(&mut c, other, hash_of("bob@example.com"))
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn lock_owner_takes_a_transaction_lock(pool: PgPool) {
    use sqlx::Connection;
    let owner = seed_user(&pool, "active").await;
    let mut c = pool.acquire().await.unwrap();
    let mut tx = c.begin().await.unwrap();
    ContactRepo::lock_owner(&mut tx, owner).await.unwrap();
    ContactRepo::lock_owner(&mut tx, owner).await.unwrap();
    tx.commit().await.unwrap();
}

fn staged(row_no: i32) -> StagedRow {
    StagedRow {
        row_no,
        data: json!({"email": format!("row{row_no}@example.com")}),
    }
}

fn rejects(from: i32, to: i32, code: &'static str) -> Vec<ImportRejection> {
    (from..=to)
        .map(|row_no| ImportRejection { row_no, code })
        .collect()
}

async fn new_import(pool: &PgPool, who: &postit_data::testkit::Callers) -> ContactImportId {
    let id = ContactImportId::from_uuid(Uuid::now_v7());
    let mut c = pool.acquire().await.unwrap();
    ContactImportRepo::create(&mut c, &who.owner, id, 0, None)
        .await
        .unwrap();
    id
}

async fn staged_count(pool: &PgPool, id: ContactImportId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM contact_import_rows WHERE import_id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn take_chunk_removes_rows_it_returns(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let id = new_import(&pool, &who).await;
    let mut c = pool.acquire().await.unwrap();
    let rows: Vec<StagedRow> = (1..=5).rev().map(staged).collect();
    ContactImportRepo::stage_rows(&mut c, &who.owner, id, rows)
        .await
        .unwrap();
    assert_eq!(staged_count(&pool, id).await, 5);

    let mut tx = pool.begin().await.unwrap();
    let first = ContactImportRepo::take_chunk(&mut tx, owner, id, 3)
        .await
        .unwrap();
    assert_eq!(
        first.iter().map(|r| r.row_no).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(first[0].data, staged(1).data);
    let rest = ContactImportRepo::take_chunk(&mut tx, owner, id, 3)
        .await
        .unwrap();
    assert_eq!(
        rest.iter().map(|r| r.row_no).collect::<Vec<_>>(),
        vec![4, 5]
    );
    assert!(
        ContactImportRepo::take_chunk(&mut tx, owner, id, 3)
            .await
            .unwrap()
            .is_empty()
    );
    tx.commit().await.unwrap();
    assert_eq!(staged_count(&pool, id).await, 0);

    // A rolled-back chunk puts its rows back.
    ContactImportRepo::stage_rows(&mut c, &who.owner, id, vec![staged(9)])
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    ContactImportRepo::take_chunk(&mut tx, owner, id, 10)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(staged_count(&pool, id).await, 1);
}

#[sqlx::test]
async fn progress_appends_and_caps_report_at_1000(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let id = new_import(&pool, &who).await;
    let mut c = pool.acquire().await.unwrap();

    ContactImportRepo::progress(&mut c, owner, id, 4, &rejects(1, 3, "invalid_email"))
        .await
        .unwrap();
    ContactImportRepo::progress(&mut c, owner, id, 2, &rejects(4, 4, "duplicate_in_file"))
        .await
        .unwrap();
    let got = ContactImportRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert_eq!((got.rows_ok, got.rows_rejected), (6, 4));
    assert_eq!(got.report["entries"].as_array().unwrap().len(), 4);
    assert_eq!(
        got.report["entries"][3],
        json!({"row": 4, "code": "duplicate_in_file"})
    );
    assert_eq!(
        got.report["totals"],
        json!({"invalid_email": 3, "duplicate_in_file": 1})
    );

    ContactImportRepo::progress(&mut c, owner, id, 0, &rejects(5, 1004, "invalid_email"))
        .await
        .unwrap();
    let got = ContactImportRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert_eq!(got.report["entries"].as_array().unwrap().len(), 1000);
    assert_eq!(got.rows_rejected, 1004);
}

#[sqlx::test]
async fn progress_totals_keep_counting_past_1000(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let id = new_import(&pool, &who).await;
    let mut c = pool.acquire().await.unwrap();

    ContactImportRepo::progress(&mut c, owner, id, 0, &rejects(1, 1000, "invalid_email"))
        .await
        .unwrap();
    ContactImportRepo::progress(&mut c, owner, id, 0, &rejects(1001, 1500, "invalid_email"))
        .await
        .unwrap();
    ContactImportRepo::progress(&mut c, owner, id, 0, &rejects(1501, 1502, "suppressed"))
        .await
        .unwrap();
    let got = ContactImportRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert_eq!(got.report["entries"].as_array().unwrap().len(), 1000);
    assert_eq!(got.report["entries"][999]["row"], 1000);
    assert_eq!(got.report["totals"]["invalid_email"], 1500);
    assert_eq!(got.report["totals"]["suppressed"], 2);
    assert_eq!(got.rows_rejected, 1502);
}

#[sqlx::test]
async fn abandon_fails_import_and_deletes_rows(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let id = new_import(&pool, &who).await;
    let other = new_import(&pool, &who).await;
    let mut c = pool.acquire().await.unwrap();
    for import in [id, other] {
        ContactImportRepo::stage_rows(&mut c, &who.owner, import, vec![staged(1), staged(2)])
            .await
            .unwrap();
    }

    ContactImportRepo::abandon(&mut c, owner, id).await.unwrap();
    let got = ContactImportRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert_eq!(got.status, ImportStatus::Failed);
    assert_eq!(staged_count(&pool, id).await, 0);
    assert_eq!(staged_count(&pool, other).await, 2);

    // A finished import is not touched.
    ContactImportRepo::complete(&mut c, owner, other)
        .await
        .unwrap();
    ContactImportRepo::abandon(&mut c, owner, other)
        .await
        .unwrap();
    let got = ContactImportRepo::get(&mut c, &who.owner, other)
        .await
        .unwrap();
    assert_eq!(got.status, ImportStatus::Completed);
}

#[sqlx::test]
async fn deleting_a_list_clears_list_id(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let list = list_id();
    ContactListRepo::create(&mut c, &who.owner, list, "Customers")
        .await
        .unwrap();
    let id = ContactImportId::from_uuid(Uuid::now_v7());
    let made = ContactImportRepo::create(&mut c, &who.owner, id, 0, Some(list))
        .await
        .unwrap();
    assert_eq!(made.list_id, Some(list));
    assert!(matches!(
        ContactImportRepo::create(
            &mut c,
            &who.owner,
            ContactImportId::from_uuid(Uuid::now_v7()),
            0,
            Some(list_id())
        )
        .await,
        Err(DataError::NotFound)
    ));

    ContactListRepo::delete(&mut c, &who.owner, list)
        .await
        .unwrap();
    let got = ContactImportRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert_eq!(got.list_id, None);
    assert_eq!(got.status, ImportStatus::Processing);
    assert_eq!(got.attested_by, Some(who.owner.actor));
}

async fn age_import(pool: &PgPool, id: ContactImportId, hours: i32) {
    sqlx::query(
        "UPDATE contact_imports SET updated_at = now() - make_interval(hours => $2) WHERE id = $1",
    )
    .bind(id.as_uuid())
    .bind(hours)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn stale_import_failed_and_rows_gone(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let id = new_import(&pool, &who).await;
    let mut c = pool.acquire().await.unwrap();
    ContactImportRepo::stage_rows(&mut c, &who.owner, id, vec![staged(1), staged(2)])
        .await
        .unwrap();
    age_import(&pool, id, 25).await;

    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
    assert_eq!(
        retention::fail_stale_imports(&mut c, cutoff).await.unwrap(),
        1
    );
    let got = ContactImportRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert_eq!(got.status, ImportStatus::Failed);
    assert_eq!(staged_count(&pool, id).await, 0);
    // It is failed now, so a second sweep finds nothing.
    assert_eq!(
        retention::fail_stale_imports(&mut c, cutoff).await.unwrap(),
        0
    );
}

#[sqlx::test]
async fn running_import_untouched_by_stale_sweep(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let id = new_import(&pool, &who).await;
    let done = new_import(&pool, &who).await;
    let mut c = pool.acquire().await.unwrap();
    ContactImportRepo::stage_rows(&mut c, &who.owner, id, vec![staged(1)])
        .await
        .unwrap();
    // Old but progressing: the job's progress call is a sign of life.
    age_import(&pool, id, 30).await;
    ContactImportRepo::progress(&mut c, owner, id, 1, &[])
        .await
        .unwrap();
    // Finished imports are never failed, however old.
    ContactImportRepo::complete(&mut c, owner, done)
        .await
        .unwrap();
    age_import(&pool, done, 48).await;

    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
    assert_eq!(
        retention::fail_stale_imports(&mut c, cutoff).await.unwrap(),
        0
    );
    let got = ContactImportRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap();
    assert_eq!(got.status, ImportStatus::Processing);
    assert_eq!(staged_count(&pool, id).await, 1);
    let got = ContactImportRepo::get(&mut c, &who.owner, done)
        .await
        .unwrap();
    assert_eq!(got.status, ImportStatus::Completed);
}

#[sqlx::test]
async fn deleting_owner_cascades_staged_rows(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let keeper = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let kept_who = callers(&pool, keeper, &[]).await;
    let id = new_import(&pool, &who).await;
    let kept = new_import(&pool, &kept_who).await;
    let mut c = pool.acquire().await.unwrap();
    ContactImportRepo::stage_rows(&mut c, &who.owner, id, vec![staged(1), staged(2)])
        .await
        .unwrap();
    ContactImportRepo::stage_rows(&mut c, &kept_who.owner, kept, vec![staged(1)])
        .await
        .unwrap();

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(staged_count(&pool, id).await, 0);
    assert_eq!(staged_count(&pool, kept).await, 1);
}
