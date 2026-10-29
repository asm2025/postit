#![allow(dead_code, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use postit_agent_mock::{MockAgent, MockScript};
use postit_config::PublishingSettings;
use postit_core::{
    AccountId, AccountRef, Agent, ByteStream, Clock, DeliveryId, DeliveryState, IdGenerator,
    MediaAssetId, MediaItem, Notice, PlatformId, PostId, PostKind, PostVersion, PublishJobId,
    PublishSnapshot, QuotaBucket, ReplyTo, SystemClock, SystemIdGenerator, UserId, Variant,
};
use postit_data::OwnerScope;
use postit_data::app_settings::AppSettingsRepo;
use postit_data::audit::AuditContext;
use postit_data::platforms::PlatformRepo;
use postit_data::posts::{NewPost, PostRepo};
use postit_data::publishing::{DeliveryRepo, Dispatch, JobRepo, NewDelivery, NewJob, Operation};
use postit_data::testkit::seed_user;
use postit_jobs::JobQueue;
use postit_media::MediaService;
use postit_media::testkit::{media_service, media_settings};
use postit_notify::{Notifier, notifications_for};
use postit_oauth::AccountService;
use postit_oauth::testkit::{
    TestOptions, account_service, account_service_with, test_accounts_settings,
};
use postit_publishing::worker::run_publish;
use postit_publishing::{PublishDelivery, PublishingDeps, PublishingService};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Env {
    pub dir: tempfile::TempDir,
    pub pool: PgPool,
    pub owner: UserId,
    pub scope: OwnerScope,
    pub media: MediaService,
}

pub async fn env(pool: &PgPool) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let owner = seed_user(pool, "active").await;
    let media = media_service(
        pool.clone(),
        media_settings(dir.path()),
        Arc::new(SystemClock),
    )
    .unwrap();
    Env {
        dir,
        pool: pool.clone(),
        owner,
        scope: OwnerScope::own(owner),
        media,
    }
}

pub fn publishing_settings() -> PublishingSettings {
    PublishingSettings {
        live: true,
        max_attempts: 5,
        lease: std::time::Duration::from_mins(5),
        sweep_grace: std::time::Duration::from_mins(2),
        park_grace: std::time::Duration::from_mins(15),
        processing_timeout: std::time::Duration::from_hours(24),
    }
}

pub fn service(env: &Env) -> PublishingService {
    service_with(
        env,
        account_service(env.pool.clone(), Vec::new(), test_accounts_settings()),
        vec![mock()],
    )
}

/// A service over `accounts` that believes `compiled` are the platforms in this build.
pub fn service_with(
    env: &Env,
    accounts: AccountService,
    compiled: Vec<PlatformId>,
) -> PublishingService {
    service_full(
        env,
        accounts,
        compiled,
        publishing_settings(),
        CancellationToken::new(),
    )
}

/// [`service_with`] with its own settings and shutdown token.
pub fn service_full(
    env: &Env,
    accounts: AccountService,
    compiled: Vec<PlatformId>,
    settings: PublishingSettings,
    shutdown: CancellationToken,
) -> PublishingService {
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    PublishingService::new(PublishingDeps {
        pool: env.pool.clone(),
        accounts,
        media: env.media.clone(),
        notifier: Notifier::for_tests(env.pool.clone()),
        queue: JobQueue::new(env.pool.clone(), Arc::clone(&ids), Arc::clone(&clock)),
        ids,
        clock,
        settings,
        shutdown,
        compiled_platforms: compiled,
    })
}

pub fn mock() -> PlatformId {
    PlatformId::new("mock").unwrap()
}

/// What a [`fixture_with`] changes from the defaults.
#[derive(Default)]
pub struct FixtureOptions {
    /// The mock's quota buckets instead of its own.
    pub buckets: Option<Vec<QuotaBucket>>,
    /// The `[platforms.mock]` table.
    pub config: Option<Value>,
    /// The platforms the service believes are compiled in; `mock` by default.
    pub compiled: Option<Vec<PlatformId>>,
    /// More agents next to `mock`, with their `[platforms.<id>]` tables (`None` leaves one
    /// unconfigured).
    pub extra: Vec<(Arc<dyn Agent>, Option<Value>)>,
    /// `[publishing]` instead of [`publishing_settings`].
    pub settings: Option<PublishingSettings>,
    /// Capabilities the mock agent drops.
    pub without: Vec<postit_core::Capability>,
}

