//! Test harness: the real router over a `#[sqlx::test]` pool, the test OIDC issuer, and a
//! real outbox-backed job queue. Feature `testkit`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use postit_config::{BootstrapSettings, Environment, RateBucket, RateLimitSettings, UserinfoMode};
use postit_core::{Clock, IdGenerator, SystemClock, SystemIdGenerator};
use postit_identity::admin::UserAdminService;
use postit_identity::auth::Authenticate;
use postit_identity::cache::PrincipalCache;
use postit_identity::testkit::{TestIssuer, authenticator_with};
use postit_jobs::JobQueue;
use postit_mail::MailOutbox;
use sqlx::PgPool;
use tower::ServiceExt as _;

use crate::router::api_router;
use crate::settings::ApiSettings;
use crate::state::{AppState, Limits, Readiness};

#[cfg(feature = "ai")]
pub use ai::{
    AI_COMPOSED_MAX, AI_COMPOSED_PLATFORM, AI_TEXT_MAX, AI_TEXT_PLATFORM, DrafterTestOptions,
    seed_ai_account,
};

/// Subject of the pre-created test admin.
pub const ADMIN_SUB: &str = "admin";
/// The VAPID private key [`TestApp::start_with_push`] configures (the RFC 8291 example's
/// sender key; never used outside tests).
pub const TEST_VAPID_PRIVATE_KEY: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";

/// Push as a test app configures it: off, or on with or without private endpoints.
#[derive(Debug, Clone, Copy, Default)]
struct PushOptions {
    enabled: bool,
    allow_private: bool,
}

/// The `email.signing_key` the test app's contact service hashes addresses with.
pub const TEST_EMAIL_SIGNING_KEY: &[u8] = b"test-email-signing-key-0123456789abcdef";

/// The base the test app's signed email links are built under.
pub const TEST_EMAIL_LINK_BASE: &str = "https://api.postit.test/";

/// Peer address attached to test requests that do not set one.
pub const PEER: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, 10)),
    40000,
);

/// A response read fully into memory.
pub struct TestResponse {
    /// Status code.
    pub status: StatusCode,
    /// Response headers.
    pub headers: HeaderMap,
    /// Body parsed as JSON, or null when empty or not JSON.
    pub body: serde_json::Value,
    /// The body as received.
    pub bytes: axum::body::Bytes,
}

/// The real router over a test database and a mock issuer.
pub struct TestApp {
    /// The router under test.
    pub router: Router,
    /// The mock issuer that signs test tokens.
    pub issuer: TestIssuer,
    /// The test database pool.
    pub pool: PgPool,
    /// State the router was built with.
    pub state: AppState,
    /// The directory media is stored in; removed when the app is dropped.
    pub media_dir: tempfile::TempDir,
}

struct DbAndJwks {
    pool: PgPool,
    auth: Arc<dyn Authenticate>,
}

#[async_trait]
impl Readiness for DbAndJwks {
    async fn check(&self) -> Result<(), &'static str> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|_| "database")?;
        if !self.auth.jwks_ready() {
            return Err("jwks");
        }
        Ok(())
    }
}

fn generous() -> RateBucket {
    RateBucket {
        rate_per_minute: 10_000,
        burst: 10_000,
    }
}

/// The contact service over `pool`, reading files up to `settings.contacts_csv_max_bytes`.
fn contact_service(
    pool: &PgPool,
    clock: &Arc<dyn Clock>,
    ids: &Arc<dyn IdGenerator>,
    jobs: &JobQueue,
    settings: &ApiSettings,
) -> Arc<postit_contacts::ContactService> {
    Arc::new(postit_contacts::ContactService::new(
        postit_contacts::ContactsDeps {
            pool: pool.clone(),
            clock: Arc::clone(clock),
            ids: Arc::clone(ids),
            queue: jobs.clone(),
            signing_key: secrecy::SecretSlice::from(TEST_EMAIL_SIGNING_KEY.to_vec()),
            settings: postit_config::ContactsSettings {
                csv_max_bytes: u64::try_from(settings.contacts_csv_max_bytes).unwrap_or(u64::MAX),
                csv_max_rows: 200_000,
            },
        },
    ))
}

