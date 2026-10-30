use std::sync::Arc;
use std::time::Duration;

use postit_core::SystemIdGenerator;
use postit_jobs::maintenance::{self, DataRetention, JobHistoryPurge};
use postit_jobs::testkit::{RunningWorker, jobs_settings, retention_settings};
use postit_jobs::{Job, JobContext, JobQueue, JobRegistry, Queue, RetryPolicy};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Serialize, Deserialize)]
struct Noop {}
impl Job for Noop {
    const JOB_TYPE: &'static str = "noop";
    const QUEUE: Queue = Queue::Default;
}

#[test]
fn registration_adds_both_maintenance_job_types() {
    // sqlx 0.9's pool needs a Tokio context even to build a lazy (not-yet-connecting) pool.
    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|e| unreachable!("runtime: {e}"));
    let _guard = runtime.enter();

    let settings = jobs_settings();
    let pool = PgPool::connect_lazy("postgres://localhost/unused")
        .unwrap_or_else(|e| unreachable!("lazy pool: {e}"));
    let mut registry = JobRegistry::default();
    maintenance::register_job_history_purge(
        &mut registry,
        pool.clone(),
        &settings,
        Arc::new(postit_core::SystemClock),
    )
    .unwrap_or_else(|e| unreachable!("history: {e}"));
    maintenance::register_data_retention(
        &mut registry,
        pool,
        &settings,
        retention_settings(),
        Arc::new(postit_core::SystemClock),
    )
    .unwrap_or_else(|e| unreachable!("retention: {e}"));
    assert_eq!(
        registry.job_types(),
        vec![
            "data_retention".to_string(),
            "job_history_purge".to_string()
        ]
    );
    assert_eq!(JobHistoryPurge::JOB_TYPE, "job_history_purge");
    assert_eq!(DataRetention::JOB_TYPE, "data_retention");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_deletes_only_finished_jobs_past_their_window(pool: PgPool) {
    // Run two jobs to completion.
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, |_: Noop, _: JobContext| async { Ok(()) })
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    let types = registry.registered_types();
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let q = JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    );
    let old = q
        .enqueue(&Noop {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue old: {e}"));
    let recent = q
        .enqueue(&Noop {})
        .await
        .unwrap_or_else(|e| unreachable!("enqueue recent: {e}"));
    let done = || async { postit_jobs::testkit::finished_job_ids(&pool).await };
    let mut finished = Vec::new();
    for _ in 0..100 {
        finished = done().await;
        if finished.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    worker.stop().await;
    assert_eq!(finished.len(), 2);

    // Age one of them past the 7-day succeeded window.
    postit_jobs::testkit::age_finished_job(&pool, old.0, chrono::Duration::days(8)).await;
    // And an old recurring-run row (past the longest window) plus a recent one.
    sqlx::query(
        "INSERT INTO job_recurring_runs (id, name, scheduled_for, manual, created_at)
                 VALUES ($1, 'x', now(), TRUE, now() - interval '40 days')",
    )
    .bind(uuid::Uuid::now_v7())
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert run: {e}"));
    let recent_run = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO job_recurring_runs (id, name, scheduled_for, manual, created_at)
                 VALUES ($1, 'x', now(), TRUE, now() - interval '1 day')",
    )
    .bind(recent_run)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert recent run: {e}"));

    postit_jobs::testkit::run_job_history_purge(
        &pool,
        &jobs_settings(),
        &types,
        chrono::Utc::now(),
    )
    .await;

    let remaining = done().await;
    assert_eq!(remaining, vec![recent.0]);
    let runs: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM job_recurring_runs")
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("runs: {e}"));
    assert_eq!(runs, vec![recent_run]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn data_retention_purges_expired_idempotency_keys(pool: PgPool) {
    let user = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status)
                 VALUES ($1, 'https://i', 's', 'n', 'member', 'active')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("user: {e}"));
    sqlx::query(
        "INSERT INTO idempotency_keys (id, owner_id, actor_id, key, route, request_hash, state, expires_at)
                 VALUES ($1, $2, $2, 'k', 'r', 'h', 'completed', now() - interval '1 hour')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(user)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("key: {e}"));

    postit_jobs::testkit::run_data_retention(&pool, &retention_settings(), chrono::Utc::now())
        .await;

    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM idempotency_keys")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(left, 0);
}

