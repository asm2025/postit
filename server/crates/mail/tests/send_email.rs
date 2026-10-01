use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use postit_core::{SystemIdGenerator, UserId};
use postit_data::users::UserRecord;
use postit_jobs::testkit::{RunningWorker, wait_until};
use postit_jobs::{JobContext, JobError, JobId, JobQueue, JobRegistry};
use postit_mail::{
    LoadOutcome, MailContent, MailContextLoader, MailError, MailKind, MailLoaders, MailOutbox,
    MailParams, Mailer, MemoryMailer, RenderedMessage, SendEmail, SendEmailDeps, SendEmailHandler,
};
use sqlx::{PgConnection, PgPool};
use url::Url;
use uuid::Uuid;

/// Returns a scripted outcome and records `mark_sent` calls.
#[derive(Default)]
struct ScriptedLoader {
    outcome: Mutex<Option<LoadOutcome>>,
    marked: Mutex<Vec<UserId>>,
}

impl ScriptedLoader {
    fn with(outcome: LoadOutcome) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(Some(outcome)),
            marked: Mutex::new(Vec::new()),
        })
    }
    fn marked(&self) -> Vec<UserId> {
        self.marked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl MailContextLoader for ScriptedLoader {
    async fn load(
        &self,
        _conn: &mut PgConnection,
        _recipient: &UserRecord,
        _params: &MailParams,
        _now: DateTime<Utc>,
    ) -> Result<LoadOutcome, MailError> {
        Ok(self
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or(LoadOutcome::Skip))
    }

    async fn mark_sent(
        &self,
        _conn: &mut PgConnection,
        recipient: UserId,
        _at: DateTime<Utc>,
    ) -> Result<(), MailError> {
        self.marked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(recipient);
        Ok(())
    }
}

struct FailingMailer {
    permanent: bool,
}

#[async_trait]
impl Mailer for FailingMailer {
    async fn send(&self, _message: RenderedMessage) -> Result<(), MailError> {
        if self.permanent {
            Err(MailError::Permanent("rejected".into()))
        } else {
            Err(MailError::Transient("try later".into()))
        }
    }
}

/// Simulates a concurrent `delete_user` job's step 3 racing the `EmailFailed` audit: the
/// recipient row is locked (`FOR NO KEY UPDATE`) by `SendEmailHandler::deliver`'s transaction for
/// the whole mailer call, so a deletion attempted from another connection at that moment
/// can only queue behind that lock — it cannot actually run until the handler drops its
/// transaction, exactly the same as a real second job/process would. `send` queues the
/// delete on a spawned task (polling `pg_locks` to confirm it is genuinely queued, not just
/// spawned, before returning) and returns the permanent failure; the delete then wins the
/// row lock the instant the handler's transaction is dropped, ahead of the handler's own
/// fresh re-lock, because it has been waiting since before that transaction was dropped.
struct DeletingMailer {
    pool: PgPool,
    recipient: UserId,
    delete_task:
        Mutex<Option<tokio::task::JoinHandle<Result<sqlx::postgres::PgQueryResult, sqlx::Error>>>>,
}

impl DeletingMailer {
    fn new(pool: PgPool, recipient: UserId) -> Self {
        Self {
            pool,
            recipient,
            delete_task: Mutex::new(None),
        }
    }

    /// Awaits the queued deletion so the test can assert on its result deterministically.
    async fn join_deletion(&self) {
        let task = self
            .delete_task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            task.await
                .unwrap_or_else(|e| unreachable!("delete task panicked: {e}"))
                .unwrap_or_else(|e| unreachable!("delete: {e}"));
        }
    }
}