fn default_settings(issuer: &TestIssuer) -> ApiSettings {
    ApiSettings {
        environment: Environment::Development,
        cors_origins: vec![axum::http::HeaderValue::from_static(
            "https://app.postit.test",
        )],
        trusted_proxies: vec![],
        rate_limit: RateLimitSettings {
            unauthenticated: generous(),
            provisioning: generous(),
            authenticated: generous(),
            credential_checks: generous(),
            lookup: generous(),
            contact_transfers: generous(),
            email_checks: generous(),
            ai_requests: generous(),
            uploads: generous(),
            publish_actions: generous(),
        },
        request_timeout: Duration::from_secs(10),
        body_limit: 64 * 1024,
        issuer: issuer.issuer_url(),
        client_id: "postit-app".into(),
        android_client_id: None,
        ios_client_id: None,
        scopes: vec!["openid".into(), "profile".into(), "email".into()],
        account_url: None,
        csv_max_bytes: 5 * 1024 * 1024,
        csv_timeout: Duration::from_secs(30),
        contacts_csv_max_bytes: 5 * 1024 * 1024,
        media_single_request_max: 8 * 1024 * 1024,
        media_chunk_max: 1024 * 1024,
        media_idle_timeout: Duration::from_secs(10),
        media_complete_timeout: Duration::from_secs(30),
        ai_request_timeout: Duration::from_secs(30),
    }
}

impl TestApp {
    /// Starts an app over `pool` with default settings.
    pub async fn start(pool: PgPool) -> Self {
        Self::start_with(pool, |_| {}).await
    }

    /// Like [`Self::start`], with push on: the VAPID key is [`TEST_VAPID_PRIVATE_KEY`], and
    /// `allow_private` is `push.allow_private_endpoints`.
    pub async fn start_with_push(pool: PgPool, allow_private: bool) -> Self {
        Self::start_inner(
            pool,
            |_| {},
            UserinfoMode::Never,
            Vec::new(),
            postit_oauth::testkit::TestOptions::default(),
            PushOptions {
                enabled: true,
                allow_private,
            },
        )
        .await
    }

    /// Like [`Self::start`], with these agents compiled in. Register their platforms with
    /// `PlatformRepo::upsert_registered` before connecting accounts.
    pub async fn start_with_agents(
        pool: PgPool,
        agents: Vec<Arc<dyn postit_core::Agent>>,
        configure: impl FnOnce(&mut ApiSettings),
    ) -> Self {
        Self::start_inner(
            pool,
            configure,
            UserinfoMode::Never,
            agents,
            postit_oauth::testkit::TestOptions::default(),
            PushOptions::default(),
        )
        .await
    }

    /// Like [`Self::start_with_agents`], with the account service's test options (platform
    /// tables, the `return_to` allowlist, the callback deadline) for the OAuth routes.
    pub async fn start_with_oauth(
        pool: PgPool,
        agents: Vec<Arc<dyn postit_core::Agent>>,
        configure: impl FnOnce(&mut ApiSettings),
        options: postit_oauth::testkit::TestOptions,
    ) -> Self {
        Self::start_inner(
            pool,
            configure,
            UserinfoMode::Never,
            agents,
            options,
            PushOptions::default(),
        )
        .await
    }

    /// Like [`Self::start`], with the drafter building providers through `factory` (usually a
    /// `postit_drafter::testkit::ScriptedFactory`), private endpoints off. The drafter knows the
    /// platforms [`AI_TEXT_PLATFORM`], [`AI_COMPOSED_PLATFORM`], and `email`; seed accounts on
    /// them with [`seed_ai_account`].
    #[cfg(feature = "ai")]
    pub async fn start_with_drafter(
        pool: PgPool,
        factory: Arc<dyn postit_drafter::ProviderFactory>,
    ) -> Self {
        Self::start_with_drafter_using(
            pool,
            DrafterTestOptions {
                factory,
                allow_private: false,
            },
            |_| {},
        )
        .await
    }

