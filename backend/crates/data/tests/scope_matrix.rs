mod common;

use chrono::{Duration, Utc};
use emixdb::dto::Pagination;
use postit_core::{AccountId, ContactImportId, RecurrenceId};
use postit_data::contacts::ContactImportRepo;
use postit_data::delegations::DelegationRepo;
use postit_data::posts::{PostContent, PostRepo};
use postit_data::recurrences::{NewRecurrence, PauseReason, RecurrenceRepo};
use postit_data::testkit::{callers, seed_user};
use postit_data::{Access, AccountLimit, DataError, Level, OwnerScope, ScopeError};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use common::{account, post};

async fn visible(pool: &PgPool, account: AccountId, filter: Option<Vec<Uuid>>) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM (VALUES ($1::uuid)) v(a) WHERE ($2::uuid[] IS NULL OR a = ANY($2))",
    )
    .bind(account.as_uuid())
    .bind(filter)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| unreachable!("count: {e}"))
}

#[sqlx::test]
async fn account_filter_binds_through_sqlx(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let reachable = account(&pool, owner, "reachable").await;
    let hidden = account(&pool, owner, "hidden").await;
    let c = callers(&pool, owner, &[reachable]).await;

    for scope in [&c.owner, &c.view, &c.edit, &c.publish] {
        assert_eq!(visible(&pool, hidden, scope.account_filter()).await, 1);
    }
    assert_eq!(
        visible(&pool, reachable, c.limited.account_filter()).await,
        1
    );
    assert_eq!(visible(&pool, hidden, c.limited.account_filter()).await, 0);

    let empty = callers(&pool, owner, &[]).await;
    assert_eq!(
        visible(&pool, reachable, empty.limited.account_filter()).await,
        0
    );
}

#[sqlx::test]
async fn callers_resolve_from_real_grants(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let c = callers(&pool, owner, &[a]).await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));

    assert!(matches!(c.owner.access, Access::Owner));
    assert_eq!(c.owner.actor, owner);
    assert!(matches!(c.stranger.access, Access::Owner));
    assert_ne!(c.stranger.actor, owner);
    for (scope, want_level, want_limit) in [
        (&c.view, Level::View, AccountLimit::All),
        (&c.edit, Level::Edit, AccountLimit::All),
        (&c.publish, Level::Publish, AccountLimit::All),
        (&c.limited, Level::Publish, AccountLimit::only([a])),
    ] {
        let Access::Delegated {
            delegation,
            level,
            accounts,
        } = &scope.access
        else {
            unreachable!("a delegate caller has the owner's access");
        };
        assert_eq!(scope.owner, owner);
        assert_ne!(scope.actor, owner);
        assert_eq!((*level, accounts), (want_level, &want_limit));

        let row = DelegationRepo::lock(&mut conn, &OwnerScope::own(owner), *delegation)
            .await
            .unwrap_or_else(|e| unreachable!("the caller's grant has no row: {e}"));
        assert!(row.is_live(chrono::Utc::now()));
        assert_eq!(row.delegate_id, scope.actor);
        assert_eq!(row.level, want_level);
        assert_eq!(row.accounts, want_limit);
    }
}

// Capability audit (B6): each case below pins a repository to plan 01's level table and plan
// 03's REST surface Access column where the audit found the two disagreed.

fn ok<T, E: std::fmt::Display>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|e| unreachable!("{what}: {e}"))
}

/// Contact import and its progress (`POST /contacts/import`, `GET /contacts/imports/{id}`)
/// are owner-only. The repository required `edit` to start or finish one and `view` to read
/// one.
#[sqlx::test]
async fn contact_imports_are_owner_only(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let c = callers(&pool, owner, &[]).await;
    let mut conn = ok(pool.acquire().await, "acquire");
    let id = ContactImportId::from_uuid(Uuid::now_v7());
    let page = Pagination {
        page: 1,
        page_size: 10,
    };
    let owner_only =
        |r: Result<(), DataError>| matches!(r, Err(DataError::Scope(ScopeError::OwnerOnly)));
    for scope in [&c.view, &c.edit, &c.publish, &c.limited] {
        assert!(owner_only(
            ContactImportRepo::create(&mut conn, scope, id, 1)
                .await
                .map(|_| ())
        ));
        assert!(owner_only(
            ContactImportRepo::list(&mut conn, scope, page)
                .await
                .map(|_| ())
        ));
    }
    ok(
        ContactImportRepo::create(&mut conn, &c.owner, id, 1).await,
        "owner import",
    );
    for scope in [&c.view, &c.edit, &c.publish, &c.limited] {
        assert!(owner_only(
            ContactImportRepo::get(&mut conn, scope, id)
                .await
                .map(|_| ())
        ));
        assert!(owner_only(
            ContactImportRepo::finish(&mut conn, scope, id, 1, 0, json!([]))
                .await
                .map(|_| ())
        ));
    }
    ok(
        ContactImportRepo::finish(&mut conn, &c.owner, id, 1, 0, json!([])).await,
        "owner finish",
    );
}

