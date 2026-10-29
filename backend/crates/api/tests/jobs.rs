//! Admin job console read endpoints.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{DateTime, Utc};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_config::{AuditSettings, RedactedSecret};
use postit_core::{IdGenerator, SystemClock, SystemIdGenerator, UserId};
use postit_data::users::UserRecord;
use postit_jobs::testkit::{RunningWorker, finished_job_ids, jobs_settings, wait_until_async};
use postit_jobs::{Job, JobContext, JobQueue, JobRegistry, Queue, RetryPolicy};
use postit_mail::{
    LoadOutcome, MailContent, MailContextLoader, MailError, MailKind, MailLoaders, MailOutbox,
    MailParams, Mailer, MemoryMailer, RenderedMessage, SendEmail, SendEmailDeps, SendEmailHandler,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use url::Url;
use uuid::Uuid;

/// Each worker test holds ~10 database connections; tests that start a worker run one at a
/// time so a full run stays under Postgres's default `max_connections`.
static WORKERS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Signs `sub` in (provisioned `pending`) and, when `approve`, has the admin set it `active`.
async fn user_token(app: &TestApp, sub: &str, approve: bool) -> String {
    let token = app.token(sub);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    if approve {
        let id = me.body["id"].as_str().unwrap_or_default().to_string();
        let res = app
            .call(
                Method::PATCH,
                &format!("/api/v1/users/{id}"),
                Some(&app.token(ADMIN_SUB)),
                Some(serde_json::json!({ "status": "active" })),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK);
    }
    token
}

#[sqlx::test(migrations = "../data/migrations")]
async fn members_pending_users_and_anonymous_callers_are_refused_on_read_routes(pool: PgPool) {
    let app = TestApp::start(pool).await;
    app.call(Method::GET, "/api/v1/me", Some(&app.token(ADMIN_SUB)), None)
        .await; // bootstrap admin first
    let member = user_token(&app, "mallory", true).await;
    let pending = user_token(&app, "pat", false).await;
    let id = uuid::Uuid::now_v7();
    let routes = [
        "/api/v1/admin/jobs".to_string(),
        "/api/v1/admin/jobs/stats".to_string(),
        format!("/api/v1/admin/jobs/{id}"),
        "/api/v1/admin/jobs/recurring".to_string(),
    ];
    for path in routes {
        let anon = app.call(Method::GET, &path, None, None).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED, "{path}");
        for token in [&member, &pending] {
            let denied = app.call(Method::GET, &path, Some(token), None).await;
            assert_eq!(denied.status, StatusCode::FORBIDDEN, "{path}");
        }
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_rejects_bad_filters_with_422(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    for query in [
        "state=nope",
        "from=2026-02-01T00:00:00Z&to=2026-01-01T00:00:00Z",
        "page=0",
        "page_size=101",
        "bogus=1",
    ] {
        let res = app
            .call(
                Method::GET,
                &format!("/api/v1/admin/jobs?{query}"),
                Some(&admin),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
        assert_eq!(res.body["code"], "validation_failed");
    }
    for query in ["window_hours=0", "window_hours=169"] {
        let res = app
            .call(
                Method::GET,
                &format!("/api/v1/admin/jobs/stats?{query}"),
                Some(&admin),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_unknown_job_id_is_404(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let res = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/jobs/{}", uuid::Uuid::now_v7()),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(
        (res.status, res.body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_filters_paginates_and_detail_maps_actions(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let mut dead_id = uuid::Uuid::nil();
    for i in 0..3 {
        let id = uuid::Uuid::now_v7();
        if i > 0 {
            dead_id = id;
        }
        sqlx::query("INSERT INTO jobs (id, job_type, queue, payload, state, run_at, finished_at) VALUES ($1, 'send_email', 'mail', '{}', $2, now(), CASE WHEN $2 = 'succeeded' THEN now() END)")
            .bind(id)
            .bind(if i == 0 { "succeeded" } else { "dead" })
            .execute(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let dead = app
        .call(
            Method::GET,
            "/api/v1/admin/jobs?state=dead&page_size=1&page=2",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(dead.status, StatusCode::OK);
    assert_eq!(dead.body["total"], 2);
    assert_eq!(dead.body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(dead.body["data"][0]["state"], "dead");

    let detail = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/jobs/{dead_id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.body["job"]["state"], "dead");
    assert_eq!(detail.body["job"]["actions"]["retry"], true);
    assert_eq!(detail.body["job"]["actions"]["cancel"], false);
    assert!(detail.body["attempts"].is_array());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_delete_user_job_reports_the_must_finish_reason_code(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO jobs (id, job_type, queue, payload, state, run_at, finished_at) VALUES ($1, 'delete_user', 'default', '{}', 'dead', now(), now())")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let detail = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/jobs/{id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.body["job"]["actions_disabled_reason"], "must_finish");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stats_and_recurring_return_the_documented_shapes(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let stats = app
        .call(Method::GET, "/api/v1/admin/jobs/stats", Some(&admin), None)
        .await;
    assert_eq!(stats.status, StatusCode::OK);
    assert!(stats.body["states"].is_array() && stats.body["throughput"].is_array());
    assert_eq!(stats.body["window_hours"], 24);
    let recurring = app
        .call(
            Method::GET,
            "/api/v1/admin/jobs/recurring",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(recurring.status, StatusCode::OK);
    assert!(recurring.body.is_array());
}

async fn audit_count(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE kind = $1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

async fn insert_job(pool: &PgPool, state: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO jobs (id, job_type, queue, payload, state, run_at, finished_at) VALUES ($1, 'purge_pending_users', 'maintenance', '{}', $2, now(), CASE WHEN $2 IN ('succeeded','dead','killed','cancelled') THEN now() END)")
        .bind(id)
        .bind(state)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    id
}

async fn ledger_state(pool: &PgPool, id: &str) -> String {
    sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn actions_are_admin_only_and_unknown_jobs_are_404(pool: PgPool) {
    let app = TestApp::start(pool).await;
    app.call(Method::GET, "/api/v1/me", Some(&app.token(ADMIN_SUB)), None)
        .await;
    let member = user_token(&app, "mallory", true).await;
    let pending = user_token(&app, "pat", false).await;
    let admin = app.token(ADMIN_SUB);
    let id = Uuid::now_v7();
    let by_id = [
        (Method::POST, format!("/api/v1/admin/jobs/{id}/retry")),
        (Method::POST, format!("/api/v1/admin/jobs/{id}/cancel")),
        (Method::DELETE, format!("/api/v1/admin/jobs/{id}")),
    ];
    let trigger = (
        Method::POST,
        "/api/v1/admin/jobs/recurring/purge_pending_users/trigger".to_string(),
    );
    for (method, path) in by_id.iter().chain(std::iter::once(&trigger)) {
        let anon = app.call(method.clone(), path, None, None).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED, "{path}");
        for token in [&member, &pending] {
            let denied = app.call(method.clone(), path, Some(token), None).await;
            assert_eq!(denied.status, StatusCode::FORBIDDEN, "{path}");
        }
    }
    for (method, path) in by_id {
        let res = app.call(method, &path, Some(&admin), None).await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancel_writes_its_audit_event_and_a_second_cancel_is_409(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let id = insert_job(&pool, "queued").await;
    let first = app
        .call(
            Method::POST,
            &format!("/api/v1/admin/jobs/{id}/cancel"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(first.status, StatusCode::NO_CONTENT);
    assert_eq!(audit_count(&pool, "job_cancelled").await, 1);
    let second = app
        .call(
            Method::POST,
            &format!("/api/v1/admin/jobs/{id}/cancel"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(
        (second.status, second.body["code"].as_str()),
        (StatusCode::CONFLICT, Some("job_action_not_allowed"))
    );
    assert_eq!(audit_count(&pool, "job_cancelled").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retry_returns_the_new_job_and_audits_both_ids(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let id = insert_job(&pool, "dead").await;
    let res = app
        .call(
            Method::POST,
            &format!("/api/v1/admin/jobs/{id}/retry"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let new_id = res.body["job_id"].as_str().unwrap_or_default().to_string();
    assert_ne!(new_id, id.to_string());
    let details: serde_json::Value =
        sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = 'job_retried'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(details["job_id"], id.to_string());
    assert_eq!(details["new_job_id"], new_id);
    assert_eq!(details["job_type"], "purge_pending_users");

    // The retried job no longer offers Retry, and says why.
    let parent = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/jobs/{id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(parent.status, StatusCode::OK);
    assert_eq!(parent.body["job"]["actions"]["retry"], false);
    assert_eq!(parent.body["retried_by"], new_id);

    // A refused retry (running job) audits nothing.
    let running = insert_job(&pool, "running").await;
    let refused = app
        .call(
            Method::POST,
            &format!("/api/v1/admin/jobs/{running}/retry"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(audit_count(&pool, "job_retried").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_and_trigger_are_audited(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let id = insert_job(&pool, "succeeded").await;
    let del = app
        .call(
            Method::DELETE,
            &format!("/api/v1/admin/jobs/{id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
    assert_eq!(audit_count(&pool, "job_deleted").await, 1);
    let trig = app
        .call(
            Method::POST,
            "/api/v1/admin/jobs/recurring/purge_pending_users/trigger",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(trig.status, StatusCode::ACCEPTED);
    assert!(trig.body["job_id"].is_string());
    assert_eq!(audit_count(&pool, "job_triggered").await, 1);
    let details: serde_json::Value =
        sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = 'job_triggered'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(details["name"], "purge_pending_users");
    assert_eq!(details["job_id"], trig.body["job_id"]);
    let missing = app
        .call(
            Method::POST,
            "/api/v1/admin/jobs/recurring/nope/trigger",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(audit_count(&pool, "job_triggered").await, 1);
}

// ---- exit-criteria tests: a real worker behind the console ----

/// Fails the first `failures` sends with a transient error that names the recipient, then
/// records into `inner`.
struct FlakyMailer {
    inner: MemoryMailer,
    failures: AtomicUsize,
}

#[async_trait]
impl Mailer for FlakyMailer {
    async fn send(&self, message: RenderedMessage) -> Result<(), MailError> {
        if self
            .failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(MailError::Transient("smtp down: ada@example.com".into()));
        }
        self.inner.send(message).await
    }
}

struct ApprovedLoader;

#[async_trait]
impl MailContextLoader for ApprovedLoader {
    async fn load(
        &self,
        _conn: &mut PgConnection,
        _recipient: &UserRecord,
        _params: &MailParams,
        _now: DateTime<Utc>,
    ) -> Result<LoadOutcome, MailError> {
        Ok(LoadOutcome::Send(MailContent::UserApproved {
            display_name: "Ada Lovelace".into(),
        }))
    }

    async fn mark_sent(
        &self,
        _conn: &mut PgConnection,
        _recipient: UserId,
        _at: DateTime<Utc>,
    ) -> Result<(), MailError> {
        Ok(())
    }
}

fn job_queue(pool: &PgPool) -> JobQueue {
    JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(SystemClock),
    )
}

async fn verified_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, email, email_verified, display_name, role, status)
         VALUES ($1, 'https://i.test', $1::text, 'ada@example.com', true, 'Ada Lovelace', 'member', 'active')",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert user: {e}"));
    id
}

/// A worker whose `send_email` handler uses `mailer` and gives up after one try.
async fn mail_worker(pool: &PgPool, mailer: Arc<dyn Mailer>) -> RunningWorker {
    let mut loaders = MailLoaders::default();
    loaders
        .register(MailKind::UserApproved, Arc::new(ApprovedLoader))
        .unwrap_or_else(|e| unreachable!("register loader: {e}"));
    let handler = SendEmailHandler::new(SendEmailDeps {
        pool: pool.clone(),
        ids: Arc::new(SystemIdGenerator),
        mailer,
        loaders,
        outbox: MailOutbox::new(job_queue(pool)),
        app_url: Url::parse("https://app.postit.test").unwrap_or_else(|e| unreachable!("{e}")),
        max_attempts: 1,
    });
    let mut registry = JobRegistry::default();
    postit_mail::register(&mut registry, handler).unwrap_or_else(|e| unreachable!("register: {e}"));
    RunningWorker::start(pool.clone(), registry).await
}

/// Runs a worker with the flaky mailer and returns it with the ID of a `send_email` job
/// that has exhausted its single try and is `dead`.
async fn dead_send_email(pool: &PgPool, mailer: &Arc<FlakyMailer>) -> (RunningWorker, String) {
    let recipient = verified_user(pool).await;
    let worker = mail_worker(pool, Arc::clone(mailer) as Arc<dyn Mailer>).await;
    let id = job_queue(pool)
        .enqueue(&SendEmail {
            kind: MailKind::UserApproved,
            recipient,
            params: MailParams::None,
        })
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"))
        .0
        .to_string();
    let reached = wait_until_async(Duration::from_secs(15), || async {
        ledger_state(pool, &id).await == "dead"
    })
    .await;
    assert!(reached, "job never died: {}", ledger_state(pool, &id).await);
    (worker, id)
}

fn flaky(failures: usize) -> Arc<FlakyMailer> {
    Arc::new(FlakyMailer {
        inner: MemoryMailer::default(),
        failures: AtomicUsize::new(failures),
    })
}

#[sqlx::test(migrations = "../data/migrations")]
async fn retrying_a_dead_send_email_job_delivers_the_email_once(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let mailer = flaky(99);
    let (worker, id) = dead_send_email(&pool, &mailer).await;
    assert!(mailer.inner.sent().is_empty());

    mailer.failures.store(0, Ordering::SeqCst);
    let res = app
        .call(
            Method::POST,
            &format!("/api/v1/admin/jobs/{id}/retry"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let new_id = res.body["job_id"].as_str().unwrap_or_default().to_string();
    let done = wait_until_async(Duration::from_secs(15), || async {
        ledger_state(&pool, &new_id).await == "succeeded"
    })
    .await;
    worker.stop().await;
    assert!(done, "the retried job never succeeded");
    assert_eq!(mailer.inner.sent().len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_console_never_leaks_a_send_email_failure(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let mailer = flaky(99);
    let (worker, id) = dead_send_email(&pool, &mailer).await;
    worker.stop().await;

    let detail = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/jobs/{id}"),
            Some(&admin),
            None,
        )
        .await;
    let list = app
        .call(
            Method::GET,
            "/api/v1/admin/jobs?state=dead",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(list.status, StatusCode::OK);
    let attempts = detail.body["attempts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!attempts.is_empty());
    for attempt in &attempts {
        assert!(attempt["error_message"].is_null(), "{attempt}");
    }
    for body in [&detail.body, &list.body] {
        let text = body.to_string();
        // The address, the SMTP detail, the recipient's name, and the rendered subject and
        // body of the `user_approved` mail.
        for needle in [
            "ada@example.com",
            "smtp down",
            "Ada",
            "Lovelace",
            "account was approved",
            "administrator approved",
        ] {
            assert!(!text.contains(needle), "leaked {needle}: {text}");
        }
    }
}

/// Stands in for the real `purge_pending_users` handler, counting runs. The type is
/// catalogued, so the console's guard applies to it.
#[derive(Serialize, Deserialize)]
struct CountedPurge {}

impl Job for CountedPurge {
    const JOB_TYPE: &'static str = "purge_pending_users";
    const QUEUE: Queue = Queue::Maintenance;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cancelling_a_scheduled_job_stops_it_from_running(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let runs = Arc::new(AtomicUsize::new(0));
    let mut registry = JobRegistry::default();
    let counter = Arc::clone(&runs);
    registry
        .register(RetryPolicy::None, move |_: CountedPurge, _: JobContext| {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;

    let due = Utc::now() + chrono::Duration::seconds(3);
    let id = job_queue(&pool)
        .enqueue_at(&CountedPurge {}, due)
        .await
        .unwrap_or_else(|e| unreachable!("enqueue: {e}"))
        .0;
    // Let the relay move the task into storage first, so the worker is what must skip it.
    let relayed = wait_until_async(Duration::from_secs(10), || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM job_outbox WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or(1)
            == 0
    })
    .await;
    assert!(relayed, "the relay never moved the job");

    let res = app
        .call(
            Method::POST,
            &format!("/api/v1/admin/jobs/{id}/cancel"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);

    // The worker acks the task after the due time without running the handler.
    let acked = wait_until_async(Duration::from_secs(15), || async {
        finished_job_ids(&pool).await.contains(&id)
    })
    .await;
    worker.stop().await;
    assert!(acked, "the worker never reached the cancelled task");
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert_eq!(ledger_state(&pool, &id.to_string()).await, "cancelled");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn trigger_now_runs_purge_pending_users(pool: PgPool) {
    let _serial = WORKERS.lock().await;
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let settings = jobs_settings();
    let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
    let mut registry = JobRegistry::default();
    let mut loaders = MailLoaders::default();
    postit_identity::jobs::register(
        &mut registry,
        &mut loaders,
        postit_identity::jobs::IdentityJobs {
            pool: pool.clone(),
            ids: Arc::clone(&ids),
            clock: Arc::new(SystemClock),
            jobs: job_queue(&pool),
            notifier: postit_notify::Notifier::for_tests(pool.clone()),
            pending_ttl: Duration::from_hours(30 * 24),
            approval_email_interval: Duration::from_hours(1),
            audit: AuditSettings {
                retention: Duration::from_hours(365 * 24),
                ip_retention: Duration::from_hours(90 * 24),
                pseudonym_key: RedactedSecret::from("k".to_string()),
            },
            schedules: settings.schedules.clone(),
            deletion_steps: Vec::new(),
        },
    )
    .unwrap_or_else(|e| unreachable!("identity register: {e}"));
    let worker = RunningWorker::start_with(pool.clone(), settings, registry).await;

    let stale = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status, created_at)
         VALUES ($1, 'https://i.test', 'stale', 'stale', 'member', 'pending', now() - interval '40 days')",
    )
    .bind(stale)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let res = app
        .call(
            Method::POST,
            "/api/v1/admin/jobs/recurring/purge_pending_users/trigger",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::ACCEPTED);
    let job_id = res.body["job_id"].as_str().unwrap_or_default().to_string();

    let ran = wait_until_async(Duration::from_secs(20), || async {
        ledger_state(&pool, &job_id).await == "succeeded"
    })
    .await;
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM users WHERE id = $1")
        .bind(stale)
        .fetch_optional(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    worker.stop().await;
    assert!(ran, "the triggered job did not succeed");
    assert!(
        status.as_ref().is_none_or(|s| s == "deleting"),
        "the stale pending user was not purged: {status:?}"
    );
}

/// The DTO maps a state it does not know to `failed` (with a warning). That fallback must stay
/// unreachable: every state the `jobs.state` CHECK constraint allows has a wire name, so a
/// state added to the schema without a `JobStateDto` variant fails here, not in production.
#[sqlx::test(migrations = "../data/migrations")]
async fn every_stored_job_state_has_a_wire_state(pool: PgPool) {
    let defs: Vec<String> = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint
         WHERE conrelid = 'jobs'::regclass AND contype = 'c'
           AND pg_get_constraintdef(oid) LIKE '%(state = ANY%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    let [def] = defs.as_slice() else {
        unreachable!("expected exactly one state list constraint: {defs:?}");
    };
    let states: Vec<&str> = def.split('\'').skip(1).step_by(2).collect();
    assert!(states.len() >= 7, "{def}");
    for state in states {
        assert!(
            postit_api::dto_jobs::JobStateDto::parse(state).is_some(),
            "stored state `{state}` has no JobStateDto variant"
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_repeated_trigger_with_the_same_key_starts_one_run(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let trigger = || {
        Request::post("/api/v1/admin/jobs/recurring/purge_pending_users/trigger")
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .header("idempotency-key", "trigger-1")
            .body(Body::empty())
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    let first = app.send(trigger()).await;
    assert_eq!(first.status, StatusCode::ACCEPTED);
    let replay = app.send(trigger()).await;
    assert_eq!(replay.status, StatusCode::ACCEPTED);
    assert_eq!(replay.body["job_id"], first.body["job_id"]);
    assert_eq!(audit_count(&pool, "job_triggered").await, 1);
    let runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM jobs WHERE recurring_name = 'purge_pending_users'",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(runs, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_failed_trigger_frees_its_idempotency_key(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let trigger = |name: &str| {
        Request::post(format!("/api/v1/admin/jobs/recurring/{name}/trigger"))
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .header("idempotency-key", "trigger-2")
            .body(Body::empty())
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    assert_eq!(
        app.send(trigger("nope")).await.status,
        StatusCode::NOT_FOUND
    );
    // The 404 released the key, so the same request is not stuck `in_progress`.
    assert_eq!(
        app.send(trigger("nope")).await.status,
        StatusCode::NOT_FOUND
    );
    let first = app.send(trigger("purge_pending_users")).await;
    assert_eq!(first.status, StatusCode::ACCEPTED);
    // Once a run holds the key, the key is bound to its route: another trigger is refused.
    let other = app.send(trigger("audit_retention")).await;
    assert_eq!(
        (other.status, other.body["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("idempotency_key_reused")
        )
    );
}
