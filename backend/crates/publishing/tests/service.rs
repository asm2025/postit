//! The publishing service (Task 12): job creation, reschedule, cancel, retry, and resolve, the
//! cancellation checkpoints a running publish honors, and who may call each of them.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use common::{
    Fixture, FixtureOptions, JobSpec, audits, connect, create_job, fixture, fixture_with, make_due,
    mock, notices, publishing_settings, queued, row, run, service_full, short_lease, state, upload,
};
use postit_agent_mock::MockAgent;
use postit_core::{
    AccountId, Agent, DeliveryId, DeliveryState, JobStatus, Notice, PlatformId, PostId, PostKind,
    PostVersion, PublishJobId, ResolvedOutcome, UserId, ViolationCode,
};
use postit_data::audit::AuditContext;
use postit_data::posts::{NewPost, PostRepo};
use postit_data::publishing::{DeliveryRepo, Dispatch, JobRepo};
use postit_data::testkit::{callers, grant, seed_user};
use postit_data::{AccountLimit, Level, OwnerScope, ScopeError};
use postit_oauth::AccountService;
use postit_oauth::testkit::{TestOptions, account_service_with, test_accounts_settings};
use postit_publishing::worker::run_publish;
use postit_publishing::{
    CancelReport, PublishDelivery, PublishInput, PublishingError, PublishingService,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn mock2() -> PlatformId {
    PlatformId::new("mock2").unwrap()
}

fn ctx() -> AuditContext {
    AuditContext::default()
}

/// A draft post of the owner, at version 1.
async fn new_post(pool: &PgPool, owner: UserId, text: &str) -> PostId {
    let id = PostId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();
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
    .await
    .unwrap();
    id
}

/// The post's current version.
async fn version(pool: &PgPool, post: PostId) -> PostVersion {
    let version: i32 = sqlx::query_scalar("SELECT version FROM posts WHERE id = $1")
        .bind(post.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    PostVersion(i64::from(version))
}

/// Sets the mock's options for `account` on `post` (bumping the post's version).
async fn set_options(
    pool: &PgPool,
    owner: UserId,
    post: PostId,
    account: AccountId,
    options: Value,
) {
    let scope = OwnerScope::own(owner);
    let mut tx = pool.begin().await.unwrap();
    let locked = PostRepo::lock(&mut tx, &scope, post, None).await.unwrap();
    PostRepo::upsert_override(&mut tx, &locked, account, json!({}), options)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// A live publish to `targets` now, confirmed, at `if_match`.
fn live(targets: &[AccountId], if_match: PostVersion) -> PublishInput {
    PublishInput {
        targets: targets.to_vec(),
        at: None,
        native_schedule: false,
        dry_run: false,
        confirm: true,
        if_match,
    }
}

async fn publish(
    fx: &Fixture,
    scope: &OwnerScope,
    post: PostId,
    input: PublishInput,
) -> Result<postit_data::publishing::CreatedJob, PublishingError> {
    fx.service.create_job(scope, post, input, ctx()).await
}

/// A post published live to the fixture's account now, and the delivery's dispatch.
async fn one_live_job(fx: &Fixture) -> (PostId, PublishJobId, Dispatch) {
    let pool = &fx.env.pool;
    let post = new_post(pool, fx.env.owner, "hello").await;
    let created = publish(fx, &fx.env.scope, post, live(&[fx.account], PostVersion(1)))
        .await
        .unwrap();
    (post, created.job, created.deliveries[0])
}

async fn job_status(pool: &PgPool, job: PublishJobId) -> JobStatus {
    let status: String = sqlx::query_scalar("SELECT status FROM publish_jobs WHERE id = $1")
        .bind(job.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    serde_json::from_value(Value::String(status)).unwrap()
}

async fn job_version(pool: &PgPool, job: PublishJobId) -> i64 {
    let version: i32 = sqlx::query_scalar("SELECT version FROM publish_jobs WHERE id = $1")
        .bind(job.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    i64::from(version)
}

async fn jobs_of(pool: &PgPool, post: PostId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM publish_jobs WHERE post_id = $1")
        .bind(post.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_state(pool: &PgPool, id: DeliveryId, state: &str) {
    sqlx::query("UPDATE deliveries SET state = $2 WHERE id = $1")
        .bind(id.as_uuid())
        .bind(state)
        .execute(pool)
        .await
        .unwrap();
}

async fn remote_publish_at(pool: &PgPool, id: DeliveryId) -> Option<DateTime<Utc>> {
    sqlx::query_scalar("SELECT remote_publish_at FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

fn micros(time: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(time.timestamp_micros()).unwrap()
}

fn partials(all: &[Notice]) -> usize {
    all.iter()
        .filter(|n| matches!(n, Notice::JobPartial(_)))
        .count()
}

/// The dispatch of `created`'s delivery to `account`.
async fn dispatch_to(
    pool: &PgPool,
    created: &postit_data::publishing::CreatedJob,
    account: AccountId,
) -> Dispatch {
    let id: Uuid =
        sqlx::query_scalar("SELECT id FROM deliveries WHERE job_id = $1 AND account_id = $2")
            .bind(created.job.as_uuid())
            .bind(account.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap();
    created
        .deliveries
        .iter()
        .copied()
        .find(|d| d.delivery.as_uuid() == id)
        .unwrap()
}

/// Connects a `platform` account named `name` for `scope`'s owner.
async fn connect_on(
    accounts: &AccountService,
    scope: &OwnerScope,
    platform: &PlatformId,
    name: &str,
) -> AccountId {
    let Value::Object(values) = json!({"account_name": name, "token": "s3cret-token-value"}) else {
        unreachable!()
    };
    let check = accounts
        .start_credentials(scope, platform, values)
        .await
        .unwrap();
    let connect_id = check.pending.unwrap().0;
    let confirmed = accounts
        .confirm(scope, connect_id.as_str(), None, ctx())
        .await
        .unwrap();
    confirmed.connected[0].0.id
}

/// A fixture whose build also has `mock2`, the broadcast mock without native scheduling, and
/// one connected `mock2` account.
async fn with_mock2(pool: &PgPool) -> (Fixture, AccountId, postit_agent_mock::MockScript) {
    let agent = MockAgent::broadcast()
        .with_platform(mock2())
        .without_native_schedule();
    let script = agent.script();
    let agent: Arc<dyn Agent> = Arc::new(agent);
    let fx = fixture_with(
        pool,
        FixtureOptions {
            extra: vec![(agent, None)],
            compiled: Some(vec![mock(), mock2()]),
            ..FixtureOptions::default()
        },
    )
    .await;
    let other = connect_on(&fx.accounts, &fx.env.scope, &mock2(), "second").await;
    (fx, other, script)
}

/// A service over the fixture's database whose `mock` agent cannot delete or move natively
/// scheduled posts.
fn service_without_native_scheduler(fx: &Fixture) -> PublishingService {
    let agent: Arc<dyn Agent> = Arc::new(MockAgent::broadcast().without_native_schedule());
    let accounts = account_service_with(
        fx.env.pool.clone(),
        vec![agent],
        test_accounts_settings(),
        TestOptions::default(),
    );
    service_full(
        &fx.env,
        accounts,
        vec![mock()],
        publishing_settings(),
        CancellationToken::new(),
    )
}

/// A live native-scheduled job an hour ahead, uploaded: its delivery is `processing`.
async fn native_upload(fx: &Fixture, at: DateTime<Utc>) -> (PublishJobId, Dispatch) {
    let pool = &fx.env.pool;
    let post = new_post(pool, fx.env.owner, "later").await;
    let created = publish(
        fx,
        &fx.env.scope,
        post,
        PublishInput {
            at: Some(at),
            native_schedule: true,
            ..live(&[fx.account], PostVersion(1))
        },
    )
    .await
    .unwrap();
    let dispatch = created.deliveries[0];
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
    (created.job, dispatch)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stale_if_match_is_refused(pool: PgPool) {
    let fx = fixture(&pool).await;
    let post = new_post(&pool, fx.env.owner, "hello").await;
    set_options(&pool, fx.env.owner, post, fx.account, json!({})).await;
    assert_eq!(version(&pool, post).await, PostVersion(2));

    let err = publish(
        &fx,
        &fx.env.scope,
        post,
        live(&[fx.account], PostVersion(1)),
    )
    .await
    .unwrap_err();

    assert!(matches!(err, PublishingError::VersionConflict), "{err:?}");
    assert_eq!(jobs_of(&pool, post).await, 0);
    publish(
        &fx,
        &fx.env.scope,
        post,
        live(&[fx.account], PostVersion(2)),
    )
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn live_publish_needs_confirm(pool: PgPool) {
    let fx = fixture(&pool).await;
    let post = new_post(&pool, fx.env.owner, "hello").await;

    let err = publish(
        &fx,
        &fx.env.scope,
        post,
        PublishInput {
            confirm: false,
            ..live(&[fx.account], PostVersion(1))
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, PublishingError::ConfirmationRequired));
    assert_eq!(jobs_of(&pool, post).await, 0);
    // A dry run needs no confirmation and records no `publish_confirmed`.
    publish(
        &fx,
        &fx.env.scope,
        post,
        PublishInput {
            confirm: false,
            dry_run: true,
            ..live(&[fx.account], PostVersion(1))
        },
    )
    .await
    .unwrap();
    assert_eq!(audits(&pool, "publish_confirmed").await, 0);
    // The dry run holds that post until it ends; publish another one live.
    let other = new_post(&pool, fx.env.owner, "hello").await;
    publish(
        &fx,
        &fx.env.scope,
        other,
        live(&[fx.account], PostVersion(1)),
    )
    .await
    .unwrap();
    assert_eq!(audits(&pool, "publish_confirmed").await, 1);
    let details: Value =
        sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = 'publish_confirmed'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(details["post_kind"], json!("post"));
    assert_eq!(details["account_ids"], json!([fx.account.to_string()]));
    assert!(!details.to_string().contains("hello"), "no post text");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn schedule_up_to_a_minute_past_is_now(pool: PgPool) {
    let fx = fixture(&pool).await;
    let post = new_post(&pool, fx.env.owner, "hello").await;
    let before = Utc::now();

    let err = publish(
        &fx,
        &fx.env.scope,
        post,
        PublishInput {
            at: Some(before - chrono::Duration::seconds(90)),
            ..live(&[fx.account], PostVersion(1))
        },
    )
    .await
    .unwrap_err();
    let PublishingError::ValidationFailed(violations) = err else {
        panic!("{err:?}");
    };
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].code, ViolationCode::ScheduleInPast);

    let created = publish(
        &fx,
        &fx.env.scope,
        post,
        PublishInput {
            at: Some(before - chrono::Duration::seconds(30)),
            ..live(&[fx.account], PostVersion(1))
        },
    )
    .await
    .unwrap();
    let dispatch = created.deliveries[0];
    assert!(dispatch.run_at >= micros(before), "treated as now");
    let jobs = queued(&pool, "publish_delivery").await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].0,
        json!({"delivery": dispatch.delivery.as_uuid(), "dispatch_gen": 0})
    );
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn mixed_native_and_postit_targets_in_one_job(pool: PgPool) {
    let (fx, other, other_script) = with_mock2(&pool).await;
    let post = new_post(&pool, fx.env.owner, "hello").await;
    let at = micros(Utc::now() + chrono::Duration::hours(1));

    let created = publish(
        &fx,
        &fx.env.scope,
        post,
        PublishInput {
            at: Some(at),
            native_schedule: true,
            ..live(&[fx.account, other], PostVersion(1))
        },
    )
    .await
    .unwrap();

    let native = dispatch_to(&pool, &created, fx.account).await;
    let postit = dispatch_to(&pool, &created, other).await;
    // The native target uploads now; the other waits for the time.
    assert!(native.run_at < at - chrono::Duration::minutes(30));
    assert_eq!(postit.run_at, at);

    run(&fx.service, native).await;
    assert_eq!(
        state(&pool, native.delivery).await,
        DeliveryState::Processing
    );
    assert_eq!(remote_publish_at(&pool, native.delivery).await, Some(at));
    assert_eq!(fx.script.posts()[0].publish_at, Some(at));

    // The job's time comes: the other target is due and postit publishes it.
    sqlx::query("UPDATE publish_jobs SET scheduled_at = now() WHERE id = $1")
        .bind(created.job.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    make_due(&pool, postit.delivery).await;
    run(&fx.service, postit).await;
    assert_eq!(
        state(&pool, postit.delivery).await,
        DeliveryState::Published
    );
    let posts = other_script.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].publish_at, None, "postit scheduled it");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn edit_delegate_can_dry_run_but_not_publish_live(pool: PgPool) {
    let fx = fixture(&pool).await;
    let who = callers(&pool, fx.env.owner, &[]).await;
    let post = new_post(&pool, fx.env.owner, "hello").await;

    let err = publish(&fx, &who.edit, post, live(&[fx.account], PostVersion(1)))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        PublishingError::Scope(ScopeError::InsufficientAccess)
    ));

    let created = publish(
        &fx,
        &who.edit,
        post,
        PublishInput {
            dry_run: true,
            confirm: false,
            ..live(&[fx.account], PostVersion(1))
        },
    )
    .await
    .unwrap();
    run(&fx.service, created.deliveries[0]).await;
    assert_eq!(
        state(&pool, created.deliveries[0].delivery).await,
        DeliveryState::Simulated
    );
    let err = publish(
        &fx,
        &who.view,
        post,
        PublishInput {
            dry_run: true,
            ..live(&[fx.account], PostVersion(1))
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PublishingError::Scope(_)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_cannot_target_outside_limit(pool: PgPool) {
    let fx = fixture(&pool).await;
    let second = connect(&fx.accounts, &fx.env.scope, "second").await;
    let who = callers(&pool, fx.env.owner, &[second]).await;
    let post = new_post(&pool, fx.env.owner, "hello").await;

    let err = publish(
        &fx,
        &who.limited,
        post,
        live(&[fx.account, second], PostVersion(1)),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, PublishingError::AccountNotInScope), "{err:?}");
    assert_eq!(jobs_of(&pool, post).await, 0);

    let created = publish(&fx, &who.limited, post, live(&[second], PostVersion(1)))
        .await
        .unwrap();
    let creator: Uuid = sqlx::query_scalar("SELECT created_by FROM publish_jobs WHERE id = $1")
        .bind(created.job.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(creator, who.limited.actor.as_uuid());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reschedule_moves_pending_and_native_uploads(pool: PgPool) {
    let (fx, other, _) = with_mock2(&pool).await;
    let post = new_post(&pool, fx.env.owner, "hello").await;
    let at = micros(Utc::now() + chrono::Duration::hours(1));
    let created = publish(
        &fx,
        &fx.env.scope,
        post,
        PublishInput {
            at: Some(at),
            native_schedule: true,
            ..live(&[fx.account, other], PostVersion(1))
        },
    )
    .await
    .unwrap();
    let native = created
        .deliveries
        .iter()
        .copied()
        .find(|d| d.run_at < at)
        .unwrap();
    let pending = created
        .deliveries
        .iter()
        .copied()
        .find(|d| d.run_at == at)
        .unwrap();
    run(&fx.service, native).await;
    assert_eq!(job_status(&pool, created.job).await, JobStatus::Scheduled);
    let later = micros(at + chrono::Duration::hours(1));

    let stale = job_version(&pool, created.job).await - 1;
    let err = fx
        .service
        .reschedule(&fx.env.scope, created.job, later, stale, ctx())
        .await
        .unwrap_err();
    assert!(matches!(err, PublishingError::VersionConflict), "{err:?}");

    let current = job_version(&pool, created.job).await;
    let moved = fx
        .service
        .reschedule(&fx.env.scope, created.job, later, current, ctx())
        .await
        .unwrap();

    let mut ids = moved.moved.clone();
    ids.sort_by_key(DeliveryId::as_uuid);
    let mut expected = vec![native.delivery, pending.delivery];
    expected.sort_by_key(DeliveryId::as_uuid);
    assert_eq!(ids, expected);
    assert!(moved.failed.is_empty());
    let stored = row(&pool, pending.delivery).await;
    assert_eq!(stored.dispatch_gen, 1);
    assert_eq!(stored.next_attempt_at, Some(later));
    assert!(
        queued(&pool, "publish_delivery")
            .await
            .iter()
            .any(|(payload, run_at)| payload["dispatch_gen"] == json!(1) && *run_at == later)
    );
    assert_eq!(remote_publish_at(&pool, native.delivery).await, Some(later));
    assert_eq!(fx.script.posts()[0].publish_at, Some(later));
    // The upload's poll chain also wakes at the new time.
    assert!(queued(&pool, "poll_delivery_status").await.iter().any(
        |(payload, run_at)| payload["delivery"] == json!(native.delivery.as_uuid())
            && *run_at == later
    ));
    assert_eq!(audits(&pool, "publish_job_rescheduled").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn reschedule_refused_while_in_flight(pool: PgPool) {
    let fx = fixture(&pool).await;
    let gate = fx.script.hold_publishes();
    let (_, job, dispatch) = one_live_job(&fx).await;
    let worker = {
        let service = fx.service.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);

    let at = Utc::now() + chrono::Duration::hours(2);
    let current = job_version(&pool, job).await;
    let err = fx
        .service
        .reschedule(&fx.env.scope, job, at, current, ctx())
        .await
        .unwrap_err();

    assert!(matches!(err, PublishingError::NotReschedulable), "{err:?}");
    gate.release();
    worker.await.unwrap().unwrap();
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(audits(&pool, "publish_job_rescheduled").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_at_each_safe_checkpoint(pool: PgPool) {
    // Between two upload chunks: the renewer sees the request and the agent stops.
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            settings: Some(short_lease()),
            ..FixtureOptions::default()
        },
    )
    .await;
    fx.script.set_publish_steps(3);
    let gate = fx.script.hold_after_chunk(1);
    let (_, _, dispatch) = one_live_job(&fx).await;
    let worker = {
        let service = fx.service.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    let report = fx
        .service
        .cancel_delivery(&fx.env.scope, dispatch.delivery, ctx())
        .await
        .unwrap();
    assert_eq!(
        report,
        CancelReport {
            requested: vec![dispatch.delivery],
            ..CancelReport::default()
        }
    );
    // The renewer runs every 200 ms and sees the request.
    tokio::time::sleep(Duration::from_millis(700)).await;
    gate.release();
    worker.await.unwrap().unwrap();
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(fx.script.chunk_uploads(), 1, "stopped between chunks");
    assert!(fx.script.posts().is_empty());
    assert_eq!(audits(&pool, "delivery_cancelled").await, 1);

    // Before the first mutating call: a run whose claim already carries the request (it was
    // recorded while an earlier run was stopped by a shutdown) stops before calling out.
    let stopping = CancellationToken::new();
    let stopped = service_full(
        &fx.env,
        fx.accounts.clone(),
        vec![mock()],
        publishing_settings(),
        stopping.clone(),
    );
    let gate = fx.script.hold_after_chunk(1);
    let (_, _, second) = one_live_job_on(&fx, "again").await;
    let worker = {
        let service = stopped.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(second)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    fx.service
        .cancel_delivery(&fx.env.scope, second.delivery, ctx())
        .await
        .unwrap();
    stopping.cancel();
    gate.release();
    worker.await.unwrap().unwrap();
    assert_eq!(state(&pool, second.delivery).await, DeliveryState::Pending);
    let calls = fx.script.mutating_calls();

    run(
        &fx.service,
        Dispatch {
            dispatch_gen: 1,
            ..second
        },
    )
    .await;

    assert_eq!(
        state(&pool, second.delivery).await,
        DeliveryState::Cancelled
    );
    assert_eq!(fx.script.mutating_calls(), calls, "nothing called out");
    assert!(fx.script.posts().is_empty());
}

/// [`one_live_job`] for a post with `text`.
async fn one_live_job_on(fx: &Fixture, text: &str) -> (PostId, PublishJobId, Dispatch) {
    let pool = &fx.env.pool;
    let post = new_post(pool, fx.env.owner, text).await;
    let created = publish(fx, &fx.env.scope, post, live(&[fx.account], PostVersion(1)))
        .await
        .unwrap();
    (post, created.job, created.deliveries[0])
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_after_final_call_finishes(pool: PgPool) {
    let fx = fixture_with(
        &pool,
        FixtureOptions {
            settings: Some(short_lease()),
            ..FixtureOptions::default()
        },
    )
    .await;
    // One step: once past its first checkpoint, the agent's next call is the final create.
    let gate = fx.script.hold_publishes();
    let (_, _, dispatch) = one_live_job(&fx).await;
    let worker = {
        let service = fx.service.clone();
        tokio::spawn(async move { run_publish(&service, PublishDelivery::from(dispatch)).await })
    };
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    let report = fx
        .service
        .cancel_delivery(&fx.env.scope, dispatch.delivery, ctx())
        .await
        .unwrap();
    assert_eq!(report.requested, vec![dispatch.delivery]);
    tokio::time::sleep(Duration::from_millis(700)).await;
    gate.release();
    worker.await.unwrap().unwrap();

    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(fx.script.posts().len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_cancel_with_and_without_agent_delete(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let (job, deleted) = native_upload(&fx, at).await;
    assert_eq!(fx.script.posts().len(), 1);

    let report = fx
        .service
        .cancel_job(&fx.env.scope, job, ctx())
        .await
        .unwrap();

    assert_eq!(report.cancelled, vec![deleted.delivery]);
    assert!(report.not_cancellable.is_empty());
    assert_eq!(
        state(&pool, deleted.delivery).await,
        DeliveryState::Cancelled
    );
    assert!(fx.script.posts().is_empty(), "deleted on the platform");
    assert_eq!(job_status(&pool, job).await, JobStatus::Cancelled);
    assert_eq!(audits(&pool, "publish_job_cancelled").await, 1);

    // An agent that cannot delete: the upload stays and is listed.
    let (_, kept) = native_upload(&fx, at).await;
    let without = service_without_native_scheduler(&fx);
    let report = without
        .cancel_delivery(&fx.env.scope, kept.delivery, ctx())
        .await
        .unwrap();
    assert_eq!(
        report,
        CancelReport {
            not_cancellable: vec![kept.delivery],
            ..CancelReport::default()
        }
    );
    assert_eq!(state(&pool, kept.delivery).await, DeliveryState::Processing);
    assert_eq!(fx.script.posts().len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_cancel_of_an_upload_the_platform_dropped_is_not_cancellable(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let (_, dispatch) = native_upload(&fx, at).await;
    // The platform already dropped the upload: the agent's delete fails.
    sqlx::query("UPDATE deliveries SET remote_id = 'gone' WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let report = fx
        .service
        .cancel_delivery(&fx.env.scope, dispatch.delivery, ctx())
        .await
        .unwrap();

    assert_eq!(report.not_cancellable, vec![dispatch.delivery]);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_cancel_of_a_delivery_redispatched_meanwhile_is_not_cancellable(pool: PgPool) {
    let fx = fixture(&pool).await;
    let at = Utc::now() + chrono::Duration::hours(1);
    let (_, dispatch) = native_upload(&fx, at).await;
    let gate = fx.script.hold_deletes();
    let service = fx.service.clone();
    let scope = fx.env.scope.clone();
    let delivery = dispatch.delivery;
    let cancel =
        tokio::spawn(async move { service.cancel_delivery(&scope, delivery, ctx()).await });
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    // While the platform deletes, the delivery moves to a new generation.
    sqlx::query("UPDATE deliveries SET dispatch_gen = dispatch_gen + 1 WHERE id = $1")
        .bind(delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    gate.release();

    let report = cancel.await.unwrap().unwrap();

    assert_eq!(
        report,
        CancelReport {
            not_cancellable: vec![delivery],
            ..CancelReport::default()
        }
    );
    assert_eq!(state(&pool, delivery).await, DeliveryState::Processing);
    assert_eq!(
        row(&pool, delivery).await.dispatch_gen,
        dispatch.dispatch_gen + 1
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_cancel_write_failure_is_per_delivery(pool: PgPool) {
    let fx = fixture(&pool).await;
    let second = connect(&fx.accounts, &fx.env.scope, "second").await;
    let post = new_post(&pool, fx.env.owner, "later").await;
    let created = publish(
        &fx,
        &fx.env.scope,
        post,
        PublishInput {
            at: Some(Utc::now() + chrono::Duration::hours(1)),
            native_schedule: true,
            ..live(&[fx.account, second], PostVersion(1))
        },
    )
    .await
    .unwrap();
    for dispatch in &created.deliveries {
        run(&fx.service, *dispatch).await;
        assert_eq!(
            state(&pool, dispatch.delivery).await,
            DeliveryState::Processing
        );
    }
    let (broken, fine) = (
        created.deliveries[0].delivery,
        created.deliveries[1].delivery,
    );
    // The cancel of the first delivery cannot be written after its platform delete.
    sqlx::query(
        "CREATE FUNCTION refuse_cancel() RETURNS trigger LANGUAGE plpgsql AS          $$ BEGIN RAISE EXCEPTION 'cancel refused'; END $$",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TRIGGER refuse_cancel BEFORE UPDATE ON deliveries FOR EACH ROW          WHEN (OLD.id = '{}' AND NEW.state = 'cancelled') EXECUTE FUNCTION refuse_cancel()",
        broken.as_uuid()
    )))
    .execute(&pool)
    .await
    .unwrap();

    let report = fx
        .service
        .cancel_job(&fx.env.scope, created.job, ctx())
        .await
        .unwrap();

    assert_eq!(report.not_cancellable, vec![broken]);
    assert_eq!(report.cancelled, vec![fine]);
    assert_eq!(state(&pool, broken).await, DeliveryState::Processing);
    assert_eq!(state(&pool, fine).await, DeliveryState::Cancelled);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn outcome_unknown_cannot_be_cancelled(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("outcome_unknown_landed"),
    )
    .await[0];
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );

    let report = fx
        .service
        .cancel_delivery(&fx.env.scope, dispatch.delivery, ctx())
        .await
        .unwrap();

    assert_eq!(
        report,
        CancelReport {
            not_cancellable: vec![dispatch.delivery],
            ..CancelReport::default()
        }
    );
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::OutcomeUnknown
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_requires_confirm_and_acknowledgement(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(
        &pool,
        fx.env.owner,
        &[fx.account],
        JobSpec::outcome("outcome_unknown_landed"),
    )
    .await[0];
    run(&fx.service, dispatch).await;
    let id = dispatch.delivery;

    let err = fx
        .service
        .retry(&fx.env.scope, id, false, true, ctx())
        .await
        .unwrap_err();
    assert!(matches!(err, PublishingError::ConfirmationRequired));
    let err = fx
        .service
        .retry(&fx.env.scope, id, true, false, ctx())
        .await
        .unwrap_err();
    assert!(matches!(err, PublishingError::AcknowledgementRequired));
    assert_eq!(state(&pool, id).await, DeliveryState::OutcomeUnknown);

    let retried = fx
        .service
        .retry(&fx.env.scope, id, true, true, ctx())
        .await
        .unwrap();

    assert_eq!(retried.delivery, id);
    assert_eq!(state(&pool, id).await, DeliveryState::Pending);
    let generation = retried.dispatch_gen;
    assert!(
        queued(&pool, "publish_delivery")
            .await
            .iter()
            .any(|(payload, _)| payload["dispatch_gen"] == json!(generation))
    );
    assert_eq!(audits(&pool, "delivery_retried").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_revalidates_deleted_media(pool: PgPool) {
    let fx = fixture(&pool).await;
    let asset = upload(&fx.env, "a.png").await;
    let spec = JobSpec {
        media: vec![asset],
        ..JobSpec::outcome("permanent_failure")
    };
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];
    run(&fx.service, dispatch).await;
    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
    fx.env.media.delete(&fx.env.scope, asset).await.unwrap();

    let err = fx
        .service
        .retry(&fx.env.scope, dispatch.delivery, true, false, ctx())
        .await
        .unwrap_err();

    let PublishingError::ValidationFailed(violations) = err else {
        panic!("{err:?}");
    };
    assert_eq!(violations[0].code, ViolationCode::MediaMissing);
    assert_eq!(state(&pool, dispatch.delivery).await, DeliveryState::Failed);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_of_a_published_delivery_is_not_retryable_before_revalidation(pool: PgPool) {
    let fx = fixture(&pool).await;
    let asset = upload(&fx.env, "a.png").await;
    let spec = JobSpec {
        media: vec![asset],
        ..JobSpec::live()
    };
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], spec).await[0];
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    fx.env.media.delete(&fx.env.scope, asset).await.unwrap();

    let err = fx
        .service
        .retry(&fx.env.scope, dispatch.delivery, true, false, ctx())
        .await
        .unwrap_err();

    assert!(matches!(err, PublishingError::NotRetryable), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_refused_while_post_has_another_live_job(pool: PgPool) {
    let fx = fixture(&pool).await;
    let post = new_post(&pool, fx.env.owner, "hello").await;
    set_options(
        &pool,
        fx.env.owner,
        post,
        fx.account,
        json!({"outcome": "permanent_failure"}),
    )
    .await;
    let first = publish(
        &fx,
        &fx.env.scope,
        post,
        live(&[fx.account], PostVersion(2)),
    )
    .await
    .unwrap();
    run(&fx.service, first.deliveries[0]).await;
    assert_eq!(job_status(&pool, first.job).await, JobStatus::Failed);
    publish(
        &fx,
        &fx.env.scope,
        post,
        PublishInput {
            at: Some(Utc::now() + chrono::Duration::hours(1)),
            ..live(&[fx.account], PostVersion(2))
        },
    )
    .await
    .unwrap();

    let err = fx
        .service
        .retry(
            &fx.env.scope,
            first.deliveries[0].delivery,
            true,
            false,
            ctx(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, PublishingError::JobActive), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn resolve_from_dead_letter(pool: PgPool) {
    let fx = fixture(&pool).await;
    let dispatch = create_job(&pool, fx.env.owner, &[fx.account], JobSpec::live()).await[0];
    set_state(&pool, dispatch.delivery, "dead_letter").await;

    let resolved = fx
        .service
        .resolve(
            &fx.env.scope,
            dispatch.delivery,
            ResolvedOutcome::Published,
            ctx(),
        )
        .await
        .unwrap();

    assert_eq!(resolved, DeliveryState::Published);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(audits(&pool, "delivery_resolved").await, 1);
    let err = fx
        .service
        .resolve(
            &fx.env.scope,
            dispatch.delivery,
            ResolvedOutcome::Failed,
            ctx(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, PublishingError::NotResolvable), "{err:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_and_resolve_record_job_partial_once_for_live_jobs_only(pool: PgPool) {
    let fx = fixture(&pool).await;
    let second = connect(&fx.accounts, &fx.env.scope, "second").await;

    // A cancel that ends the job with one delivery out: `job_partial`, once.
    let dispatches = create_job(&pool, fx.env.owner, &[fx.account, second], JobSpec::live()).await;
    run(&fx.service, dispatches[0]).await;
    let job: Uuid = sqlx::query_scalar("SELECT job_id FROM deliveries WHERE id = $1")
        .bind(dispatches[0].delivery.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    let job = PublishJobId::from_uuid(job);
    let report = fx
        .service
        .cancel_job(&fx.env.scope, job, ctx())
        .await
        .unwrap();
    assert_eq!(report.cancelled, vec![dispatches[1].delivery]);
    assert_eq!(job_status(&pool, job).await, JobStatus::Partial);
    assert_eq!(partials(&notices(&pool, fx.env.owner).await), 1);
    let again = fx
        .service
        .cancel_job(&fx.env.scope, job, ctx())
        .await
        .unwrap();
    assert_eq!(again, CancelReport::default());
    assert_eq!(partials(&notices(&pool, fx.env.owner).await), 1);

    // One delivery cancelled after the other went out.
    let dispatches = create_job(&pool, fx.env.owner, &[fx.account, second], JobSpec::live()).await;
    run(&fx.service, dispatches[0]).await;
    fx.service
        .cancel_delivery(&fx.env.scope, dispatches[1].delivery, ctx())
        .await
        .unwrap();
    assert_eq!(partials(&notices(&pool, fx.env.owner).await), 2);

    // A resolve to `failed` that ends the job.
    let dispatches = create_job(&pool, fx.env.owner, &[fx.account, second], JobSpec::live()).await;
    run(&fx.service, dispatches[0]).await;
    set_state(&pool, dispatches[1].delivery, "dead_letter").await;
    fx.service
        .resolve(
            &fx.env.scope,
            dispatches[1].delivery,
            ResolvedOutcome::Failed,
            ctx(),
        )
        .await
        .unwrap();
    assert_eq!(partials(&notices(&pool, fx.env.owner).await), 3);

    // A dry run records nothing.
    let before = notices(&pool, fx.env.owner).await.len();
    let dispatches = create_job(
        &pool,
        fx.env.owner,
        &[fx.account, second],
        JobSpec::dry_run(),
    )
    .await;
    run(&fx.service, dispatches[0]).await;
    fx.service
        .cancel_delivery(&fx.env.scope, dispatches[1].delivery, ctx())
        .await
        .unwrap();
    assert_eq!(notices(&pool, fx.env.owner).await.len(), before);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn jobs_survive_delegation_revocation(pool: PgPool) {
    let fx = fixture(&pool).await;
    let delegate = seed_user(&pool, "active").await;
    let delegation = grant(
        &pool,
        fx.env.owner,
        delegate,
        Level::Publish,
        AccountLimit::All,
    )
    .await;
    let scope = OwnerScope::delegated(
        delegate,
        fx.env.owner,
        delegation,
        Level::Publish,
        AccountLimit::All,
    );
    let post = new_post(&pool, fx.env.owner, "hello").await;
    let created = publish(&fx, &scope, post, live(&[fx.account], PostVersion(1)))
        .await
        .unwrap();
    sqlx::query("UPDATE delegations SET ended_at = now(), ended_reason = 'revoked' WHERE id = $1")
        .bind(delegation.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    run(&fx.service, created.deliveries[0]).await;

    assert_eq!(
        state(&pool, created.deliveries[0].delivery).await,
        DeliveryState::Published
    );
    assert_eq!(fx.script.posts().len(), 1);
    assert!(notices(&pool, delegate).await.is_empty());
}

/// What one caller may do, per service call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Ok,
    Scope,
    NotFound,
    NotInScope,
}

fn outcome<T: std::fmt::Debug>(result: &Result<T, PublishingError>) -> Expect {
    match result {
        Ok(_) => Expect::Ok,
        Err(PublishingError::Scope(_)) => Expect::Scope,
        Err(PublishingError::NotFound) => Expect::NotFound,
        Err(PublishingError::AccountNotInScope) => Expect::NotInScope,
        Err(other) => panic!("unexpected {other:?}"),
    }
}

/// Every audit event recorded so far.
async fn all_audits(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn isolation_matrix_for_every_service_call(pool: PgPool) {
    use Expect::{NotFound, NotInScope, Ok as Yes, Scope};
    let fx = fixture(&pool).await;
    let other = connect(&fx.accounts, &fx.env.scope, "other").await;
    // The limited delegate reaches only `other`.
    let who = callers(&pool, fx.env.owner, &[other]).await;
    let scopes = [
        ("owner", &who.owner),
        ("view", &who.view),
        ("edit", &who.edit),
        ("publish", &who.publish),
        ("limited", &who.limited),
        ("stranger", &who.stranger),
    ];
    let later = Utc::now() + chrono::Duration::hours(1);
    // Per caller: create live, create dry run, reschedule, cancel job, cancel delivery, retry,
    // resolve.
    let expected: [(&str, [Expect; 7]); 6] = [
        ("owner", [Yes, Yes, Yes, Yes, Yes, Yes, Yes]),
        ("view", [Scope, Scope, Scope, Scope, Scope, Scope, Scope]),
        ("edit", [Scope, Yes, Scope, Scope, Scope, Scope, Scope]),
        ("publish", [Yes, Yes, Yes, Yes, Yes, Yes, Yes]),
        (
            "limited",
            [
                NotInScope, NotInScope, NotFound, NotFound, NotFound, NotFound, NotFound,
            ],
        ),
        (
            "stranger",
            [
                NotFound, NotFound, NotFound, NotFound, NotFound, NotFound, NotFound,
            ],
        ),
    ];
    for ((name, scope), (expected_name, expected)) in scopes.iter().zip(expected) {
        assert_eq!(*name, expected_name);
        let post = new_post(&pool, fx.env.owner, name).await;
        let live_job = publish(
            &fx,
            scope,
            post,
            PublishInput {
                at: Some(later),
                ..live(&[fx.account], PostVersion(1))
            },
        )
        .await;
        let dry_post = new_post(&pool, fx.env.owner, name).await;
        let dry_job = publish(
            &fx,
            scope,
            dry_post,
            PublishInput {
                dry_run: true,
                at: Some(later),
                ..live(&[fx.account], PostVersion(1))
            },
        )
        .await;
        // The owner's own jobs to act on.
        let (_, job, dispatch) = one_live_job_on(&fx, "target").await;
        let pending_job = {
            let post = new_post(&pool, fx.env.owner, "pending").await;
            publish(
                &fx,
                &fx.env.scope,
                post,
                PublishInput {
                    at: Some(later),
                    ..live(&[fx.account], PostVersion(1))
                },
            )
            .await
            .unwrap()
        };
        if live_job.is_err() {
            assert_eq!(jobs_of(&pool, post).await, 0, "{name}: live job");
        }
        if dry_job.is_err() {
            assert_eq!(jobs_of(&pool, dry_post).await, 0, "{name}: dry-run job");
        }
        let version = job_version(&pool, pending_job.job).await;
        let pending_status = job_status(&pool, pending_job.job).await;
        let audits_before = all_audits(&pool).await;
        let reschedule = fx
            .service
            .reschedule(scope, pending_job.job, later, version, ctx())
            .await;
        if reschedule.is_err() {
            assert_eq!(job_version(&pool, pending_job.job).await, version, "{name}");
            assert_eq!(all_audits(&pool).await, audits_before, "{name}: reschedule");
        }
        let version = job_version(&pool, pending_job.job).await;
        let audits_before = all_audits(&pool).await;
        let cancel_job = fx.service.cancel_job(scope, pending_job.job, ctx()).await;
        if cancel_job.is_err() {
            assert_eq!(job_version(&pool, pending_job.job).await, version, "{name}");
            assert_eq!(job_status(&pool, pending_job.job).await, pending_status);
            assert_eq!(all_audits(&pool).await, audits_before, "{name}: cancel job");
        }
        let delivery_state = state(&pool, dispatch.delivery).await;
        let version = job_version(&pool, job).await;
        let audits_before = all_audits(&pool).await;
        let cancel_delivery = fx
            .service
            .cancel_delivery(scope, dispatch.delivery, ctx())
            .await;
        if cancel_delivery.is_err() {
            assert_eq!(state(&pool, dispatch.delivery).await, delivery_state);
            assert_eq!(job_version(&pool, job).await, version, "{name}");
            assert_eq!(all_audits(&pool).await, audits_before, "{name}: cancel");
        }
        // Settle the delivery so retry and resolve have something to act on.
        set_state(&pool, dispatch.delivery, "dead_letter").await;
        sqlx::query("UPDATE publish_jobs SET status = 'failed' WHERE id = $1")
            .bind(job.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let version = job_version(&pool, job).await;
        let audits_before = all_audits(&pool).await;
        let retry = fx
            .service
            .retry(scope, dispatch.delivery, true, false, ctx())
            .await;
        if retry.is_err() {
            assert_eq!(
                state(&pool, dispatch.delivery).await,
                DeliveryState::DeadLetter
            );
            assert_eq!(job_version(&pool, job).await, version, "{name}");
            assert_eq!(all_audits(&pool).await, audits_before, "{name}: retry");
        }
        set_state(&pool, dispatch.delivery, "dead_letter").await;
        let version = job_version(&pool, job).await;
        let audits_before = all_audits(&pool).await;
        let resolve = fx
            .service
            .resolve(scope, dispatch.delivery, ResolvedOutcome::Failed, ctx())
            .await;
        if resolve.is_err() {
            assert_eq!(
                state(&pool, dispatch.delivery).await,
                DeliveryState::DeadLetter
            );
            assert_eq!(job_version(&pool, job).await, version, "{name}");
            assert_eq!(all_audits(&pool).await, audits_before, "{name}: resolve");
        }
        let got = [
            outcome(&live_job),
            outcome(&dry_job),
            outcome(&reschedule),
            outcome(&cancel_job),
            outcome(&cancel_delivery),
            outcome(&retry),
            outcome(&resolve),
        ];
        assert_eq!(got, expected, "{name}");
        if *name == "owner" {
            assert_eq!(
                DeliveryRepo::get(
                    &mut pool.acquire().await.unwrap(),
                    &who.owner,
                    dispatch.delivery
                )
                .await
                .unwrap()
                .state,
                DeliveryState::Failed
            );
        }
        // Nothing the refused calls touched changed the owner's job.
        let mut conn = pool.acquire().await.unwrap();
        JobRepo::get(&mut conn, &who.owner, job).await.unwrap();
    }
}