async fn ledger_state(pool: &PgPool, id: uuid::Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("state: {e}"))
}

/// `(outcome, error_kind, error_code)` of the newest attempt.
async fn last_attempt(
    pool: &PgPool,
    id: uuid::Uuid,
) -> (Option<String>, Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT outcome, error_kind, error_code FROM job_attempts
         WHERE job_id = $1 ORDER BY attempt DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| unreachable!("attempt: {e}"))
}

/// A ledger-only row (no outbox row, no storage task): what a job whose try ran unrecorded
/// leaves behind.
async fn ledger_row(pool: &PgPool, state: &str, hours_ago: i32) -> uuid::Uuid {
    let id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO jobs (id, job_type, queue, payload, state, run_at)
         VALUES ($1, 'noop', 'default', '{}', $2, now() - make_interval(hours => $3))",
    )
    .bind(id)
    .bind(state)
    .bind(hours_ago)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("ledger row: {e}"));
    if state == "running" {
        sqlx::query(
            "INSERT INTO job_attempts (job_id, attempt, started_at, worker)
             VALUES ($1, 1, now() - make_interval(hours => $2), 'gone:1')",
        )
        .bind(id)
        .bind(hours_ago)
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("attempt: {e}"));
    }
    id
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "shorthand for expected nullable columns"
)]
fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_kills_a_stranded_running_job_and_spares_a_recent_one(pool: PgPool) {
    postit_jobs::migrate(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("migrate: {e}"));
    let stranded = ledger_row(&pool, "running", 30).await;
    let recent = ledger_row(&pool, "running", 1).await;

    let types = JobRegistry::default().registered_types();
    postit_jobs::testkit::run_job_history_purge(
        &pool,
        &jobs_settings(),
        &types,
        chrono::Utc::now(),
    )
    .await;

    assert_eq!(ledger_state(&pool, stranded).await, "killed");
    assert_eq!(
        last_attempt(&pool, stranded).await,
        (s("interrupted"), s("interrupted"), s("worker_lost"))
    );
    assert_eq!(ledger_state(&pool, recent).await, "running");
    assert_eq!(last_attempt(&pool, recent).await, (None, None, None));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_kills_an_orphaned_queued_job_and_spares_a_recent_one(pool: PgPool) {
    postit_jobs::migrate(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("migrate: {e}"));
    let orphan = ledger_row(&pool, "queued", 30).await;
    let recent = ledger_row(&pool, "queued", 1).await;

    let types = JobRegistry::default().registered_types();
    postit_jobs::testkit::run_job_history_purge(
        &pool,
        &jobs_settings(),
        &types,
        chrono::Utc::now(),
    )
    .await;

    assert_eq!(ledger_state(&pool, orphan).await, "killed");
    assert_eq!(
        last_attempt(&pool, orphan).await,
        (s("interrupted"), s("interrupted"), s("ledger_orphan"))
    );
    assert_eq!(ledger_state(&pool, recent).await, "queued");
}

/// A job the storage still holds (here `Pending`, its workers down for longer than the
/// threshold) is waiting, not orphaned.
#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_spares_an_old_queued_job_the_storage_still_holds(pool: PgPool) {
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, |_: Noop, _: JobContext| async { Ok(()) })
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    let types = registry.registered_types();
    let worker = RunningWorker::start(pool.clone(), registry).await;
    let id = JobQueue::new(
        pool.clone(),
        Arc::new(SystemIdGenerator),
        Arc::new(postit_core::SystemClock),
    )
    .enqueue_at(&Noop {}, chrono::Utc::now() + chrono::Duration::hours(1))
    .await
    .unwrap_or_else(|e| unreachable!("enqueue: {e}"))
    .0;
    let relayed = postit_jobs::testkit::wait_until_async(Duration::from_secs(10), || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM job_outbox WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox: {e}"))
            == 0
    })
    .await;
    worker.stop().await;
    assert!(relayed);
    sqlx::query("UPDATE jobs SET run_at = now() - interval '30 hours' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("age: {e}"));

    postit_jobs::testkit::run_job_history_purge(
        &pool,
        &jobs_settings(),
        &types,
        chrono::Utc::now(),
    )
    .await;

    assert_eq!(ledger_state(&pool, id).await, "queued");
}

