//! Direct deliveries: the audience materialized by `publish_delivery`, then capped
//! batches through `direct_send_batch`, against the `mock_direct` agent with contacts inserted
//! directly.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use common::{Env, env, notices, publishing_settings, queued, run, state};
use postit_agent_mock::{MockAgent, MockScript};
use postit_core::{
    AccountId, Agent, Clock, ContactListId, DeliveryId, DeliveryState, EmailBlock, EmailDocument,
    IdGenerator, Notice, PlatformId, PostId, PostKind, PostVersion, PublishJobId, PublishSnapshot,
    SystemClock, SystemIdGenerator, TestClock, UserId,
};
use postit_data::OwnerScope;
use postit_data::app_settings::AppSettingsRepo;
use postit_data::audit::AuditContext;
use postit_data::platforms::PlatformRepo;
use postit_data::posts::{NewPost, PostRepo};
use postit_data::publishing::{DeliveryRepo, Dispatch, JobRepo, NewDelivery, NewJob, Operation};
use postit_jobs::JobQueue;
use postit_notify::Notifier;
use postit_oauth::AccountService;
use postit_oauth::testkit::{TestOptions, account_service_with, test_accounts_settings};
use postit_publishing::direct::run_batch;
use postit_publishing::poll::run_poll;
use postit_publishing::sweep::run_sweep;
use postit_publishing::{DirectSendBatch, PollDeliveryStatus, PublishingDeps, PublishingService};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn mock_direct() -> PlatformId {
    PlatformId::new("mock_direct").unwrap()
}

struct Fx {
    env: Env,
    accounts: AccountService,
    service: PublishingService,
    script: MockScript,
    account: AccountId,
    list: ContactListId,
}

impl Fx {
    fn owner(&self) -> UserId {
        self.env.owner
    }

    /// The engine with `batch_size` recipients per batch and `clock` as its time.
    fn service_at(&self, batch_size: u32, clock: Arc<dyn Clock>) -> PublishingService {
        build_service(&self.env, self.accounts.clone(), batch_size, clock)
    }
}

fn build_service(
    env: &Env,
    accounts: AccountService,
    batch_size: u32,
    clock: Arc<dyn Clock>,
) -> PublishingService {
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    PublishingService::new(PublishingDeps {
        pool: env.pool.clone(),
        accounts,
        media: env.media.clone(),
        notifier: Notifier::for_tests(env.pool.clone()),
        queue: JobQueue::new(env.pool.clone(), Arc::clone(&ids), Arc::clone(&clock)),
        ids,
        clock,
        settings: publishing_settings(),
        shutdown: CancellationToken::new(),
        compiled_platforms: vec![mock_direct()],
        email_batch_size: batch_size,
    })
}

/// An owner with one connected `mock_direct` account (token `token`), one contact list, a
/// daily cap of `cap`, and time zone `zone`; the engine sends `batch_size` per batch.
async fn fixture(pool: &PgPool, batch_size: u32, cap: i32, zone: &str, token: &str) -> Fx {
    let env = env(pool).await;
    {
        let mut conn = pool.acquire().await.unwrap();
        AppSettingsRepo::seed_defaults(&mut conn, true)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes) \
         VALUES ($1, $2, 0, 0)",
    )
    .bind(env.owner.as_uuid())
    .bind(cap)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO user_preferences (user_id, timezone, locale) VALUES ($1, $2, 'en')")
        .bind(env.owner.as_uuid())
        .bind(zone)
        .execute(pool)
        .await
        .unwrap();
    let agent = MockAgent::direct();
    let script = agent.script();
    PlatformRepo::upsert_registered(pool, &[(mock_direct(), "Mock direct".to_owned())])
        .await
        .unwrap();
    let agents: Vec<Arc<dyn Agent>> = vec![Arc::new(agent)];
    let accounts = account_service_with(
        pool.clone(),
        agents,
        test_accounts_settings(),
        TestOptions {
            configs: BTreeMap::new(),
            ..TestOptions::default()
        },
    );
    let account = connect(&accounts, &env.scope, "main", token).await;
    let service = build_service(&env, accounts.clone(), batch_size, Arc::new(SystemClock));
    let list = list(pool, env.owner).await;
    Fx {
        env,
        accounts,
        service,
        script,
        account,
        list,
    }
}

async fn plain(pool: &PgPool) -> Fx {
    fixture(pool, 50, 0, "UTC", "s3cret-token-value").await
}

async fn connect(
    accounts: &AccountService,
    scope: &OwnerScope,
    name: &str,
    token: &str,
) -> AccountId {
    let Value::Object(values) = json!({"account_name": name, "token": token}) else {
        unreachable!()
    };
    let check = accounts
        .start_credentials(scope, &mock_direct(), values)
        .await
        .unwrap();
    let connect_id = check.pending.unwrap().0;
    let confirmed = accounts
        .confirm(scope, connect_id.as_str(), None, AuditContext::default())
        .await
        .unwrap();
    confirmed.connected[0].0.id
}

