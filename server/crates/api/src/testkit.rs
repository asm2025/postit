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
use postit_core::{IdGenerator, SystemIdGenerator};
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

pub const ADMIN_SUB: &str = "admin";
pub const PEER: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, 10)),
    40000,
);

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: serde_json::Value,
}

pub struct TestApp {
    pub router: Router,
    pub issuer: TestIssuer,
    pub pool: PgPool,
    pub state: AppState,
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
        },
        request_timeout: Duration::from_secs(10),
        body_limit: 64 * 1024,
        issuer: issuer.issuer_url(),
        client_id: "postit-app".into(),
        scopes: vec!["openid".into(), "profile".into(), "email".into()],
        account_url: None,
    }
}

impl TestApp {
    pub async fn start(pool: PgPool) -> Self {
        Self::start_with(pool, |_| {}).await
    }

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
        let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
        let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids));
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
        };
        Self {
            router: api_router(state.clone()),
            issuer,
            pool,
            state,
        }
    }

    #[must_use]
    pub fn token(&self, sub: &str) -> String {
        let email = format!("{sub}@postit.test");
        self.issuer
            .token_and_claims(sub, Some(&email), Some(true), Some(sub))
            .0
    }

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
        }
    }

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
