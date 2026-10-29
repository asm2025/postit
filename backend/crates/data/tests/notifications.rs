#![allow(clippy::unwrap_used)]

mod common;

use chrono::{DateTime, Duration, Utc};
use emixdb::dto::Pagination;
use postit_core::{
    AccountId, AccountLabel, CommentsNotice, DeliveryId, DeliveryNotice, Notice, NotificationId,
    PlatformId, PostId, PublishJobId, UserId,
};
use postit_data::DataError;
use postit_data::notifications::{NewNotification, NotificationRepo, ReadSelection};
use postit_data::testkit::seed_user;
use sqlx::PgPool;
use uuid::Uuid;

fn label() -> AccountLabel {
    AccountLabel {
        account_id: AccountId::from(Uuid::now_v7()),
        account_name: "Shop".to_owned(),
        platform: PlatformId::new("mastodon").unwrap(),
    }
}

fn failed() -> Notice {
    Notice::DeliveryFailed(DeliveryNotice {
        delivery_id: DeliveryId::from(Uuid::now_v7()),
        job_id: PublishJobId::from(Uuid::now_v7()),
        post_id: PostId::from(Uuid::now_v7()),
        account: label(),
        park_reason: None,
    })
}

fn published() -> Notice {
    Notice::DeliveryPublished(DeliveryNotice {
        delivery_id: DeliveryId::from(Uuid::now_v7()),
        job_id: PublishJobId::from(Uuid::now_v7()),
        post_id: PostId::from(Uuid::now_v7()),
        account: label(),
        park_reason: None,
    })
}

fn comments() -> Notice {
    Notice::NewComments(CommentsNotice {
        account: label(),
        count: 3,
    })
}

fn new(user: UserId, notice: Notice, email_wanted: bool, at: DateTime<Utc>) -> NewNotification {
    NewNotification {
        id: NotificationId::from(Uuid::now_v7()),
        user,
        owner: Some(user),
        notice,
        email_wanted,
        created_at: at,
    }
}

async fn add(
    pool: &PgPool,
    user: UserId,
    notice: Notice,
    email_wanted: bool,
    at: DateTime<Utc>,
) -> NotificationId {
    let n = new(user, notice, email_wanted, at);
    NotificationRepo::insert(&mut pool.acquire().await.unwrap(), &n)
        .await
        .unwrap();
    n.id
}

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

#[sqlx::test]
async fn insert_and_list_round_trip_newest_first(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let t = now();
    let older = add(&pool, user, failed(), true, t - Duration::minutes(2)).await;
    let newer = add(&pool, user, published(), true, t).await;
    let mut conn = pool.acquire().await.unwrap();

    let got = NotificationRepo::list(&mut conn, user, page())
        .await
        .unwrap();

    assert_eq!(got.total, 2);
    let ids: Vec<_> = got.data.iter().map(|n| n.id).collect();
    assert_eq!(ids, vec![newer, older]);
    assert_eq!(got.data[1].severity, postit_core::Severity::Critical);
    assert!(matches!(got.data[1].notice, Notice::DeliveryFailed(_)));
    assert_eq!(got.data[0].owner, Some(user));
}

#[sqlx::test]
async fn mark_read_ignores_other_users_ids(pool: PgPool) {
    let mine = seed_user(&pool, "active").await;
    let theirs = seed_user(&pool, "active").await;
    let a = add(&pool, mine, published(), false, Utc::now()).await;
    let b = add(&pool, theirs, published(), false, Utc::now()).await;
    let mut conn = pool.acquire().await.unwrap();

    let changed =
        NotificationRepo::mark_read(&mut conn, mine, &ReadSelection::Ids(vec![a, b]), Utc::now())
            .await
            .unwrap();

    assert_eq!(changed, 1);
    let other = NotificationRepo::get_for_user(&mut conn, theirs, b)
        .await
        .unwrap();
    assert!(other.read_at.is_none());
    let again = NotificationRepo::mark_read(&mut conn, mine, &ReadSelection::All, Utc::now())
        .await
        .unwrap();
    assert_eq!(again, 0);
}

#[sqlx::test]
async fn unread_state_counts_and_newest(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let t = now();
    let first = add(&pool, user, published(), false, t - Duration::minutes(3)).await;
    let second = add(&pool, user, published(), false, t - Duration::minutes(2)).await;
    let third = add(&pool, user, published(), false, t - Duration::minutes(1)).await;
    let mut conn = pool.acquire().await.unwrap();

    let state = NotificationRepo::unread_state(&mut conn, user)
        .await
        .unwrap();
    assert_eq!((state.count, state.newest), (3, Some(third)));

    NotificationRepo::mark_read(&mut conn, user, &ReadSelection::Ids(vec![third]), t)
        .await
        .unwrap();
    let state = NotificationRepo::unread_state(&mut conn, user)
        .await
        .unwrap();
    assert_eq!((state.count, state.newest), (2, Some(second)));

    NotificationRepo::mark_read(&mut conn, user, &ReadSelection::All, t)
        .await
        .unwrap();
    let state = NotificationRepo::unread_state(&mut conn, user)
        .await
        .unwrap();
    assert_eq!((state.count, state.newest), (0, None));
    let _ = first;
}