async fn list(pool: &PgPool, owner: UserId) -> ContactListId {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO contact_lists (id, owner_id, name) VALUES ($1, $2, 'list')")
        .bind(id)
        .bind(owner.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    ContactListId::from_uuid(id)
}

/// A stand-in address hash: the address bytes padded or cut to the 32 bytes a real hash has.
fn hash(email: &str) -> Vec<u8> {
    let mut bytes = email.as_bytes().to_vec();
    bytes.resize(32, 0);
    bytes
}

/// A contact with `status` on `fx`'s list.
async fn contact(fx: &Fx, email: &str, status: &str) -> Uuid {
    let pool = &fx.env.pool;
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO contacts (id, owner_id, email, email_hash, first_name, status) \
         VALUES ($1, $2, $3, $4, 'Ada', $5)",
    )
    .bind(id)
    .bind(fx.owner().as_uuid())
    .bind(email)
    .bind(hash(email))
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_list_members (list_id, contact_id, owner_id) VALUES ($1, $2, $3)",
    )
    .bind(fx.list.as_uuid())
    .bind(id)
    .bind(fx.owner().as_uuid())
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn suppress(fx: &Fx, email: &str) {
    sqlx::query(
        "INSERT INTO suppressions (id, owner_id, email_hash, reason) VALUES ($1, $2, $3, 'manual')",
    )
    .bind(Uuid::now_v7())
    .bind(fx.owner().as_uuid())
    .bind(hash(email))
    .execute(&fx.env.pool)
    .await
    .unwrap();
}