/// An owner with one connected `mock` account, and the engine over the mock agent.
pub struct Fixture {
    pub env: Env,
    pub accounts: AccountService,
    pub service: PublishingService,
    pub script: MockScript,
    pub account: AccountId,
}

pub async fn fixture(pool: &PgPool) -> Fixture {
    fixture_with(pool, FixtureOptions::default()).await
}

pub async fn fixture_with(pool: &PgPool, options: FixtureOptions) -> Fixture {
    let env = env(pool).await;
    {
        let mut conn = pool.acquire().await.unwrap();
        AppSettingsRepo::seed_defaults(&mut conn, true)
            .await
            .unwrap();
    }
    let mut agent = MockAgent::broadcast();
    if let Some(buckets) = options.buckets {
        agent = agent.with_quota_buckets(buckets);
    }
    for capability in options.without {
        agent = agent.without_capability(capability);
    }
    let script = agent.script();
    let mut configs = BTreeMap::new();
    if let Some(config) = options.config {
        configs.insert(mock(), Some(config));
    }
    let mut agents: Vec<Arc<dyn Agent>> = vec![Arc::new(agent)];
    let mut registered = vec![(mock(), "Mock".to_owned())];
    for (extra, config) in options.extra {
        let id = extra.descriptor().platform.clone();
        registered.push((id.clone(), id.as_str().to_owned()));
        configs.insert(id, config);
        agents.push(extra);
    }
    PlatformRepo::upsert_registered(pool, &registered)
        .await
        .unwrap();
    let accounts = account_service_with(
        pool.clone(),
        agents,
        test_accounts_settings(),
        TestOptions {
            configs,
            ..TestOptions::default()
        },
    );
    let account = connect(&accounts, &env.scope, "main").await;
    let service = service_full(
        &env,
        accounts.clone(),
        options.compiled.unwrap_or_else(|| vec![mock()]),
        options.settings.unwrap_or_else(publishing_settings),
        CancellationToken::new(),
    );
    Fixture {
        env,
        accounts,
        service,
        script,
        account,
    }
}

/// Connects the `mock` account named `name` for `scope`'s owner.
pub async fn connect(accounts: &AccountService, scope: &OwnerScope, name: &str) -> AccountId {
    let Value::Object(values) = json!({"account_name": name, "token": "s3cret-token-value"}) else {
        unreachable!()
    };
    let check = accounts
        .start_credentials(scope, &mock(), values)
        .await
        .unwrap();
    let connect_id = check.pending.unwrap().0;
    let confirmed = accounts
        .confirm(scope, connect_id.as_str(), None, AuditContext::default())
        .await
        .unwrap();
    confirmed.connected[0].0.id
}

/// The job a test publishes.
pub struct JobSpec {
    pub dry_run: bool,
    pub text: String,
    pub media: Vec<MediaAssetId>,
    pub scheduled_at: DateTime<Utc>,
    pub platform: PlatformId,
    /// The mock's `outcome` option on every variant.
    pub outcome: Option<&'static str>,
    /// Per-account `outcome` options, in account order; overrides `outcome` when set.
    pub outcomes: Vec<&'static str>,
    /// Makes the job a comment answering this remote post.
    pub comment_on: Option<String>,
    /// The mock's `reconcile` option on every variant.
    pub reconcile: Option<&'static str>,
}

impl JobSpec {
    /// A live job due now.
    pub fn live() -> Self {
        Self {
            dry_run: false,
            text: "hello".to_owned(),
            media: Vec::new(),
            scheduled_at: Utc::now() - chrono::Duration::seconds(1),
            platform: mock(),
            outcome: None,
            outcomes: Vec::new(),
            comment_on: None,
            reconcile: None,
        }
    }

    /// A live job due now whose mock `outcome` option is `outcome`.
    pub fn outcome(outcome: &'static str) -> Self {
        Self {
            outcome: Some(outcome),
            ..Self::live()
        }
    }

    /// A dry-run job due now.
    pub fn dry_run() -> Self {
        Self {
            dry_run: true,
            ..Self::live()
        }
    }
}