    /// Like [`Self::start_with_drafter`], with the drafter's options, letting `configure`
    /// adjust the API settings first (`ai_request_timeout` is also the drafter's deadline).
    #[cfg(feature = "ai")]
    pub async fn start_with_drafter_using(
        pool: PgPool,
        options: DrafterTestOptions,
        configure: impl FnOnce(&mut ApiSettings),
    ) -> Self {
        let mut app = Self::start_with(pool, configure).await;
        app.state.drafter = ai::drafter_service(&app.pool, &app.state.settings, options);
        app.router = api_router(app.state.clone());
        app
    }

    /// Starts an app, letting `configure` adjust the API settings first.
    pub async fn start_with(pool: PgPool, configure: impl FnOnce(&mut ApiSettings)) -> Self {
        Self::start_full(pool, configure, UserinfoMode::Never).await
    }

    /// Like [`Self::start`], but every cache miss calls the issuer's `userinfo` endpoint with
    /// the caller's bearer token (the redaction test's outbound path). Mount the endpoint
    /// with `app.issuer.mount_userinfo(token, body)` before the first request.
    pub async fn start_with_userinfo(pool: PgPool) -> Self {
        Self::start_full(pool, |_| {}, UserinfoMode::Always).await
    }

    async fn start_full(
        pool: PgPool,
        configure: impl FnOnce(&mut ApiSettings),
        userinfo_mode: UserinfoMode,
    ) -> Self {
        Self::start_inner(
            pool,
            configure,
            userinfo_mode,
            Vec::new(),
            postit_oauth::testkit::TestOptions::default(),
            PushOptions::default(),
        )
        .await
    }