fn content(text: &str) -> PostContent {
    PostContent {
        reply_to: None,
        campaign_id: None,
        text: text.to_owned(),
        link: None,
        tags: vec![],
        disclose_ai_generated: false,
    }
}

/// `PATCH /posts/{id}` and `DELETE /posts/{id}` are "edit, whole post in limit". A limited
/// delegate could change the text or media of, or delete, a post with an override for an
/// account outside the limit: the rule existed only as the `outside_limit` predicate.
#[sqlx::test]
async fn post_edit_and_delete_need_the_whole_post_in_limit(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let c = callers(&pool, owner, &[a]).await;
    let id = post(&pool, owner, "draft").await;
    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(PostRepo::lock(&mut tx, &c.owner, id, None).await, "lock");
    ok(
        PostRepo::upsert_override(&mut tx, &locked, b, json!({"text": "b"}), json!({})).await,
        "override",
    );
    ok(tx.commit().await, "commit");

    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(PostRepo::lock(&mut tx, &c.limited, id, None).await, "lock");
    assert!(matches!(
        PostRepo::update_content(&mut tx, &locked, content("changed")).await,
        Err(DataError::AccountNotInScope)
    ));
    assert!(matches!(
        PostRepo::set_media(&mut tx, &locked, &[]).await,
        Err(DataError::AccountNotInScope)
    ));
    // Account-bound writes stay "account in limit": the delegate's own override is theirs.
    ok(
        PostRepo::upsert_override(&mut tx, &locked, a, json!({"text": "a"}), json!({})).await,
        "own override",
    );
    assert!(matches!(
        PostRepo::delete(&mut tx, locked).await,
        Err(DataError::AccountNotInScope)
    ));
    drop(tx);

    // A delegate without a limit passes.
    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(PostRepo::lock(&mut tx, &c.edit, id, None).await, "lock");
    ok(
        PostRepo::update_content(&mut tx, &locked, content("edited")).await,
        "edit delegate",
    );
    ok(
        PostRepo::delete(&mut tx, locked).await,
        "edit delegate delete",
    );
}

fn recurrence(targets: &[AccountId]) -> NewRecurrence {
    NewRecurrence {
        id: RecurrenceId::from_uuid(Uuid::now_v7()),
        rule: "FREQ=WEEKLY;BYDAY=MO".to_owned(),
        timezone: "Asia/Riyadh".to_owned(),
        targets: targets.to_vec(),
        rewrite: false,
        next_occurrence_at: Utc::now() + Duration::days(1),
        confirmed_version: 2,
    }
}

/// Recurrence changes are "publish, whole post in limit" and a refusal is
/// `DataError::AccountNotInScope` (403 `account_not_in_scope` at the API). The repository checked
/// only the recurrence's own targets and refused with a `recurrence_outside_limit` conflict
/// instead of the `account_not_in_scope` code the plans define.
#[sqlx::test]
async fn recurrence_changes_need_the_whole_post_in_limit(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let c = callers(&pool, owner, &[a]).await;
    let id = post(&pool, owner, "weekly").await;
    // The recurrence is inside the limit; only the override for B is not.
    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(PostRepo::lock(&mut tx, &c.owner, id, None).await, "lock");
    ok(
        PostRepo::upsert_override(&mut tx, &locked, b, json!({"text": "b"}), json!({})).await,
        "override",
    );
    ok(
        RecurrenceRepo::set(&mut tx, &locked, recurrence(&[a])).await,
        "owner recurrence",
    );
    ok(tx.commit().await, "commit");

    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(PostRepo::lock(&mut tx, &c.limited, id, None).await, "lock");
    let outside = |r: Result<(), DataError>| matches!(r, Err(DataError::AccountNotInScope));
    assert!(outside(
        RecurrenceRepo::set(&mut tx, &locked, recurrence(&[a]))
            .await
            .map(|_| ())
    ));
    assert!(outside(
        RecurrenceRepo::pause(&mut tx, &locked, PauseReason::User)
            .await
            .map(|_| ())
    ));
    assert!(outside(
        RecurrenceRepo::resume(&mut tx, &locked, Utc::now() + Duration::days(1), 3)
            .await
            .map(|_| ())
    ));
    assert!(outside(RecurrenceRepo::delete(&mut tx, &locked).await));
    drop(tx);

    // With the hidden override gone the whole post is in the limit.
    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(PostRepo::lock(&mut tx, &c.owner, id, None).await, "lock");
    ok(
        PostRepo::delete_override(&mut tx, &locked, b).await,
        "drop override",
    );
    ok(tx.commit().await, "commit");
    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(PostRepo::lock(&mut tx, &c.limited, id, None).await, "lock");
    ok(
        RecurrenceRepo::pause(&mut tx, &locked, PauseReason::User).await,
        "limited pause",
    );
}
