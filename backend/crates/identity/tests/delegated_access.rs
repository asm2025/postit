//! Delegated access at the service layer: every delegate scope here comes from
//! [`resolve_act_as`] over a grant made through the [`DelegationService`], and acts through the
//! B2 repositories directly, so the whole-object rule, the level table, and version conflicts
//! are proven on scopes exactly as the API builds them.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use postit_core::{
    AccountId, AccountRef, DeliveryId, JobStatus, PlatformId, PostId, PostKind, PostVersion,
    PublishJobId, PublishSnapshot, RecurrenceId, SystemClock, SystemIdGenerator, UserId, Variant,
};
use postit_data::audit::AuditContext;
use postit_data::posts::{NewPost, PostContent, PostRepo};
use postit_data::publishing::{CreatedJob, DeliveryRepo, JobRepo, NewDelivery, NewJob, Operation};
use postit_data::recurrences::{NewRecurrence, RecurrenceRepo};
use postit_data::{DataError, Level, OwnerScope, ScopeError};
use postit_identity::delegation::{AccountsInput, DelegationService, GrantRequest, resolve_act_as};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn ok<T, E: std::fmt::Display>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|e| unreachable!("{what}: {e}"))
}

async fn active(pool: &PgPool, name: &str) -> UserId {
    let id = Uuid::now_v7();
    ok(
        sqlx::query(
            "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status)
             VALUES ($1, 'https://i.test', $2, $3, 'member', 'active')",
        )
        .bind(id)
        .bind(id.to_string())
        .bind(name)
        .execute(pool)
        .await,
        "user",
    );
    UserId::from(id)
}

async fn account(pool: &PgPool, owner: UserId, remote: &str) -> AccountId {
    ok(
        sqlx::query(
            "INSERT INTO platforms (id, display_name) VALUES ('mastodon', 'Mastodon')
             ON CONFLICT DO NOTHING",
        )
        .execute(pool)
        .await,
        "platform",
    );
    let id = Uuid::now_v7();
    ok(
        sqlx::query(
            "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id,
                                             display_name)
             VALUES ($1, $2, 'mastodon', $3, $3)",
        )
        .bind(id)
        .bind(owner.as_uuid())
        .bind(remote)
        .execute(pool)
        .await,
        "account",
    );
    AccountId::from(id)
}

fn service(pool: &PgPool) -> DelegationService {
    DelegationService::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(SystemClock),
    )
}

/// The owner and the delegate scope the API would build for `delegate` acting as `owner`
/// after the owner grants `level` over `accounts`.
async fn delegate_scope(
    pool: &PgPool,
    owner: UserId,
    delegate: UserId,
    level: Level,
    accounts: AccountsInput,
) -> OwnerScope {
    ok(
        service(pool)
            .grant(
                &OwnerScope::own(owner),
                GrantRequest {
                    delegate_id: delegate,
                    level,
                    accounts,
                    expires_at: None,
                },
                AuditContext::default(),
            )
            .await,
        "grant",
    );
    resolved(pool, delegate, owner)
        .await
        .unwrap_or_else(|| unreachable!("a fresh grant resolves"))
}

async fn resolved(pool: &PgPool, delegate: UserId, owner: UserId) -> Option<OwnerScope> {
    ok(
        resolve_act_as(pool, delegate, owner, Utc::now()).await,
        "resolve",
    )
}