/// An outbox row whose type no worker registers, and the ledger job behind it.
async fn outbox_job(pool: &PgPool, job_type: &str, hours_ago: i32) -> uuid::Uuid {
    let id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO jobs (id, job_type, queue, payload, state, run_at)
         VALUES ($1, $2, 'default', '{}', 'queued', now() - make_interval(hours => $3))",
    )
    .bind(id)
    .bind(job_type)
    .bind(hours_ago)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("job: {e}"));
    sqlx::query(
        "INSERT INTO job_outbox (id, job_type, payload, run_at) VALUES ($1, $2, '{}', now())",
    )
    .bind(id)
    .bind(job_type)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("outbox: {e}"));
    id
}

async fn outbox_count(pool: &PgPool, id: uuid::Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM job_outbox WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_kills_an_old_unregistered_outbox_job_and_spares_the_rest(pool: PgPool) {
    postit_jobs::migrate(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("migrate: {e}"));
    let mut registry = JobRegistry::default();
    registry
        .register(RetryPolicy::None, |_: Noop, _: JobContext| async { Ok(()) })
        .unwrap_or_else(|e| unreachable!("register: {e}"));
    let types = registry.registered_types();
    // The cutoff is `unknown_type_max_age` (7 days): the stuck row is past it.
    let stuck = outbox_job(&pool, "removed_type", 8 * 24).await;
    let recent = outbox_job(&pool, "removed_type", 30).await;
    let registered = outbox_job(&pool, "noop", 8 * 24).await;

    postit_jobs::testkit::run_job_history_purge(
        &pool,
        &jobs_settings(),
        &types,
        chrono::Utc::now(),
    )
    .await;

    assert_eq!(ledger_state(&pool, stuck).await, "killed");
    assert_eq!(outbox_count(&pool, stuck).await, 0);
    assert_eq!(
        last_attempt(&pool, stuck).await,
        (s("interrupted"), s("interrupted"), s("type_unregistered"))
    );
    for untouched in [recent, registered] {
        assert_eq!(ledger_state(&pool, untouched).await, "queued");
        assert_eq!(outbox_count(&pool, untouched).await, 1);
    }
}

fn no_types() -> postit_jobs::RegisteredTypes {
    JobRegistry::default().registered_types()
}

async fn ledger_count(pool: &PgPool, id: uuid::Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
}

#[test]
fn console_describes_the_maintenance_jobs() {
    let settings = jobs_settings();
    let mut catalog = postit_jobs::ConsoleCatalog::default();
    maintenance::console(&mut catalog, &settings);
    assert_eq!(
        catalog.recurring(),
        vec![
            ("data_retention", settings.schedules.data_retention.clone()),
            (
                "job_history_purge",
                settings.schedules.job_history_purge.clone()
            ),
        ]
    );
    let shows = |name: &str| {
        catalog
            .get(name)
            .map(postit_jobs::ConsoleSpec::shows_error_message)
    };
    assert_eq!(shows("job_history_purge"), Some(true));
    assert_eq!(shows("data_retention"), Some(false));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_also_deletes_old_ledger_rows(pool: PgPool) {
    postit_jobs::migrate(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("migrate: {e}"));
    let old = uuid::Uuid::now_v7();
    let recent = uuid::Uuid::now_v7();
    for (id, age_days) in [(old, 30_i32), (recent, 0)] {
        sqlx::query(
            "INSERT INTO jobs (id, job_type, queue, payload, state, run_at, finished_at)
             VALUES ($1, 'probe', 'default', '{}', 'succeeded', now(), now() - make_interval(days => $2))",
        )
        .bind(id)
        .bind(age_days)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
    }
    postit_jobs::testkit::run_job_history_purge(
        &pool,
        &jobs_settings(),
        &no_types(),
        chrono::Utc::now(),
    )
    .await;
    assert_eq!(
        (
            ledger_count(&pool, old).await,
            ledger_count(&pool, recent).await
        ),
        (0, 1)
    );
}

async fn insert_worker(pool: &PgPool, id: &str, age_hours: i32) {
    sqlx::query(
        "INSERT INTO apalis.workers (id, worker_type, storage_name, layers, last_seen)
         VALUES ($1, 'postit::default', 'PostgresStorage', '', now() - make_interval(hours => $2))",
    )
    .bind(id)
    .bind(age_hours)
    .execute(pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert worker: {e}"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_deletes_stale_worker_rows(pool: PgPool) {
    postit_jobs::migrate(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("migrate: {e}"));
    insert_worker(&pool, "stale-worker", 48).await;
    insert_worker(&pool, "live-worker", 0).await;
    postit_jobs::testkit::run_job_history_purge(
        &pool,
        &jobs_settings(),
        &no_types(),
        chrono::Utc::now(),
    )
    .await;
    let left: Vec<String> = sqlx::query_scalar("SELECT id FROM apalis.workers ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("workers: {e}"));
    assert_eq!(left, vec!["live-worker".to_string()]);
}

/// `apalis.jobs.lock_by` references `apalis.workers(id)` (`APALIS_NOTES.md` item 10). A stale
/// worker that a task still points at must survive: deleting it would fail the whole purge
/// on the foreign key, and `reenqueue_orphaned` needs the row to rescue that task.
#[sqlx::test(migrations = "../data/migrations")]
async fn history_purge_keeps_a_stale_worker_a_task_still_references(pool: PgPool) {
    postit_jobs::migrate(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("migrate: {e}"));
    insert_worker(&pool, "stale-worker", 48).await;
    // A pending task locked to the stale worker (a worker that died mid-claim).
    sqlx::query(
        "INSERT INTO apalis.jobs (id, job_type, job, status, lock_by, run_at)
         VALUES ('01ARZ3NDEKTSV4RRFFQ69G5FAV', 'postit::default', '\x7b7d'::bytea, 'Pending', 'stale-worker', now())",
    )
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert task: {e}"));

    // Panics (via the testkit) if the purge errors, e.g. on the foreign key.
    postit_jobs::testkit::run_job_history_purge(
        &pool,
        &jobs_settings(),
        &no_types(),
        chrono::Utc::now(),
    )
    .await;

    let left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM apalis.workers WHERE id = 'stale-worker'")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(left, 1, "a referenced worker was deleted");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn data_retention_ignores_a_window_too_large_to_subtract(pool: PgPool) {
    let user = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status) \
         VALUES ($1, 'https://issuer.test', $1::text, 'U', 'member', 'active')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("user: {e}"));
    sqlx::query(
        "INSERT INTO notifications (id, user_id, kind, read_at) \
         VALUES ($1, $2, 'user_pending', now() - interval '10 days')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(user)
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("notification: {e}"));

    let mut windows = retention_settings();
    windows.notifications_read_after = Duration::MAX;
    postit_jobs::testkit::run_data_retention(&pool, &windows, chrono::Utc::now()).await;

    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(
        left, 1,
        "an unrepresentable window must keep the row, not purge it"
    );
}