#[sqlx::test]
async fn take_unemailed_critical_marks_exactly_what_it_returns(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let t = now();
    let late = add(&pool, user, failed(), true, t - Duration::minutes(1)).await;
    let early = add(&pool, user, failed(), true, t - Duration::minutes(5)).await;
    let info = add(&pool, user, published(), true, t - Duration::minutes(4)).await;
    let done = add(&pool, user, failed(), false, t - Duration::minutes(3)).await;
    let mut conn = pool.acquire().await.unwrap();

    let taken = NotificationRepo::take_unemailed_critical(&mut conn, user, t)
        .await
        .unwrap();

    let ids: Vec<_> = taken.iter().map(|n| n.id).collect();
    assert_eq!(ids, vec![early, late]);
    assert!(taken.iter().all(|n| n.emailed_at == Some(t)));
    let info = NotificationRepo::get_for_user(&mut conn, user, info)
        .await
        .unwrap();
    assert!(info.emailed_at.is_none());
    let done = NotificationRepo::get_for_user(&mut conn, user, done)
        .await
        .unwrap();
    assert_eq!(done.emailed_at, Some(t - Duration::minutes(3)));
    let again = NotificationRepo::take_unemailed_critical(&mut conn, user, t)
        .await
        .unwrap();
    assert!(again.is_empty());
}

#[sqlx::test]
async fn row_not_wanted_by_email_is_settled_at_insert(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let at = now() - Duration::minutes(10);
    let id = add(&pool, user, failed(), false, at).await;
    let mut conn = pool.acquire().await.unwrap();

    let row = NotificationRepo::get_for_user(&mut conn, user, id)
        .await
        .unwrap();
    assert_eq!(row.emailed_at, Some(at));
    let taken = NotificationRepo::take_unemailed_critical(&mut conn, user, Utc::now())
        .await
        .unwrap();
    assert!(taken.is_empty());
}

#[sqlx::test]
async fn take_undigested_comments_marks_what_it_returns(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let t = now();
    let c1 = add(&pool, user, comments(), true, t - Duration::minutes(2)).await;
    let c2 = add(&pool, user, comments(), true, t - Duration::minutes(1)).await;
    let settled = add(&pool, user, comments(), false, t - Duration::minutes(3)).await;
    let crit = add(&pool, user, failed(), true, t - Duration::minutes(4)).await;
    let mut conn = pool.acquire().await.unwrap();

    let taken = NotificationRepo::take_undigested_comments(&mut conn, user, t)
        .await
        .unwrap();

    let ids: Vec<_> = taken.iter().map(|n| n.id).collect();
    assert_eq!(ids, vec![c1, c2]);
    assert!(
        NotificationRepo::take_undigested_comments(&mut conn, user, t)
            .await
            .unwrap()
            .is_empty()
    );
    let settled = NotificationRepo::get_for_user(&mut conn, user, settled)
        .await
        .unwrap();
    assert_eq!(settled.emailed_at, Some(t - Duration::minutes(3)));
    let crit = NotificationRepo::get_for_user(&mut conn, user, crit)
        .await
        .unwrap();
    assert!(crit.emailed_at.is_none());
}

#[sqlx::test]
async fn mark_pushed_records_the_first_success_only(pool: PgPool) {
    let user = seed_user(&pool, "active").await;
    let id = add(&pool, user, failed(), true, Utc::now()).await;
    let mut conn = pool.acquire().await.unwrap();
    let first = now();

    NotificationRepo::mark_pushed(&mut conn, id, first)
        .await
        .unwrap();
    NotificationRepo::mark_pushed(&mut conn, id, first + Duration::minutes(5))
        .await
        .unwrap();

    let row = NotificationRepo::get_for_user(&mut conn, user, id)
        .await
        .unwrap();
    assert_eq!(row.pushed_at, Some(first));
}

#[sqlx::test]
async fn another_users_notification_is_not_found(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let stranger = seed_user(&pool, "active").await;
    let id = add(&pool, owner, failed(), true, Utc::now()).await;
    let mut conn = pool.acquire().await.unwrap();

    let got = NotificationRepo::get_for_user(&mut conn, stranger, id).await;
    assert!(matches!(got, Err(DataError::NotFound)));
    let listed = NotificationRepo::list(&mut conn, stranger, page())
        .await
        .unwrap();
    assert_eq!(listed.total, 0);
    assert!(
        NotificationRepo::take_unemailed_critical(&mut conn, stranger, Utc::now())
            .await
            .unwrap()
            .is_empty()
    );
}

/// Now at the microsecond precision Postgres stores.
fn now() -> DateTime<Utc> {
    DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap()
}