/// Creates a post and a job for it to `accounts`, as `owner`, and returns the deliveries'
/// dispatches in account order.
pub async fn create_job(
    pool: &PgPool,
    owner: UserId,
    accounts: &[AccountId],
    spec: JobSpec,
) -> Vec<Dispatch> {
    let scope = OwnerScope::own(owner);
    let post = PostId::from_uuid(Uuid::now_v7());
    let reply_to = spec.comment_on.as_ref().map(|remote| ReplyTo {
        remote_post_id: remote.clone(),
        remote_comment_id: None,
        url: None,
    });
    let kind = if reply_to.is_some() {
        PostKind::Comment
    } else {
        PostKind::Post
    };
    let mut tx = pool.begin().await.unwrap();
    PostRepo::create(
        &mut tx,
        &scope,
        NewPost {
            id: post,
            kind,
            reply_to: reply_to.as_ref().map(|r| serde_json::to_value(r).unwrap()),
            campaign_id: None,
            text: spec.text.clone(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();
    let locked = PostRepo::lock(&mut tx, &scope, post, None).await.unwrap();
    let version = locked.post().version;
    let variants = accounts
        .iter()
        .enumerate()
        .map(|(i, account)| {
            let mut v = variant(*account, &spec.media);
            v.target.platform = spec.platform.clone();
            v.text.clone_from(&spec.text);
            v.kind = kind;
            v.reply_to.clone_from(&reply_to);
            if let Some(outcome) = spec.outcomes.get(i).copied().or(spec.outcome) {
                v.options.insert("outcome".to_owned(), json!(outcome));
            }
            if let Some(reconcile) = spec.reconcile {
                v.options.insert("reconcile".to_owned(), json!(reconcile));
            }
            v
        })
        .collect();
    let created = JobRepo::create(
        &mut tx,
        &scope,
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
                    operation: if reply_to.is_some() {
                        Operation::Comment
                    } else {
                        Operation::Publish
                    },
                })
                .collect(),
            scheduled_at: spec.scheduled_at,
            native_schedule: false,
            dry_run: spec.dry_run,
            recurrence_id: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    created.deliveries
}

/// Runs the publish job for `dispatch`.
pub async fn run(service: &PublishingService, dispatch: Dispatch) {
    run_publish(service, PublishDelivery::from(dispatch))
        .await
        .unwrap();
}

/// The delivery's state.
pub async fn state(pool: &PgPool, id: DeliveryId) -> DeliveryState {
    let owner: Uuid = sqlx::query_scalar("SELECT owner_id FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    DeliveryRepo::get(&mut conn, &OwnerScope::own(UserId::from_uuid(owner)), id)
        .await
        .unwrap()
        .state
}

/// The delivery's `last_error`.
pub async fn last_error(pool: &PgPool, id: DeliveryId) -> Option<Value> {
    sqlx::query_scalar("SELECT last_error FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The delivery's `parked_until` and `next_attempt_at`.
pub async fn park_times(
    pool: &PgPool,
    id: DeliveryId,
) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    sqlx::query_as("SELECT parked_until, next_attempt_at FROM deliveries WHERE id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Every notice recorded for `user`.
pub async fn notices(pool: &PgPool, user: UserId) -> Vec<Notice> {
    let mut conn = pool.acquire().await.unwrap();
    notifications_for(&mut conn, user)
        .await
        .into_iter()
        .map(|row| row.notice)
        .collect()
}

/// How many attempts the delivery has recorded.
pub async fn attempts(pool: &PgPool, id: DeliveryId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM delivery_attempts WHERE delivery_id = $1")
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Units used in every quota bucket of `account`, summed.
pub async fn account_units(pool: &PgPool, account: AccountId) -> i64 {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(units_used), 0)::bigint FROM platform_quota_usage \
         WHERE account_id = $1",
    )
    .bind(account.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Turns the runtime live-publishing switch on or off.
pub async fn set_live(pool: &PgPool, live: bool) {
    sqlx::query("UPDATE app_settings SET value = $1 WHERE key = 'publishing.live'")
        .bind(Value::Bool(live))
        .execute(pool)
        .await
        .unwrap();
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 200, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn whole(bytes: &[u8]) -> ByteStream {
    futures::stream::iter(vec![Ok::<_, std::io::Error>(Bytes::copy_from_slice(bytes))]).boxed()
}

/// A ready image the owner uploaded.
pub async fn upload(env: &Env, name: &str) -> MediaAssetId {
    let bytes = png(40, 20);
    env.media
        .upload(&env.scope, name, bytes.len() as u64, whole(&bytes))
        .await
        .unwrap()
        .id
}

pub fn account_ref(account: AccountId) -> AccountRef {
    AccountRef {
        platform: PlatformId::new("mock").unwrap(),
        account,
        remote_id: "remote".to_owned(),
    }
}

pub fn variant(account: AccountId, media: &[MediaAssetId]) -> Variant {
    Variant {
        target: account_ref(account),
        kind: PostKind::Post,
        text: "hello".to_owned(),
        media: media
            .iter()
            .map(|asset| MediaItem {
                asset: *asset,
                alt_text: None,
            })
            .collect(),
        link: None,
        tags: Vec::new(),
        disclose_ai_generated: false,
        reply_to: None,
        email: None,
        options: serde_json::Map::new(),
    }
}

pub fn snapshot(variants: Vec<Variant>) -> PublishSnapshot {
    PublishSnapshot {
        post_id: PostId::from_uuid(Uuid::now_v7()),
        post_version: PostVersion(1),
        variants,
    }
}

/// The delivery's payload columns a test checks.
pub struct Row {
    pub remote_id: Option<String>,
    pub remote_url: Option<String>,
    pub attempt_count: i32,
    pub dispatch_gen: i32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub container_ref: Option<String>,
}

pub async fn row(pool: &PgPool, id: DeliveryId) -> Row {
    let (remote_id, remote_url, attempt_count, dispatch_gen, next_attempt_at, container_ref) =
        sqlx::query_as(
            "SELECT remote_id, remote_url, attempt_count, dispatch_gen, next_attempt_at, \
             container_ref FROM deliveries WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    Row {
        remote_id,
        remote_url,
        attempt_count,
        dispatch_gen,
        next_attempt_at,
        container_ref,
    }
}

/// Makes a pending delivery due now.
pub async fn make_due(pool: &PgPool, id: DeliveryId) {
    sqlx::query(
        "UPDATE deliveries SET next_attempt_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(id.as_uuid())
    .execute(pool)
    .await
    .unwrap();
}

/// The payloads and run times of every job of `job_type` in the ledger, oldest first.
pub async fn queued(pool: &PgPool, job_type: &str) -> Vec<(Value, DateTime<Utc>)> {
    sqlx::query_as("SELECT payload, run_at FROM jobs WHERE job_type = $1 ORDER BY created_at")
        .bind(job_type)
        .fetch_all(pool)
        .await
        .unwrap()
}

/// How many audit events of `kind` were recorded.
pub async fn audits(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE kind = $1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Makes every notification insert fail until [`allow_notifications`], to roll back the
/// transaction recording one.
pub async fn break_notifications(pool: &PgPool) {
    sqlx::query(
        "CREATE OR REPLACE FUNCTION test_refuse_notification() RETURNS trigger AS $$ \
         BEGIN RAISE EXCEPTION 'injected failure'; END $$ LANGUAGE plpgsql",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER test_refuse_notification BEFORE INSERT ON notifications \
         FOR EACH ROW EXECUTE FUNCTION test_refuse_notification()",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Undoes [`break_notifications`].
pub async fn allow_notifications(pool: &PgPool) {
    sqlx::query("DROP TRIGGER test_refuse_notification ON notifications")
        .execute(pool)
        .await
        .unwrap();
}

/// The `[publishing]` settings with a short lease, so the renewer runs every `lease / 3`.
pub fn short_lease() -> PublishingSettings {
    PublishingSettings {
        lease: std::time::Duration::from_millis(600),
        ..publishing_settings()
    }
}

/// The account's and its grant's status.
pub async fn account_status(pool: &PgPool, account: AccountId) -> (String, String) {
    sqlx::query_as(
        "SELECT a.status, g.status FROM connected_accounts a \
         JOIN account_grants g ON g.id = a.grant_id WHERE a.id = $1",
    )
    .bind(account.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap()
}
