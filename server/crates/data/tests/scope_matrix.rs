use postit_core::AccountId;
use postit_data::testkit::{callers, seed_user};
use sqlx::PgPool;
use uuid::Uuid;

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
    let reachable = AccountId::from(Uuid::now_v7());
    let hidden = AccountId::from(Uuid::now_v7());
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