#[async_trait]
impl Mailer for DeletingMailer {
    async fn send(&self, _message: RenderedMessage) -> Result<(), MailError> {
        let pool = self.pool.clone();
        let recipient = self.recipient;
        let task = tokio::spawn(async move {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(recipient.as_uuid())
                .execute(&pool)
                .await
        });
        // Wait until the delete is genuinely queued behind the caller's row lock (not
        // merely spawned) before returning, so the race below is deterministic. Scoped to
        // this database's `users` table so a lock wait elsewhere on a shared cluster can't
        // produce a false positive. A row-lock waiter's ungranted lock is on the holder's
        // transaction ID (no database/relation), so match it through the granted `tuple`
        // lock the same backend holds on `users` in this database.
        let mut waited = false;
        for _ in 0..200 {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pg_locks w \
                 WHERE NOT w.granted \
                 AND EXISTS (SELECT 1 FROM pg_locks t \
                     WHERE t.pid = w.pid AND t.granted AND t.locktype = 'tuple' \
                     AND t.database = (SELECT oid FROM pg_database WHERE datname = current_database()) \
                     AND t.relation = 'users'::regclass)",
            )
            .fetch_one(&self.pool)
            .await
            .unwrap_or_else(|e| unreachable!("poll pg_locks: {e}"));
            if waiting > 0 {
                waited = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if !waited {
            unreachable!("timed out waiting for the delete to queue behind the row lock");
        }
        *self
            .delete_task
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(task);
        Err(MailError::Permanent("rejected".into()))
    }
}

async fn user(pool: &PgPool, status: &str, email: Option<&str>, verified: bool) -> UserId {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, email, email_verified, display_name, role, status)
         VALUES ($1, 'https://i.test', $1::text, $2, $3, 'Ada Lovelace', 'member', $4)",
    )
    .bind(id)
    .bind(email)
    .bind(verified)
    .bind(status)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert user: {e}"));
    UserId::from(id)
}

fn outbox(pool: &PgPool) -> MailOutbox {
    MailOutbox::new(JobQueue::new(pool.clone(), Arc::new(SystemIdGenerator)))
}

fn handler(
    pool: &PgPool,
    mailer: Arc<dyn Mailer>,
    loader: Arc<ScriptedLoader>,
) -> SendEmailHandler {
    let mut loaders = MailLoaders::default();
    loaders
        .register(MailKind::UserApproved, loader)
        .unwrap_or_else(|e| unreachable!("register loader: {e}"));
    SendEmailHandler::new(SendEmailDeps {
        pool: pool.clone(),
        ids: Arc::new(SystemIdGenerator),
        mailer,
        loaders,
        outbox: outbox(pool),
        app_url: Url::parse("https://app.postit.test").unwrap_or_else(|e| unreachable!("url: {e}")),
        max_attempts: 3,
    })
}

fn job(recipient: UserId) -> SendEmail {
    SendEmail {
        kind: MailKind::UserApproved,
        recipient: recipient.as_uuid(),
        params: MailParams::None,
    }
}

fn ctx(attempt: u32) -> JobContext {
    JobContext {
        job_id: JobId(Uuid::now_v7()),
        attempt,
        max_attempts: Some(3),
    }
}

async fn audit_kinds(pool: &PgPool, subject: UserId) -> Vec<String> {
    sqlx::query_scalar("SELECT kind FROM audit_events WHERE subject_user_id = $1 ORDER BY at")
        .bind(subject.as_uuid())
        .fetch_all(pool)
        .await
        .unwrap_or_else(|e| unreachable!("audit: {e}"))
}