    #[allow(clippy::too_many_lines)]
    async fn start_inner(
        pool: PgPool,
        configure: impl FnOnce(&mut ApiSettings),
        userinfo_mode: UserinfoMode,
        agents: Vec<Arc<dyn postit_core::Agent>>,
        options: postit_oauth::testkit::TestOptions,
        push: PushOptions,
    ) -> Self {
        let issuer = TestIssuer::start().await;
        let mut settings = default_settings(&issuer);
        configure(&mut settings);
        let bootstrap = BootstrapSettings {
            admin_email: Some(format!("{ADMIN_SUB}@postit.test")),
            admin_subject: None,
        };
        let auth: Arc<dyn Authenticate> = Arc::new(authenticator_with(
            pool.clone(),
            &issuer,
            bootstrap,
            PrincipalCache::new(Duration::from_secs(60)),
            userinfo_mode,
        ));
        let compiled_platforms: Vec<postit_core::PlatformId> = agents
            .iter()
            .map(|agent| agent.descriptor().platform.clone())
            .collect();
        let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let media_dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("temp dir: {e}"));
        let media = postit_media::testkit::media_service(
            pool.clone(),
            postit_media::testkit::media_settings(media_dir.path()),
            Arc::clone(&clock),
        )
        .unwrap_or_else(|e| unreachable!("media service: {e}"));
        let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids), Arc::clone(&clock));
        let mut catalog = postit_jobs::ConsoleCatalog::default();
        postit_mail::console(&mut catalog);
        let schedules = postit_jobs::testkit::jobs_settings().schedules;
        postit_identity::jobs::console(&mut catalog, &schedules);
        postit_jobs::maintenance::console(&mut catalog, &postit_jobs::testkit::jobs_settings());
        let console =
            postit_jobs::JobConsole::new(pool.clone(), jobs.clone(), catalog, Arc::clone(&clock));
        let delegations = postit_identity::delegation::DelegationService::new(
            pool.clone(),
            Arc::clone(&ids),
            Arc::clone(&clock),
            postit_notify::Notifier::for_tests(pool.clone()),
        );
        let push = postit_notify::PushEndpoints::new(postit_notify::PushEndpointsDeps {
            pool: pool.clone(),
            vault: postit_vault::Vault::from_keys([(
                1,
                b"0123456789abcdef0123456789abcdef-test-key".to_vec(),
            )])
            .unwrap_or_else(|e| unreachable!("test vault: {e}")),
            ids: Arc::clone(&ids),
            keys: push.enabled.then(|| {
                Arc::new(
                    postit_notify::vapid::VapidKeys::new(
                        TEST_VAPID_PRIVATE_KEY,
                        "mailto:ops@postit.test",
                    )
                    .unwrap_or_else(|e| unreachable!("test VAPID key: {e}")),
                )
            }),
            allow_private: push.allow_private,
        });
        let contacts = contact_service(&pool, &clock, &ids, &jobs, &settings);
        #[cfg(feature = "ai")]
        let drafter = ai::drafter_service(&pool, &settings, DrafterTestOptions::unscripted());
        let accounts = postit_oauth::testkit::account_service_with(
            pool.clone(),
            agents,
            postit_oauth::testkit::test_accounts_settings(),
            options,
        );
        let publishing =
            postit_publishing::PublishingService::new(postit_publishing::PublishingDeps {
                pool: pool.clone(),
                accounts: accounts.clone(),
                media: media.clone(),
                notifier: postit_notify::Notifier::for_tests(pool.clone()),
                queue: jobs.clone(),
                ids: Arc::clone(&ids),
                clock: Arc::clone(&clock),
                settings: postit_config::PublishingSettings {
                    live: true,
                    max_attempts: 5,
                    lease: Duration::from_mins(5),
                    sweep_grace: Duration::from_mins(2),
                    park_grace: Duration::from_mins(15),
                    processing_timeout: Duration::from_hours(24),
                },
                shutdown: tokio_util::sync::CancellationToken::new(),
                compiled_platforms,
                email_batch_size: 50,
                email_image_ttl: Duration::from_hours(365 * 24),
            });
        let admin = UserAdminService::new(pool.clone(), ids, jobs.clone(), MailOutbox::new(jobs));
        let state = AppState {
            readiness: Arc::new(DbAndJwks {
                pool: pool.clone(),
                auth: Arc::clone(&auth),
            }),
            auth,
            admin,
            pool: pool.clone(),
            limits: Arc::new(Limits::new(&settings.rate_limit)),
            settings: Arc::new(settings),
            console,
            clock,
            accounts,
            publishing,
            media,
            delegations,
            push,
            contacts,
            email_links: url::Url::parse(TEST_EMAIL_LINK_BASE)
                .ok()
                .and_then(|base| {
                    crate::email_links::EmailLinks::new(TEST_EMAIL_SIGNING_KEY, base).ok()
                })
                .unwrap_or_else(|| unreachable!("the test link base is an https URL")),
            #[cfg(feature = "ai")]
            drafter,
        };
        Self {
            router: api_router(state.clone()),
            issuer,
            pool,
            state,
            media_dir,
        }
    }

    /// A bearer token for user `sub`, with a verified email.
    #[must_use]
    pub fn token(&self, sub: &str) -> String {
        let email = format!("{sub}@postit.test");
        self.issuer
            .token_and_claims(sub, Some(&email), Some(true), Some(sub))
            .0
    }

    /// Sends `req`, attaching the default peer address if it has none.
    pub async fn send(&self, mut req: Request<Body>) -> TestResponse {
        if req.extensions().get::<ConnectInfo<SocketAddr>>().is_none() {
            req.extensions_mut().insert(ConnectInfo(PEER));
        }
        let res = self
            .router
            .clone()
            .oneshot(req)
            .await
            .unwrap_or_else(|e| unreachable!("router is infallible: {e}"));
        let (parts, body) = res.into_parts();
        let bytes = body
            .collect()
            .await
            .unwrap_or_else(|e| unreachable!("collect body: {e}"))
            .to_bytes();
        TestResponse {
            status: parts.status,
            headers: parts.headers,
            body: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
            bytes,
        }
    }

    /// Makes a job's pending deliveries due now: their `next_attempt_at`, and the `run_at` of
    /// their queued `publishing` ledger rows, move to one second ago. The testkit runs on the
    /// system clock with no worker, so tests move time in SQL and call [`Self::drain_publishing`].
    pub async fn make_job_due(&self, job: postit_core::PublishJobId) {
        sqlx::query(
            "UPDATE deliveries SET next_attempt_at = now() - interval '1 second' \
             WHERE job_id = $1 AND state = 'pending'",
        )
        .bind(job.as_uuid())
        .execute(&self.pool)
        .await
        .unwrap_or_else(|e| unreachable!("make deliveries due: {e}"));
        sqlx::query(
            "UPDATE jobs SET run_at = now() - interval '1 second' \
             WHERE queue = 'publishing' AND state = 'queued' \
               AND (payload->>'delivery')::uuid IN \
                   (SELECT id FROM deliveries WHERE job_id = $1 AND state = 'pending')",
        )
        .bind(job.as_uuid())
        .execute(&self.pool)
        .await
        .unwrap_or_else(|e| unreachable!("make ledger rows due: {e}"));
    }

    /// Runs the queued, due `publishing` ledger rows by hand until none is left, and returns
    /// how many it ran. No worker moves the rows in this harness, so it remembers the IDs it
    /// ran and never runs one twice; runners enqueue follow-ups under new IDs.
    ///
    /// # Panics
    /// On a job type it does not know, or when 50 passes still find new rows.
    pub async fn drain_publishing(&self) -> usize {
        use postit_publishing::{
            DirectSendBatch, PollDeliveryStatus, PublishDelivery, ReconcileDelivery,
        };
        let mut ran = std::collections::HashSet::new();
        let service = &self.state.publishing;
        for _ in 0..50 {
            let rows: Vec<(uuid::Uuid, String, serde_json::Value)> = sqlx::query_as(
                "SELECT id, job_type, payload FROM jobs \
                 WHERE queue = 'publishing' AND state = 'queued' AND run_at <= now() \
                 ORDER BY run_at, created_at",
            )
            .fetch_all(&self.pool)
            .await
            .unwrap_or_else(|e| unreachable!("read the ledger: {e}"));
            let fresh: Vec<_> = rows.into_iter().filter(|r| ran.insert(r.0)).collect();
            if fresh.is_empty() {
                return ran.len();
            }
            for (_, job_type, payload) in fresh {
                let outcome = match job_type.as_str() {
                    "publish_delivery" => {
                        let job: PublishDelivery = serde_json::from_value(payload)
                            .unwrap_or_else(|e| unreachable!("payload: {e}"));
                        postit_publishing::run_publish(service, job).await
                    }
                    "direct_send_batch" => {
                        let job: DirectSendBatch = serde_json::from_value(payload)
                            .unwrap_or_else(|e| unreachable!("payload: {e}"));
                        postit_publishing::direct::run_batch(service, job).await
                    }
                    "poll_delivery_status" => {
                        let job: PollDeliveryStatus = serde_json::from_value(payload)
                            .unwrap_or_else(|e| unreachable!("payload: {e}"));
                        postit_publishing::poll::run_poll(service, job).await
                    }
                    "reconcile_delivery" => {
                        let job: ReconcileDelivery = serde_json::from_value(payload)
                            .unwrap_or_else(|e| unreachable!("payload: {e}"));
                        postit_publishing::reconcile::run_reconcile(service, job).await
                    }
                    other => unreachable!("drain_publishing does not run {other}"),
                };
                outcome.unwrap_or_else(|e| unreachable!("a {job_type} job failed: {e}"));
            }
        }
        unreachable!("drain_publishing found new jobs after 50 passes");
    }

    /// Sends a request with an optional bearer token and JSON body.
    pub async fn call(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(token) = bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let req = match body {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap_or_else(|e| unreachable!("request: {e}"));
        self.send(req).await
    }
}

