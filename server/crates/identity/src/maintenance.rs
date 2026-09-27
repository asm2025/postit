use std::time::Duration;

use chrono::{DateTime, Utc};
use postit_config::AuditSettings;
use postit_core::IdGenerator;
use postit_data::DataError;
use postit_data::retention::purge_audit_events;
use postit_data::users::{UserStatus, UsersRepo};
use postit_jobs::{Job, JobQueue, Queue};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::IdentityError;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PurgePendingUsers {}

impl Job for PurgePendingUsers {
    const JOB_TYPE: &'static str = "purge_pending_users";
    const QUEUE: Queue = Queue::Maintenance;
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AuditRetention {}

impl Job for AuditRetention {
    const JOB_TYPE: &'static str = "audit_retention";
    const QUEUE: Queue = Queue::Maintenance;
}

const BATCH: i64 = 100;

/// Moves every `pending` user older than `pending_ttl` to `deleting` and enqueues
/// `delete_user` for each, one transaction per user. Returns how many were started.
///
/// # Errors
///
/// Returns [`IdentityError`] on a database or enqueue failure.
pub async fn purge_pending_users(
    pool: &PgPool,
    ids: &dyn IdGenerator,
    jobs: &JobQueue,
    pending_ttl: Duration,
    now: DateTime<Utc>,
) -> Result<u64, IdentityError> {
    let cutoff = now - chrono::Duration::from_std(pending_ttl).unwrap_or(chrono::Duration::MAX);
    let mut started = 0;
    loop {
        let mut conn = pool.acquire().await.map_err(DataError::from)?;
        let batch = UsersRepo::list_pending_older_than(&mut conn, cutoff, BATCH).await?;
        drop(conn);
        if batch.is_empty() {
            return Ok(started);
        }
        for id in batch {
            let mut tx = pool.begin().await.map_err(DataError::from)?;
            let still_pending = UsersRepo::lock_by_id(&mut tx, id)
                .await?
                .is_some_and(|u| u.status == UserStatus::Pending);
            if still_pending {
                crate::deletion::start_deletion(&mut tx, ids, jobs, id, None).await?;
                started += 1;
            }
            tx.commit().await.map_err(DataError::from)?;
        }
    }
}

/// # Errors
///
/// Returns [`IdentityError::Data`] on a database failure.
pub async fn audit_retention(pool: &PgPool, audit: &AuditSettings) -> Result<(), IdentityError> {
    let mut conn = pool.acquire().await.map_err(DataError::from)?;
    let (ip_cleared, deleted) = purge_audit_events(
        &mut conn,
        chrono::Duration::from_std(audit.ip_retention).unwrap_or(chrono::Duration::MAX),
        chrono::Duration::from_std(audit.retention).unwrap_or(chrono::Duration::MAX),
    )
    .await?;
    tracing::info!(ip_cleared, deleted, "audit retention applied");
    Ok(())
}