fn approved() -> LoadOutcome {
    LoadOutcome::Send(MailContent::UserApproved {
        display_name: "Ada Lovelace".into(),
    })
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_job_payload_holds_no_address_name_or_body(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    outbox(&pool)
        .send(
            &mut conn,
            MailKind::UserApproved,
            recipient,
            MailParams::None,
            None,
        )
        .await
        .unwrap_or_else(|e| unreachable!("send: {e}"));

    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM job_outbox WHERE job_type = 'send_email'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("payload: {e}"));
    let text = payload.to_string();
    assert!(!text.contains('@'), "{text}");
    assert!(!text.contains("Ada"), "{text}");
    let keys: Vec<&String> = payload
        .as_object()
        .map(|o| o.keys().collect())
        .unwrap_or_default();
    assert_eq!(keys.len(), 3, "{text}");
    assert_eq!(payload["kind"], "user_approved");
    assert_eq!(payload["recipient"], recipient.as_uuid().to_string());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_active_verified_recipient_gets_the_mail_and_it_is_marked(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    let mailer = MemoryMailer::default();
    let loader = ScriptedLoader::with(approved());

    handler(&pool, Arc::new(mailer.clone()), Arc::clone(&loader))
        .handle(job(recipient), ctx(1))
        .await
        .unwrap_or_else(|e| unreachable!("handle: {e}"));

    let sent = mailer.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "ada@example.com");
    assert!(sent[0].text.contains("Ada Lovelace"));
    assert_eq!(loader.marked(), vec![recipient]);
    assert_eq!(
        audit_kinds(&pool, recipient).await,
        vec!["email_sent".to_string()]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn inactive_or_unverified_recipients_are_dropped(pool: PgPool) {
    for (status, verified) in [
        ("pending", true),
        ("disabled", true),
        ("deleting", true),
        ("active", false),
    ] {
        let recipient = user(&pool, status, Some("ada@example.com"), verified).await;
        let mailer = MemoryMailer::default();
        handler(
            &pool,
            Arc::new(mailer.clone()),
            ScriptedLoader::with(approved()),
        )
        .handle(job(recipient), ctx(1))
        .await
        .unwrap_or_else(|e| unreachable!("handle ({status}, {verified}): {e}"));
        assert!(mailer.sent().is_empty(), "{status} {verified}");
        assert_eq!(
            audit_kinds(&pool, recipient).await,
            vec!["email_dropped".to_string()],
            "{status} {verified}"
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_recipient_without_an_email_is_dropped(pool: PgPool) {
    let recipient = user(&pool, "active", None, true).await;
    let mailer = MemoryMailer::default();
    handler(
        &pool,
        Arc::new(mailer.clone()),
        ScriptedLoader::with(approved()),
    )
    .handle(job(recipient), ctx(1))
    .await
    .unwrap_or_else(|e| unreachable!("handle: {e}"));
    assert!(mailer.sent().is_empty());
    assert_eq!(
        audit_kinds(&pool, recipient).await,
        vec!["email_dropped".to_string()]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_deleted_recipient_is_dropped_without_any_audit_row(pool: PgPool) {
    let gone = UserId::from(Uuid::now_v7());
    let mailer = MemoryMailer::default();
    handler(
        &pool,
        Arc::new(mailer.clone()),
        ScriptedLoader::with(approved()),
    )
    .handle(job(gone), ctx(1))
    .await
    .unwrap_or_else(|e| unreachable!("handle: {e}"));
    assert!(mailer.sent().is_empty());
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE subject_user_id = $1 OR details::text LIKE '%' || $1::text || '%'",
    )
    .bind(gone.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(rows, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_skip_sends_nothing_and_audits_nothing(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    let mailer = MemoryMailer::default();
    handler(
        &pool,
        Arc::new(mailer.clone()),
        ScriptedLoader::with(LoadOutcome::Skip),
    )
    .handle(job(recipient), ctx(1))
    .await
    .unwrap_or_else(|e| unreachable!("handle: {e}"));
    assert!(mailer.sent().is_empty());
    assert!(audit_kinds(&pool, recipient).await.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_defer_re_enqueues_the_same_payload_for_later(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    let later = Utc::now() + chrono::Duration::minutes(10);
    let mailer = MemoryMailer::default();
    handler(
        &pool,
        Arc::new(mailer.clone()),
        ScriptedLoader::with(LoadOutcome::Defer(later)),
    )
    .handle(job(recipient), ctx(1))
    .await
    .unwrap_or_else(|e| unreachable!("handle: {e}"));

    assert!(mailer.sent().is_empty());
    let (payload, run_at): (serde_json::Value, DateTime<Utc>) =
        sqlx::query_as("SELECT payload, run_at FROM job_outbox WHERE job_type = 'send_email'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox: {e}"));
    assert_eq!(payload["recipient"], recipient.as_uuid().to_string());
    assert!((run_at - later).num_milliseconds().abs() < 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_transient_failure_retries_and_the_last_attempt_records_email_failed(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    let loader = ScriptedLoader::with(approved());
    let h = handler(
        &pool,
        Arc::new(FailingMailer { permanent: false }),
        Arc::clone(&loader),
    );

    let first = h.handle(job(recipient), ctx(1)).await;
    assert!(matches!(first, Err(JobError::Retry(_))));
    assert!(audit_kinds(&pool, recipient).await.is_empty());
    assert!(loader.marked().is_empty());

    let last = h.handle(job(recipient), ctx(3)).await;
    assert!(matches!(last, Err(JobError::Fatal(_))));
    assert_eq!(
        audit_kinds(&pool, recipient).await,
        vec!["email_failed".to_string()]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_permanent_failure_gives_up_at_once(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    let h = handler(
        &pool,
        Arc::new(FailingMailer { permanent: true }),
        ScriptedLoader::with(approved()),
    );
    let result = h.handle(job(recipient), ctx(1)).await;
    assert!(matches!(result, Err(JobError::Fatal(_))));
    assert_eq!(
        audit_kinds(&pool, recipient).await,
        vec!["email_failed".to_string()]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_failure_audit_is_skipped_when_the_recipient_is_deleted_mid_send(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    let mailer = Arc::new(DeletingMailer::new(pool.clone(), recipient));
    let h = handler(
        &pool,
        Arc::clone(&mailer) as Arc<dyn Mailer>,
        ScriptedLoader::with(approved()),
    );
    let result = h.handle(job(recipient), ctx(1)).await;
    assert!(matches!(result, Err(JobError::Fatal(_))));
    mailer.join_deletion().await;

    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE subject_user_id = $1 OR details::text LIKE '%' || $1::text || '%'",
    )
    .bind(recipient.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(rows, 0);
}

#[test]
fn registering_two_loaders_for_one_kind_is_an_error() {
    let mut loaders = MailLoaders::default();
    loaders
        .register(
            MailKind::UserApproved,
            ScriptedLoader::with(LoadOutcome::Skip),
        )
        .unwrap_or_else(|e| unreachable!("first: {e}"));
    assert!(
        loaders
            .register(
                MailKind::UserApproved,
                ScriptedLoader::with(LoadOutcome::Skip)
            )
            .is_err()
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_queued_email_is_delivered_by_a_worker(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    let mailer = MemoryMailer::default();
    let mut registry = JobRegistry::default();
    postit_mail::register(
        &mut registry,
        handler(
            &pool,
            Arc::new(mailer.clone()),
            ScriptedLoader::with(approved()),
        ),
    )
    .unwrap_or_else(|e| unreachable!("register: {e}"));
    let worker = RunningWorker::start(pool.clone(), registry).await;

    let mut tx = pool
        .begin()
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));
    outbox(&pool)
        .send(
            &mut tx,
            MailKind::UserApproved,
            recipient,
            MailParams::None,
            None,
        )
        .await
        .unwrap_or_else(|e| unreachable!("send: {e}"));
    tx.commit()
        .await
        .unwrap_or_else(|e| unreachable!("commit: {e}"));

    assert!(wait_until(Duration::from_secs(10), || mailer.sent().len() == 1).await);
    worker.stop().await;
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_defer_never_targets_a_slot_at_or_before_now(pool: PgPool) {
    let recipient = user(&pool, "active", Some("ada@example.com"), true).await;
    // Read the database clock before the handler runs: the handler's own `now` is at or
    // after this, so its floor (`now + 1s`) is at or after `before + 1s`.
    let before: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("now: {e}"));
    let mailer = MemoryMailer::default();
    handler(
        &pool,
        Arc::new(mailer.clone()),
        ScriptedLoader::with(LoadOutcome::Defer(
            Utc::now() - chrono::Duration::seconds(30),
        )),
    )
    .handle(job(recipient), ctx(1))
    .await
    .unwrap_or_else(|e| unreachable!("handle: {e}"));

    assert!(mailer.sent().is_empty());
    let run_at: DateTime<Utc> =
        sqlx::query_scalar("SELECT run_at FROM job_outbox WHERE job_type = 'send_email'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox: {e}"));
    assert!(
        run_at >= before + chrono::Duration::seconds(1),
        "run_at {run_at} must be at least 1s after db now {before}"
    );
}
