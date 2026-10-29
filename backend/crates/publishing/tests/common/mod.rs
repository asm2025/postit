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
    PublishSnapshot, QuotaBucket, SystemClock, SystemIdGenerator, UserId, Variant,
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
        settings: publishing_settings(),
        shutdown: CancellationToken::new(),
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
    let service = service_with(
        &env,
        accounts.clone(),
        options.compiled.unwrap_or_else(|| vec![mock()]),
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
    let mut tx = pool.begin().await.unwrap();
    PostRepo::create(
        &mut tx,
        &scope,
        NewPost {
            id: post,
            kind: PostKind::Post,
            reply_to: None,
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
        .map(|account| {
            let mut v = variant(*account, &spec.media);
            v.target.platform = spec.platform.clone();
            v.text.clone_from(&spec.text);
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
                    operation: Operation::Publish,
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
