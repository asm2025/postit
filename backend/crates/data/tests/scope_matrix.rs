mod common;

use postit_core::AccountId;
use postit_data::delegations::DelegationRepo;
use postit_data::testkit::{callers, seed_user};
use postit_data::{Access, AccountLimit, Level, OwnerScope};
use sqlx::PgPool;
use uuid::Uuid;

use common::account;

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