async fn post(pool: &PgPool, owner: UserId, text: &str) -> PostId {
    let id = PostId::from_uuid(Uuid::now_v7());
    let mut conn = ok(pool.acquire().await, "acquire");
    ok(
        PostRepo::create(
            &mut conn,
            &OwnerScope::own(owner),
            NewPost {
                id,
                kind: PostKind::Post,
                reply_to: None,
                campaign_id: None,
                text: text.to_owned(),
                link: None,
                tags: vec![],
                disclose_ai_generated: false,
            },
        )
        .await,
        "post",
    );
    id
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

fn later() -> DateTime<Utc> {
    let at = Utc::now() + Duration::hours(1);
    DateTime::from_timestamp_micros(at.timestamp_micros()).unwrap_or(at)
}

fn new_job(post: PostId, version: i64, accounts: &[AccountId], at: DateTime<Utc>) -> NewJob {
    NewJob {
        id: PublishJobId::from_uuid(Uuid::now_v7()),
        snapshot: PublishSnapshot {
            post_id: post,
            post_version: PostVersion(version),
            variants: accounts
                .iter()
                .map(|account| Variant {
                    target: AccountRef {
                        platform: ok(PlatformId::new("mastodon"), "platform id"),
                        account: *account,
                        remote_id: "remote".to_owned(),
                    },
                    kind: PostKind::Post,
                    text: "hello".to_owned(),
                    media: vec![],
                    link: None,
                    tags: vec![],
                    disclose_ai_generated: false,
                    reply_to: None,
                    email: None,
                    options: serde_json::Map::new(),
                })
                .collect(),
        },
        targets: accounts
            .iter()
            .map(|account| NewDelivery {
                id: DeliveryId::from_uuid(Uuid::now_v7()),
                account: *account,
                operation: Operation::Publish,
                due_at: None,
            })
            .collect(),
        scheduled_at: at,
        native_schedule: false,
        dry_run: false,
        recurrence_id: None,
    }
}

/// Locks `post` as `scope`, creates a live job for `accounts`, and commits.
async fn schedule(
    pool: &PgPool,
    scope: &OwnerScope,
    post: PostId,
    accounts: &[AccountId],
) -> Result<CreatedJob, DataError> {
    let mut tx = ok(pool.begin().await, "begin");
    let locked = PostRepo::lock(&mut tx, scope, post, None).await?;
    let version = locked.post().version;
    let created = JobRepo::create(
        &mut tx,
        scope,
        &locked,
        new_job(post, version, accounts, later()),
    )
    .await?;
    ok(tx.commit().await, "commit");
    Ok(created)
}

async fn set_override(pool: &PgPool, owner: UserId, post: PostId, account: AccountId) {
    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(
        PostRepo::lock(&mut tx, &OwnerScope::own(owner), post, None).await,
        "lock",
    );
    ok(
        PostRepo::upsert_override(&mut tx, &locked, account, json!({"text": "o"}), json!({})).await,
        "override",
    );
    ok(tx.commit().await, "commit");
}

/// Edits the post's text as `scope` and commits on success.
async fn edit(pool: &PgPool, scope: &OwnerScope, post: PostId) -> Result<(), DataError> {
    let mut tx = ok(pool.begin().await, "begin");
    let locked = PostRepo::lock(&mut tx, scope, post, None).await?;
    PostRepo::update_content(&mut tx, &locked, content("edited")).await?;
    ok(tx.commit().await, "commit");
    Ok(())
}

/// Deletes the post as `scope` and commits on success.
async fn delete(pool: &PgPool, scope: &OwnerScope, post: PostId) -> Result<(), DataError> {
    let mut tx = ok(pool.begin().await, "begin");
    let locked = PostRepo::lock(&mut tx, scope, post, None).await?;
    PostRepo::delete(&mut tx, locked).await?;
    ok(tx.commit().await, "commit");
    Ok(())
}

async fn cancel_job(pool: &PgPool, scope: &OwnerScope, job: PublishJobId) -> Result<(), DataError> {
    let mut tx = ok(pool.begin().await, "begin");
    JobRepo::cancel(&mut tx, scope, job, Utc::now()).await?;
    ok(tx.commit().await, "commit");
    Ok(())
}

async fn post_exists(pool: &PgPool, post: PostId) -> bool {
    ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM posts WHERE id = $1)")
            .bind(post.as_uuid())
            .fetch_one(pool)
            .await,
        "exists",
    )
}