/// The drafter as test apps build it. Feature `ai`.
#[cfg(feature = "ai")]
mod ai {
    use std::sync::Arc;

    use postit_config::AiSettings;
    use postit_core::{AccountId, SystemClock, SystemIdGenerator, TextMeasure, UserId};
    use postit_drafter::testkit::{
        NoFrames, ScriptedFactory, ScriptedProvider, composed_descriptor, email_descriptor,
        registry, text_descriptor,
    };
    use postit_drafter::{DrafterDeps, DrafterService, ProviderFactory};
    use sqlx::PgPool;
    use uuid::Uuid;

    use crate::settings::ApiSettings;

    /// A platform the test drafter knows: one text format of [`AI_TEXT_MAX`] graphemes.
    pub const AI_TEXT_PLATFORM: &str = "textish";
    /// The text limit of [`AI_TEXT_PLATFORM`].
    pub const AI_TEXT_MAX: u32 = 300;
    /// A platform the test drafter knows whose text limit counts the link and tags, like
    /// Telegram's.
    pub const AI_COMPOSED_PLATFORM: &str = "telegramish";
    /// The text limit of [`AI_COMPOSED_PLATFORM`].
    pub const AI_COMPOSED_MAX: u32 = 4_096;

    /// How a test app's drafter is built.
    pub struct DrafterTestOptions {
        /// Builds a provider from each stored row.
        pub factory: Arc<dyn ProviderFactory>,
        /// `ai.allow_private_endpoints`.
        pub allow_private: bool,
    }

