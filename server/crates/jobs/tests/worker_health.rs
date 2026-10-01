use std::time::Duration;

use postit_jobs::testkit::{jobs_settings, wait_until};
use postit_jobs::{JobRegistry, Worker, migrate};
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn worker_is_ready_while_running_and_a_clean_shutdown_is_not_a_death(pool: PgPool) {
    migrate(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("migrate: {e}"));
    let worker = Worker::new(pool.clone(), &jobs_settings(), JobRegistry::default());
    let health = worker.health();
    assert!(!health.is_ready());

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(worker.run(async move {
        let _ = stop_rx.await;
    }));
    let probe = health.clone();
    assert!(wait_until(Duration::from_secs(10), move || probe.is_ready()).await);

    let _ = stop_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(30), handle).await;
    // A clean shutdown must not be reported as a dead worker.
    assert!(health.is_ready());
}