#[sqlx::test(migrations = "../data/migrations")]
async fn edit_delegate_refused_deleting_post_with_live_history_allowed_on_draft(pool: PgPool) {
    let owner = active(&pool, "owner").await;
    let editor = active(&pool, "editor").await;
    let publisher = active(&pool, "publisher").await;
    let a = account(&pool, owner, "a").await;
    let edit = delegate_scope(&pool, owner, editor, Level::Edit, AccountsInput::All).await;
    let publish = delegate_scope(&pool, owner, publisher, Level::Publish, AccountsInput::All).await;

    // Live history: a live job that was cancelled, so the post is no longer locked by it.
    let published = post(&pool, owner, "went live once").await;
    let job = ok(
        schedule(&pool, &OwnerScope::own(owner), published, &[a]).await,
        "job",
    );
    ok(
        cancel_job(&pool, &OwnerScope::own(owner), job.job).await,
        "cancel",
    );

    assert!(matches!(
        delete(&pool, &edit, published).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(post_exists(&pool, published).await);
    ok(delete(&pool, &publish, published).await, "publish delete");
    assert!(!post_exists(&pool, published).await);

    let draft = post(&pool, owner, "never published").await;
    ok(delete(&pool, &edit, draft).await, "edit delete of a draft");
    assert!(!post_exists(&pool, draft).await);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_refused_editing_post_with_override_outside_limit(pool: PgPool) {
    let owner = active(&pool, "owner").await;
    let delegate = active(&pool, "limited").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let limited = delegate_scope(
        &pool,
        owner,
        delegate,
        Level::Publish,
        AccountsInput::Only(vec![a]),
    )
    .await;

    let hidden = post(&pool, owner, "has an override for b").await;
    set_override(&pool, owner, hidden, b).await;
    assert!(matches!(
        edit(&pool, &limited, hidden).await,
        Err(DataError::AccountNotInScope)
    ));
    assert!(matches!(
        delete(&pool, &limited, hidden).await,
        Err(DataError::AccountNotInScope)
    ));
    let mut conn = ok(pool.acquire().await, "acquire");
    let unchanged = ok(
        PostRepo::get(&mut conn, &OwnerScope::own(owner), hidden).await,
        "get",
    );
    assert_eq!(unchanged.text, "has an override for b");
    drop(conn);

    let reachable = post(&pool, owner, "override for a only").await;
    set_override(&pool, owner, reachable, a).await;
    ok(edit(&pool, &limited, reachable).await, "edit in limit");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_refused_cancelling_job_with_delivery_outside_limit(pool: PgPool) {
    let owner = active(&pool, "owner").await;
    let delegate = active(&pool, "limited").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let limited = delegate_scope(
        &pool,
        owner,
        delegate,
        Level::Publish,
        AccountsInput::Only(vec![a]),
    )
    .await;
    let own = OwnerScope::own(owner);

    let both = post(&pool, owner, "a and b").await;
    let job = ok(schedule(&pool, &own, both, &[a, b]).await, "job");
    assert!(matches!(
        cancel_job(&pool, &limited, job.job).await,
        Err(DataError::AccountNotInScope)
    ));
    let mut conn = ok(pool.acquire().await, "acquire");
    let still = ok(JobRepo::get(&mut conn, &own, job.job).await, "get");
    assert_eq!(still.status, JobStatus::Scheduled);
    assert!(still.cancelled_at.is_none());

    // Once the delivery to B has ended, every open delivery is in the limit.
    let to_b = job.deliveries[1].delivery;
    ok(
        DeliveryRepo::cancel(&mut conn, &own, to_b, Utc::now()).await,
        "cancel b",
    );
    drop(conn);
    ok(
        cancel_job(&pool, &limited, job.job).await,
        "limited cancel once b ended",
    );

    let only_a = post(&pool, owner, "a only").await;
    let job = ok(schedule(&pool, &own, only_a, &[a]).await, "job");
    ok(cancel_job(&pool, &limited, job.job).await, "limited cancel");
    let mut conn = ok(pool.acquire().await, "acquire");
    let cancelled = ok(JobRepo::get(&mut conn, &own, job.job).await, "get");
    assert_eq!(cancelled.status, JobStatus::Cancelled);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_reschedule_and_recurrence_change_follow_whole_object_rule(pool: PgPool) {
    let owner = active(&pool, "owner").await;
    let delegate = active(&pool, "limited").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let limited = delegate_scope(
        &pool,
        owner,
        delegate,
        Level::Publish,
        AccountsInput::Only(vec![a]),
    )
    .await;
    let own = OwnerScope::own(owner);
    let at = later() + Duration::hours(1);

    let both = post(&pool, owner, "a and b").await;
    let job = ok(schedule(&pool, &own, both, &[a, b]).await, "job");
    let mut tx = ok(pool.begin().await, "begin");
    assert!(matches!(
        JobRepo::reschedule_pending(&mut tx, &limited, job.job, at, Utc::now()).await,
        Err(DataError::AccountNotInScope)
    ));
    drop(tx);

    let only_a = post(&pool, owner, "a only").await;
    let job = ok(schedule(&pool, &own, only_a, &[a]).await, "job");
    let mut tx = ok(pool.begin().await, "begin");
    let moved = ok(
        JobRepo::reschedule_pending(&mut tx, &limited, job.job, at, Utc::now()).await,
        "limited reschedule",
    );
    assert_eq!(moved.dispatches.len(), 1);
    assert_eq!(moved.dispatches[0].run_at, at);
    ok(tx.commit().await, "commit");

    let recurrence = |targets: &[AccountId]| NewRecurrence {
        id: RecurrenceId::from_uuid(Uuid::now_v7()),
        rule: "FREQ=WEEKLY;BYDAY=MO".to_owned(),
        timezone: "Asia/Riyadh".to_owned(),
        targets: targets.to_vec(),
        rewrite: false,
        next_occurrence_at: Utc::now() + Duration::days(1),
        confirmed_version: 2,
    };
    // The recurrence would only reach A, but the post carries an override for B.
    let hidden = post(&pool, owner, "override for b").await;
    set_override(&pool, owner, hidden, b).await;
    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(
        PostRepo::lock(&mut tx, &limited, hidden, None).await,
        "lock",
    );
    assert!(matches!(
        RecurrenceRepo::set(&mut tx, &locked, recurrence(&[a])).await,
        Err(DataError::AccountNotInScope)
    ));
    drop(tx);

    let reachable = post(&pool, owner, "a only").await;
    let mut tx = ok(pool.begin().await, "begin");
    let locked = ok(
        PostRepo::lock(&mut tx, &limited, reachable, None).await,
        "lock",
    );
    let set = ok(
        RecurrenceRepo::set(&mut tx, &locked, recurrence(&[a])).await,
        "limited recurrence",
    );
    assert_eq!(set.targets, vec![a]);
    assert_eq!(set.confirmed_by, Some(delegate));
    ok(tx.commit().await, "commit");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_owner_and_delegate_edits_one_gets_version_conflict(pool: PgPool) {
    let owner = active(&pool, "owner").await;
    let delegate = active(&pool, "editor").await;
    let editor = delegate_scope(&pool, owner, delegate, Level::Edit, AccountsInput::All).await;
    let id = post(&pool, owner, "shared").await;
    // Both read version 1 and send it as If-Match.
    let version = {
        let mut conn = ok(pool.acquire().await, "acquire");
        ok(
            PostRepo::get(&mut conn, &OwnerScope::own(owner), id).await,
            "get",
        )
        .version
    };

    let write = |scope: OwnerScope, text: &'static str| {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = ok(pool.begin().await, "begin");
            let locked = PostRepo::lock(&mut tx, &scope, id, Some(version)).await?;
            PostRepo::update_content(&mut tx, &locked, content(text)).await?;
            ok(tx.commit().await, "commit");
            Ok::<_, DataError>(text)
        })
    };
    let (first, second) = tokio::join!(
        write(OwnerScope::own(owner), "owner"),
        write(editor, "delegate")
    );
    let results = [ok(first, "join"), ok(second, "join")];
    let winners: Vec<&str> = results
        .iter()
        .filter_map(|r| r.as_ref().ok().copied())
        .collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(DataError::VersionConflict)))
            .count(),
        1,
        "{results:?}"
    );

    let mut conn = ok(pool.acquire().await, "acquire");
    let stored = ok(
        PostRepo::get(&mut conn, &OwnerScope::own(owner), id).await,
        "get",
    );
    assert_eq!(stored.version, version + 1);
    assert_eq!(stored.text, winners[0]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn jobs_scheduled_by_delegate_survive_revocation(pool: PgPool) {
    let owner = active(&pool, "owner").await;
    let delegate = active(&pool, "publisher").await;
    let a = account(&pool, owner, "a").await;
    let publish = delegate_scope(&pool, owner, delegate, Level::Publish, AccountsInput::All).await;
    let id = post(&pool, owner, "scheduled by the delegate").await;
    let job = ok(schedule(&pool, &publish, id, &[a]).await, "job");

    let grant = match &publish.access {
        postit_data::Access::Delegated { delegation, .. } => *delegation,
        postit_data::Access::Owner => unreachable!("a delegate scope"),
    };
    ok(
        service(&pool)
            .revoke(&OwnerScope::own(owner), grant, AuditContext::default())
            .await,
        "revoke",
    );
    assert!(
        resolved(&pool, delegate, owner).await.is_none(),
        "the revoked grant no longer resolves"
    );

    let mut conn = ok(pool.acquire().await, "acquire");
    let kept = ok(
        JobRepo::get(&mut conn, &OwnerScope::own(owner), job.job).await,
        "the job is still the owner's",
    );
    assert_eq!(kept.status, JobStatus::Scheduled);
    assert!(kept.cancelled_at.is_none());
    let (job_by, delivery_states): (Option<Uuid>, Vec<String>) = (
        ok(
            sqlx::query_scalar("SELECT created_by FROM publish_jobs WHERE id = $1")
                .bind(job.job.as_uuid())
                .fetch_one(&pool)
                .await,
            "created_by",
        ),
        ok(
            sqlx::query_scalar("SELECT state FROM deliveries WHERE job_id = $1")
                .bind(job.job.as_uuid())
                .fetch_all(&pool)
                .await,
            "states",
        ),
    );
    assert_eq!(job_by, Some(delegate.as_uuid()));
    assert_eq!(delivery_states, vec!["pending".to_owned()]);
}