    impl DrafterTestOptions {
        /// A scripted provider with nothing queued, for apps whose tests do not generate.
        pub(super) fn unscripted() -> Self {
            Self {
                factory: Arc::new(ScriptedFactory::new(Arc::new(ScriptedProvider::new()))),
                allow_private: false,
            }
        }
    }

    pub(super) fn drafter_service(
        pool: &PgPool,
        settings: &ApiSettings,
        options: DrafterTestOptions,
    ) -> DrafterService {
        let vault = postit_vault::Vault::from_keys([(
            1,
            b"0123456789abcdef0123456789abcdef-test-key".to_vec(),
        )])
        .unwrap_or_else(|e| unreachable!("test vault: {e}"));
        DrafterService::new(DrafterDeps {
            pool: pool.clone(),
            vault: Arc::new(vault),
            clock: Arc::new(SystemClock),
            ids: Arc::new(SystemIdGenerator),
            registry: Arc::new(registry([
                text_descriptor(AI_TEXT_PLATFORM, AI_TEXT_MAX, TextMeasure::Graphemes),
                composed_descriptor(AI_COMPOSED_PLATFORM, AI_COMPOSED_MAX),
                email_descriptor(),
            ])),
            frames: Arc::new(NoFrames),
            factory: options.factory,
            settings: AiSettings {
                request_timeout: settings.ai_request_timeout,
                allow_private_endpoints: options.allow_private,
            },
        })
        .unwrap_or_else(|e| unreachable!("test drafter: {e}"))
    }

    /// Connects an account of `owner` on `platform` (one the test drafter knows), recording the
    /// platform first.
    pub async fn seed_ai_account(pool: &PgPool, owner: UserId, platform: &str) -> AccountId {
        sqlx::query(
            "INSERT INTO platforms (id, display_name) VALUES ($1, $1) ON CONFLICT DO NOTHING",
        )
        .bind(platform)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("seed platform: {e}"));
        let id = AccountId::from(Uuid::now_v7());
        sqlx::query(
            "INSERT INTO connected_accounts
                 (id, owner_id, platform_id, remote_account_id, display_name)
             VALUES ($1, $2, $3, $4, 'Shop')",
        )
        .bind(id.as_uuid())
        .bind(owner.as_uuid())
        .bind(platform)
        .bind(format!("r-{id}"))
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("seed account: {e}"));
        id
    }
}