/// Creates a direct job to `accounts` with `fx`'s list as the audience, due now, and returns
/// the deliveries' dispatches in account order.
async fn direct_job(fx: &Fx, accounts: &[AccountId], dry_run: bool) -> Vec<Dispatch> {
    let pool = &fx.env.pool;
    let scope = &fx.env.scope;
    let post = PostId::from_uuid(Uuid::now_v7());
    let mut tx = pool.begin().await.unwrap();
    PostRepo::create(
        &mut tx,
        scope,
        NewPost {
            id: post,
            kind: PostKind::Post,
            reply_to: None,
            campaign_id: None,
            text: "hello".to_owned(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();
    let locked = PostRepo::lock(&mut tx, scope, post, None).await.unwrap();
    let version = locked.post().version;
    let variants = accounts
        .iter()
        .map(|account| {
            let mut v = common::variant(*account, &[]);
            v.target.platform = mock_direct();
            v.email = Some(EmailDocument {
                subject: "News".to_owned(),
                preheader: None,
                blocks: vec![EmailBlock::Text {
                    text: "hello".to_owned(),
                }],
            });
            v.options
                .insert("lists".to_owned(), json!([fx.list.as_uuid()]));
            v
        })
        .collect();
    let created = JobRepo::create(
        &mut tx,
        scope,
        &locked,
        NewJob {
            id: PublishJobId::from_uuid(Uuid::now_v7()),
            snapshot: PublishSnapshot {
                post_id: post,
                post_version: PostVersion(version),
                variants,
            },
            targets: accounts
                .iter()
                .map(|account| NewDelivery {
                    id: DeliveryId::from_uuid(Uuid::now_v7()),
                    account: *account,
                    operation: Operation::Direct,
                    due_at: None,
                })
                .collect(),
            scheduled_at: Utc::now() - chrono::Duration::seconds(1),
            native_schedule: false,
            dry_run,
            recurrence_id: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    created.deliveries
}

/// Starts the campaign: the `publish_delivery` run that materializes the audience.
async fn start(fx: &Fx) -> Dispatch {
    let dispatch = direct_job(fx, &[fx.account], false).await[0];
    run(&fx.service, dispatch).await;
    dispatch
}

fn batch_job(dispatch: Dispatch) -> DirectSendBatch {
    DirectSendBatch::from(dispatch)
}

/// Runs batches of `dispatch` with `service` until the delivery leaves `processing`, a batch
/// queues no successor, or the campaign waits for its daily cap (the successor runs at the
/// delivery's `next_attempt_at`). A successor delayed by a soft failure's backoff runs at once.
/// Returns how many batches ran.
async fn drain(service: &PublishingService, pool: &PgPool, dispatch: Dispatch) -> usize {
    let mut ran = 0;
    for _ in 0..20 {
        let before = batch_jobs(pool, dispatch.delivery).await;
        run_batch(service, batch_job(dispatch)).await.unwrap();
        ran += 1;
        let after = batch_jobs(pool, dispatch.delivery).await;
        let (resumes,): (Option<DateTime<Utc>>,) =
            sqlx::query_as("SELECT next_attempt_at FROM deliveries WHERE id = $1")
                .bind(dispatch.delivery.as_uuid())
                .fetch_one(pool)
                .await
                .unwrap();
        let next = after.get(before.len()).copied();
        let waits_for_cap = next.is_some() && next == resumes;
        if state(pool, dispatch.delivery).await != DeliveryState::Processing
            || next.is_none()
            || waits_for_cap
        {
            return ran;
        }
    }
    panic!("the campaign never finished");
}

/// The run times of the `direct_send_batch` jobs queued for `delivery`, oldest first.
async fn batch_jobs(pool: &PgPool, delivery: DeliveryId) -> Vec<DateTime<Utc>> {
    queued(pool, "direct_send_batch")
        .await
        .into_iter()
        .filter(|(payload, _)| payload["delivery"] == json!(delivery.as_uuid()))
        .map(|(_, at)| at)
        .collect()
}

/// Each row's contact address (via its contact), state, and tries.
async fn rows(pool: &PgPool, delivery: DeliveryId) -> Vec<(String, String, i16)> {
    sqlx::query_as(
        "SELECT c.email::text, s.state, s.tries FROM direct_sends s \
         JOIN contacts c ON c.id = s.contact_id WHERE s.delivery_id = $1 ORDER BY c.email",
    )
    .bind(delivery.as_uuid())
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn row_count(pool: &PgPool, delivery: DeliveryId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM direct_sends WHERE delivery_id = $1")
        .bind(delivery.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn counts(pool: &PgPool, delivery: DeliveryId) -> Value {
    sqlx::query_scalar("SELECT direct_counts FROM deliveries WHERE id = $1")
        .bind(delivery.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

fn count_kind(all: &[Notice], kind: &str) -> usize {
    all.iter().filter(|n| n.kind().as_str() == kind).count()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn audience_materialized_once_and_fixed(pool: PgPool) {
    let fx = plain(&pool).await;
    for n in 0..3 {
        contact(&fx, &format!("c{n}@example.com"), "subscribed").await;
    }
    let dispatch = start(&fx).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
    assert_eq!(counts(&pool, dispatch.delivery).await["audience"], json!(3));
    assert_eq!(row_count(&pool, dispatch.delivery).await, 3);
    assert_eq!(
        batch_jobs(&pool, dispatch.delivery).await.len(),
        1,
        "the first batch is queued with the materialization"
    );

    contact(&fx, "late@example.com", "subscribed").await;
    drain(&fx.service, &pool, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(row_count(&pool, dispatch.delivery).await, 3);
    assert_eq!(
        fx.script.sends().len(),
        3,
        "the late contact is not sent to"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn suppressed_and_unsubscribed_are_skipped(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "ok@example.com", "subscribed").await;
    contact(&fx, "gone@example.com", "unsubscribed").await;
    contact(&fx, "blocked@example.com", "subscribed").await;
    suppress(&fx, "blocked@example.com").await;
    let later = contact(&fx, "later@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    assert_eq!(counts(&pool, dispatch.delivery).await["audience"], json!(2));

    // Unsubscribed after the audience was fixed: recorded `suppressed`, never sent.
    sqlx::query("UPDATE contacts SET status = 'unsubscribed' WHERE id = $1")
        .bind(later)
        .execute(&pool)
        .await
        .unwrap();
    drain(&fx.service, &pool, dispatch).await;
    assert_eq!(
        rows(&pool, dispatch.delivery).await,
        vec![
            ("later@example.com".to_owned(), "suppressed".to_owned(), 0),
            ("ok@example.com".to_owned(), "sent".to_owned(), 0),
        ]
    );
    assert_eq!(fx.script.sends().len(), 1);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn batches_send_every_recipient_once(pool: PgPool) {
    let fx = fixture(&pool, 2, 0, "UTC", "s3cret-token-value").await;
    for n in 0..5 {
        contact(&fx, &format!("c{n}@example.com"), "subscribed").await;
    }
    let dispatch = start(&fx).await;
    let ran = drain(&fx.service, &pool, dispatch).await;
    assert_eq!(ran, 3, "five recipients in batches of two");
    let mut sent: Vec<_> = fx.script.sends().into_iter().map(|s| s.send_id).collect();
    assert_eq!(sent.len(), 5);
    sent.sort_by_key(postit_core::DirectSendId::as_uuid);
    sent.dedup();
    assert_eq!(sent.len(), 5, "no recipient is sent twice");
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(
        counts(&pool, dispatch.delivery).await,
        json!({"audience": 5, "queued": 0, "sent": 5, "failed": 0, "suppressed": 0, "bounced": 0})
    );
    let all = notices(&pool, fx.owner()).await;
    assert_eq!(count_kind(&all, "delivery_published"), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cap_boundary_waits_for_next_local_day(pool: PgPool) {
    let zone = "Asia/Kolkata";
    let fx = fixture(&pool, 50, 3, zone, "s3cret-token-value").await;
    for n in 0..5 {
        contact(&fx, &format!("c{n}@example.com"), "subscribed").await;
    }
    let dispatch = start(&fx).await;
    drain(&fx.service, &pool, dispatch).await;
    assert_eq!(fx.script.sends().len(), 3);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );

    let tz: Tz = zone.parse().unwrap();
    let today = Utc::now().with_timezone(&tz).date_naive();
    let midnight = tz
        .from_local_datetime(&today.succ_opt().unwrap().and_hms_opt(0, 0, 0).unwrap())
        .unwrap()
        .with_timezone(&Utc);
    let jobs = batch_jobs(&pool, dispatch.delivery).await;
    assert_eq!(
        jobs.last(),
        Some(&midnight),
        "the next batch runs at local midnight"
    );
    let (next_attempt,): (Option<DateTime<Utc>>,) =
        sqlx::query_as("SELECT next_attempt_at FROM deliveries WHERE id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        next_attempt,
        Some(midnight),
        "the delivery shows when it continues"
    );

    let tomorrow = fx.service_at(
        50,
        Arc::new(TestClock::new(midnight + chrono::Duration::seconds(1))),
    );
    drain(&tomorrow, &pool, dispatch).await;
    assert_eq!(fx.script.sends().len(), 5);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

async fn suppressions(pool: &PgPool, owner: UserId, email: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM suppressions WHERE owner_id = $1 AND email_hash = $2")
        .bind(owner.as_uuid())
        .bind(hash(email))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bounce_5xx_suppresses(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "x-bounce@example.com", "subscribed").await;
    contact(&fx, "ok@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    drain(&fx.service, &pool, dispatch).await;
    assert_eq!(
        rows(&pool, dispatch.delivery).await,
        vec![
            ("ok@example.com".to_owned(), "sent".to_owned(), 0),
            ("x-bounce@example.com".to_owned(), "bounced".to_owned(), 0),
        ]
    );
    assert_eq!(
        suppressions(&pool, fx.owner(), "x-bounce@example.com").await,
        1
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bounce_is_suppressed_once(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "x-bounce@example.com", "subscribed").await;
    let first = start(&fx).await;
    drain(&fx.service, &pool, first).await;
    assert_eq!(
        suppressions(&pool, fx.owner(), "x-bounce@example.com").await,
        1,
        "the record adds the suppression; the worker adds none of its own"
    );
    let second = start(&fx).await;
    assert_eq!(
        counts(&pool, second.delivery).await["audience"],
        json!(0),
        "the next campaign skips the bounced address"
    );
    drain(&fx.service, &pool, second).await;
    assert_eq!(
        suppressions(&pool, fx.owner(), "x-bounce@example.com").await,
        1
    );
    assert_eq!(
        state(&pool, second.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn soft_failure_retries_within_budget(pool: PgPool) {
    let fx = plain(&pool).await;
    let recovers = contact(&fx, "a-fail@example.com", "subscribed").await;
    contact(&fx, "b-fail@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    let job = batch_job(dispatch);
    run_batch(&fx.service, job.clone()).await.unwrap();
    run_batch(&fx.service, job.clone()).await.unwrap();
    assert_eq!(
        rows(&pool, dispatch.delivery).await,
        vec![
            ("a-fail@example.com".to_owned(), "queued".to_owned(), 2),
            ("b-fail@example.com".to_owned(), "queued".to_owned(), 2),
        ],
        "each soft failure is queued again and counted"
    );
    // The first address starts working.
    sqlx::query("UPDATE contacts SET email = 'a-ok@example.com' WHERE id = $1")
        .bind(recovers)
        .execute(&pool)
        .await
        .unwrap();
    run_batch(&fx.service, job).await.unwrap();
    assert_eq!(
        rows(&pool, dispatch.delivery).await,
        vec![
            ("a-ok@example.com".to_owned(), "sent".to_owned(), 2),
            ("b-fail@example.com".to_owned(), "failed".to_owned(), 2),
        ],
        "a third soft failure ends the row"
    );
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn crash_mid_batch_resumes_without_resending_terminal_rows(pool: PgPool) {
    use postit_data::publishing::Fence;
    use postit_data::publishing::direct::{BatchClaim, DirectRepo, DirectResult};

    let fx = plain(&pool).await;
    for n in 0..4 {
        contact(&fx, &format!("c{n}@example.com"), "subscribed").await;
    }
    let dispatch = start(&fx).await;
    // A worker leased all four, recorded two, and crashed.
    let mut conn = pool.acquire().await.unwrap();
    let claim = DirectRepo::claim_batch(
        &mut conn,
        Fence {
            id: dispatch.delivery,
            dispatch_gen: dispatch.dispatch_gen,
            expected: DeliveryState::Processing,
        },
        4,
        Utc::now(),
        chrono::Duration::minutes(5),
        None,
    )
    .await
    .unwrap();
    let BatchClaim::Rows {
        batch,
        rows: leased,
    } = claim
    else {
        panic!("expected rows");
    };
    for row in &leased[..2] {
        DirectRepo::record(
            &mut conn,
            row.id,
            batch,
            DirectResult::Sent {
                provider_message_id: Some("earlier".to_owned()),
            },
            Utc::now(),
        )
        .await
        .unwrap();
    }
    sqlx::query(
        "UPDATE direct_sends SET lease_expires_at = now() - interval '1 minute' \
         WHERE delivery_id = $1 AND state = 'queued'",
    )
    .bind(dispatch.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE deliveries SET updated_at = now() - interval '10 minutes', \n         next_attempt_at = now() - interval '10 minutes' WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let report = run_sweep(&fx.service).await.unwrap();
    assert_eq!(report.orphaned_direct, 1);
    drain(&fx.service, &pool, dispatch).await;
    let sent: Vec<_> = fx.script.sends().into_iter().map(|s| s.send_id).collect();
    assert_eq!(
        sent,
        vec![leased[2].id, leased[3].id],
        "only the rows the crash left queued are sent"
    );
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(counts(&pool, dispatch.delivery).await["sent"], json!(4));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn direct_dry_run_reports_count_and_writes_nothing(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    contact(&fx, "c@example.com", "unsubscribed").await;
    let dispatch = direct_job(&fx, &[fx.account], true).await[0];
    run(&fx.service, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Simulated
    );
    assert_eq!(row_count(&pool, dispatch.delivery).await, 0);
    let summary: Value =
        sqlx::query_scalar("SELECT request_summary FROM delivery_attempts WHERE delivery_id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(summary["audience"], json!(2));
    assert_eq!(summary["mode"], json!("dry_run"));
    assert!(batch_jobs(&pool, dispatch.delivery).await.is_empty());
    assert!(fx.script.sends().is_empty());
    assert!(notices(&pool, fx.owner()).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn campaign_with_some_failures_is_published_with_counts(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    contact(&fx, "x-bounce@example.com", "subscribed").await;
    contact(&fx, "y-soft-bounce@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    drain(&fx.service, &pool, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(
        counts(&pool, dispatch.delivery).await,
        json!({"audience": 4, "queued": 0, "sent": 2, "failed": 1, "suppressed": 0, "bounced": 1})
    );
    let all = notices(&pool, fx.owner()).await;
    assert_eq!(count_kind(&all, "delivery_published"), 1);
    assert_eq!(
        count_kind(&all, "job_partial"),
        0,
        "the job's one delivery went out"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn finished_campaign_records_job_partial_once(pool: PgPool) {
    let fx = plain(&pool).await;
    let other = connect(&fx.accounts, &fx.env.scope, "second", "s3cret-token-value").await;
    contact(&fx, "a@example.com", "subscribed").await;
    let dispatches = direct_job(&fx, &[fx.account, other], false).await;
    {
        let mut conn = pool.acquire().await.unwrap();
        DeliveryRepo::cancel(&mut conn, &fx.env.scope, dispatches[1].delivery, Utc::now())
            .await
            .unwrap();
    }
    run(&fx.service, dispatches[0]).await;
    drain(&fx.service, &pool, dispatches[0]).await;
    assert_eq!(
        state(&pool, dispatches[0].delivery).await,
        DeliveryState::Published
    );
    let all = notices(&pool, fx.owner()).await;
    assert_eq!(count_kind(&all, "job_partial"), 1);
    assert_eq!(count_kind(&all, "delivery_published"), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn processing_timeout_exempts_direct_with_queued_rows(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    // Two days in `processing` with its row still queued.
    sqlx::query(
        "UPDATE deliveries SET state_changed_at = now() - interval '2 days', \
         check_due_at = now() - interval '2 days' WHERE id = $1",
    )
    .bind(dispatch.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE delivery_attempts SET started_at = now() - interval '2 days' \
         WHERE delivery_id = $1",
    )
    .bind(dispatch.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    run_poll(
        &fx.service,
        PollDeliveryStatus {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen,
            poll: 0,
        },
    )
    .await
    .unwrap();
    let report = run_sweep(&fx.service).await.unwrap();
    assert_eq!(report.restarted, 0);
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
    assert!(queued(&pool, "reconcile_delivery").await.is_empty());
    drain(&fx.service, &pool, dispatch).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn soft_failure_after_cancel_is_recorded_not_requeued(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a-fail@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    let gate = fx.script.hold_publishes();
    let service = fx.service.clone();
    let sending = tokio::spawn(async move { run_batch(&service, batch_job(dispatch)).await });
    assert!(gate.wait_for_held(1, Duration::from_secs(10)).await);
    {
        let mut conn = pool.acquire().await.unwrap();
        DeliveryRepo::cancel(&mut conn, &fx.env.scope, dispatch.delivery, Utc::now())
            .await
            .unwrap();
    }
    gate.release();
    sending.await.unwrap().unwrap();
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Cancelled
    );
    let (state_, code, tries): (String, Option<String>, i16) =
        sqlx::query_as("SELECT state, error_code, tries FROM direct_sends WHERE delivery_id = $1")
            .bind(dispatch.delivery.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state_, "failed");
    assert_ne!(
        code.as_deref(),
        Some("cancelled"),
        "the real result replaces the cancel"
    );
    assert_eq!(tries, 0, "never requeued");
    assert_eq!(batch_jobs(&pool, dispatch.delivery).await.len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rejected_credentials_stop_the_campaign_without_using_the_cap(pool: PgPool) {
    let fx = fixture(&pool, 50, 10, "UTC", "expired").await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );
    run_batch(&fx.service, batch_job(dispatch)).await.unwrap();

    let leftovers: Vec<(String, i16, bool)> = sqlx::query_as(
        "SELECT state, tries, batch IS NULL AND lease_expires_at IS NULL FROM direct_sends \
         WHERE delivery_id = $1",
    )
    .bind(dispatch.delivery.as_uuid())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        leftovers,
        vec![
            ("queued".to_owned(), 0, true),
            ("queued".to_owned(), 0, true)
        ]
    );
    assert_eq!(
        common::account_units(&pool, fx.account).await,
        0,
        "the cap units are back"
    );
    let (account_status, grant_status) = common::account_status(&pool, fx.account).await;
    assert_eq!(
        (account_status.as_str(), grant_status.as_str()),
        ("needs_reconnect", "needs_reconnect")
    );
    assert_eq!(
        batch_jobs(&pool, dispatch.delivery).await.len(),
        1,
        "no next batch"
    );
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Processing
    );

    let quiet = || async {
        sqlx::query(
            "UPDATE deliveries SET updated_at = now() - interval '10 minutes', \n         next_attempt_at = now() - interval '10 minutes' WHERE id = $1",
        )
        .bind(dispatch.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    };
    quiet().await;
    assert_eq!(run_sweep(&fx.service).await.unwrap().orphaned_direct, 0);
    assert_eq!(batch_jobs(&pool, dispatch.delivery).await.len(), 1);

    // Reconnected.
    sqlx::query("UPDATE account_grants SET status = 'active'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE connected_accounts SET status = 'active' WHERE id = $1")
        .bind(fx.account.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    quiet().await;
    assert_eq!(run_sweep(&fx.service).await.unwrap().orphaned_direct, 1);
    assert_eq!(batch_jobs(&pool, dispatch.delivery).await.len(), 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn permanent_batch_error_is_bounded_by_tries(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    // The message loses its subject after the campaign started: the provider refuses every
    // batch with a validation error.
    sqlx::query(
        "UPDATE publish_jobs SET content_snapshot = \
         jsonb_set(content_snapshot, '{variants,0,email,subject}', '\"\"') \
         WHERE id = (SELECT job_id FROM deliveries WHERE id = $1)",
    )
    .bind(dispatch.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let job = batch_job(dispatch);
    for _ in 0..3 {
        assert_eq!(
            state(&pool, dispatch.delivery).await,
            DeliveryState::Processing
        );
        run_batch(&fx.service, job.clone()).await.unwrap();
    }
    assert_eq!(
        rows(&pool, dispatch.delivery).await,
        vec![
            ("a@example.com".to_owned(), "failed".to_owned(), 2),
            ("b@example.com".to_owned(), "failed".to_owned(), 2),
        ],
        "three refused calls end every row"
    );
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(
        counts(&pool, dispatch.delivery).await,
        json!({"audience": 2, "queued": 0, "sent": 0, "failed": 2, "suppressed": 0, "bounced": 0})
    );
    let jobs = batch_jobs(&pool, dispatch.delivery).await.len();
    run_batch(&fx.service, job).await.unwrap();
    assert_eq!(
        batch_jobs(&pool, dispatch.delivery).await.len(),
        jobs,
        "a fourth run does nothing"
    );
    assert!(fx.script.sends().is_empty());
}

/// A provider that answers "busy" to every batch for good: each refused call counts a try per
/// row and waits the provider's Retry-After, so the campaign ends with counts instead of being
/// called every lease plus grace forever.
#[sqlx::test(migrations = "../data/migrations")]
async fn transient_batch_error_is_bounded_by_tries(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    fx.script.set_retry_after(Some(Duration::from_secs(90)));
    let job = batch_job(dispatch);
    let before = Utc::now();
    run_batch(&fx.service, job.clone()).await.unwrap();
    assert_eq!(
        rows(&pool, dispatch.delivery).await,
        vec![
            ("a@example.com".to_owned(), "queued".to_owned(), 1),
            ("b@example.com".to_owned(), "queued".to_owned(), 1),
        ],
        "a refused batch sends its rows back with a try counted, not leased"
    );
    let jobs = batch_jobs(&pool, dispatch.delivery).await;
    assert_eq!(jobs.len(), 2);
    assert!(
        jobs[1] >= before + chrono::Duration::seconds(89),
        "the next batch waits the provider's Retry-After: {}",
        jobs[1]
    );
    for _ in 0..2 {
        assert_eq!(
            state(&pool, dispatch.delivery).await,
            DeliveryState::Processing
        );
        run_batch(&fx.service, job.clone()).await.unwrap();
    }
    assert_eq!(
        rows(&pool, dispatch.delivery).await,
        vec![
            ("a@example.com".to_owned(), "failed".to_owned(), 2),
            ("b@example.com".to_owned(), "failed".to_owned(), 2),
        ],
        "three refused calls end every row"
    );
    assert_eq!(
        state(&pool, dispatch.delivery).await,
        DeliveryState::Published
    );
    assert_eq!(
        counts(&pool, dispatch.delivery).await,
        json!({"audience": 2, "queued": 0, "sent": 0, "failed": 2, "suppressed": 0, "bounced": 0})
    );
    assert!(fx.script.sends().is_empty());
}

/// A refused batch that also held a row recorded without a send (the contact unsubscribed
/// after the audience was fixed): that row is final, but the provider made no progress, so the
/// next batch still waits the provider's Retry-After instead of calling it again at once.
#[sqlx::test(migrations = "../data/migrations")]
async fn refused_batch_with_a_suppressed_row_keeps_the_retry_after(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    let later = contact(&fx, "later@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    sqlx::query("UPDATE contacts SET status = 'unsubscribed' WHERE id = $1")
        .bind(later)
        .execute(&pool)
        .await
        .unwrap();
    fx.script.set_retry_after(Some(Duration::from_secs(90)));
    let before = Utc::now();
    run_batch(&fx.service, batch_job(dispatch)).await.unwrap();
    assert_eq!(
        rows(&pool, dispatch.delivery).await,
        vec![
            ("a@example.com".to_owned(), "queued".to_owned(), 1),
            ("b@example.com".to_owned(), "queued".to_owned(), 1),
            ("later@example.com".to_owned(), "suppressed".to_owned(), 0),
        ]
    );
    let jobs = batch_jobs(&pool, dispatch.delivery).await;
    assert_eq!(jobs.len(), 2);
    assert!(
        jobs[1] >= before + chrono::Duration::seconds(89),
        "the next batch waits the provider's Retry-After: {}",
        jobs[1]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn direct_failure_before_accepted_redispatches_and_sends_once(pool: PgPool) {
    use postit_publishing::reconcile::run_reconcile;
    use postit_publishing::{PublishDelivery, ReconcileDelivery, run_publish};

    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    let first = direct_job(&fx, &[fx.account], false).await[0];
    // The materialization fails after `StartRemote`: the delivery is left `in_flight`.
    sqlx::query(
        "CREATE FUNCTION test_refuse_send() RETURNS trigger AS $body$ \
         BEGIN RAISE EXCEPTION 'injected failure'; END $body$ LANGUAGE plpgsql",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER test_refuse_send BEFORE INSERT ON direct_sends \
         FOR EACH ROW EXECUTE FUNCTION test_refuse_send()",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        run_publish(&fx.service, PublishDelivery::from(first))
            .await
            .is_err()
    );
    assert_eq!(state(&pool, first.delivery).await, DeliveryState::InFlight);
    sqlx::query("DROP TRIGGER test_refuse_send ON direct_sends")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(row_count(&pool, first.delivery).await, 0);

    sqlx::query(
        "UPDATE deliveries SET lease_expires_at = now() - interval '10 minutes' WHERE id = $1",
    )
    .bind(first.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(run_sweep(&fx.service).await.unwrap().unknown, 1);
    assert_eq!(
        state(&pool, first.delivery).await,
        DeliveryState::OutcomeUnknown
    );
    let calls = fx.script.remote_calls();
    run_reconcile(
        &fx.service,
        ReconcileDelivery {
            delivery: first.delivery.as_uuid(),
            dispatch_gen: first.dispatch_gen,
            attempt: 0,
        },
    )
    .await
    .unwrap();
    assert_eq!(fx.script.remote_calls(), calls, "no agent is asked");
    assert_eq!(state(&pool, first.delivery).await, DeliveryState::Pending);
    assert_eq!(
        count_kind(&notices(&pool, fx.owner()).await, "delivery_parked"),
        0
    );

    let redispatched = queued(&pool, "publish_delivery")
        .await
        .into_iter()
        .filter(|(payload, _)| payload["delivery"] == json!(first.delivery.as_uuid()))
        .filter_map(|(payload, _)| payload["dispatch_gen"].as_i64())
        .max()
        .unwrap();
    let again = Dispatch {
        delivery: first.delivery,
        dispatch_gen: i32::try_from(redispatched).unwrap(),
        run_at: Utc::now(),
    };
    assert!(again.dispatch_gen > first.dispatch_gen);
    // The re-run waits the retry backoff; skip the wait.
    sqlx::query("UPDATE deliveries SET next_attempt_at = now() WHERE id = $1")
        .bind(first.delivery.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    run(&fx.service, again).await;
    drain(&fx.service, &pool, again).await;
    assert_eq!(state(&pool, first.delivery).await, DeliveryState::Published);
    assert_eq!(
        row_count(&pool, first.delivery).await,
        2,
        "materialized once"
    );
    let mut sent: Vec<_> = fx.script.sends().into_iter().map(|s| s.send_id).collect();
    assert_eq!(sent.len(), 2);
    sent.sort_by_key(postit_core::DirectSendId::as_uuid);
    sent.dedup();
    assert_eq!(sent.len(), 2, "each email is sent once");
}

/// The delivery's state, `last_error` code, stored counts, queued batch jobs, and
/// `delivery_failed` notices, for a campaign a batch had to fail.
async fn failed_campaign(
    fx: &Fx,
    dispatch: Dispatch,
) -> (DeliveryState, Value, Value, usize, usize) {
    let pool = &fx.env.pool;
    let last_error: Value = sqlx::query_scalar("SELECT last_error FROM deliveries WHERE id = $1")
        .bind(dispatch.delivery.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    (
        state(pool, dispatch.delivery).await,
        last_error["code"].clone(),
        counts(pool, dispatch.delivery).await,
        batch_jobs(pool, dispatch.delivery).await.len(),
        count_kind(&notices(pool, fx.owner()).await, "delivery_failed"),
    )
}

fn two_stopped() -> Value {
    json!({"audience": 2, "queued": 0, "sent": 0, "failed": 2, "suppressed": 0, "bounced": 0})
}

#[sqlx::test(migrations = "../data/migrations")]
async fn batch_whose_platform_is_gone_fails_the_campaign(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    // The deployment no longer has the account's agent.
    let accounts = account_service_with(
        pool.clone(),
        Vec::new(),
        test_accounts_settings(),
        TestOptions::default(),
    );
    let service = build_service(&fx.env, accounts, 50, Arc::new(SystemClock));
    run_batch(&service, batch_job(dispatch)).await.unwrap();
    let (state_, code, stored, jobs, failed) = failed_campaign(&fx, dispatch).await;
    assert_eq!(state_, DeliveryState::Failed);
    assert_eq!(code, json!("platform_unavailable"));
    assert_eq!(
        stored,
        two_stopped(),
        "the queued rows are stopped and counted"
    );
    assert_eq!(jobs, 1, "no next batch");
    assert_eq!(failed, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn batch_that_cannot_build_its_message_fails_the_campaign(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a@example.com", "subscribed").await;
    contact(&fx, "b@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    // The snapshot no longer has a variant for the account.
    sqlx::query(
        "UPDATE publish_jobs SET content_snapshot = jsonb_set(content_snapshot,          '{variants,0,target,account}', to_jsonb(gen_random_uuid()::text))          WHERE id = (SELECT job_id FROM deliveries WHERE id = $1)",
    )
    .bind(dispatch.delivery.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    run_batch(&fx.service, batch_job(dispatch)).await.unwrap();
    let (state_, code, stored, jobs, failed) = failed_campaign(&fx, dispatch).await;
    assert_eq!(state_, DeliveryState::Failed);
    assert_eq!(code, json!("revalidation_failed"));
    assert_eq!(stored, two_stopped());
    assert_eq!(jobs, 1, "no next batch");
    assert_eq!(failed, 1);
    assert!(fx.script.sends().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn soft_failure_waits_before_the_next_batch(pool: PgPool) {
    use postit_publishing::backoff::retry_ceiling;

    let fx = plain(&pool).await;
    contact(&fx, "a-fail@example.com", "subscribed").await;
    let ceiling = chrono::Duration::from_std(retry_ceiling(1)).unwrap();
    let mut delays = Vec::new();
    // The backoff is jittered: over five campaigns at least one waits more than a second.
    for _ in 0..5 {
        let dispatch = start(&fx).await;
        let before = Utc::now();
        run_batch(&fx.service, batch_job(dispatch)).await.unwrap();
        let after = Utc::now();
        let jobs = batch_jobs(&pool, dispatch.delivery).await;
        assert_eq!(jobs.len(), 2);
        assert!(
            jobs[1] >= before && jobs[1] <= after + ceiling,
            "a run that only sent rows back waits within the backoff: {}",
            jobs[1]
        );
        delays.push(jobs[1] - after);
    }
    assert!(
        delays.iter().any(|d| *d > chrono::Duration::seconds(1)),
        "the next batch is not queued at once: {delays:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_run_with_progress_continues_at_once(pool: PgPool) {
    let fx = plain(&pool).await;
    contact(&fx, "a-fail@example.com", "subscribed").await;
    contact(&fx, "ok@example.com", "subscribed").await;
    let dispatch = start(&fx).await;
    run_batch(&fx.service, batch_job(dispatch)).await.unwrap();
    let jobs = batch_jobs(&pool, dispatch.delivery).await;
    assert_eq!(jobs.len(), 2);
    assert!(jobs[1] <= Utc::now(), "progress continues at once");
}
