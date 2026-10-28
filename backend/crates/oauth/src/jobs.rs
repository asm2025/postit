//! The OAuth jobs: `refresh_credentials` keeps OAuth grants fresh and ends grants whose
//! `reconnect_by` has passed; `oauth_state_purge` revokes the tokens of discarded results and
//! deletes expired states and pending connections.

use chrono::{DateTime, Utc};
use postit_config::JobSchedules;
use postit_core::GrantId;
use postit_data::accounts::{GrantRepo, GrantStatus};
use postit_data::oauth_states::{OAuthStateRepo, RemoteUserExcept};
use postit_jobs::{
    ConsoleCatalog, ConsoleSpec, Job, JobContext, JobError, JobRegistry, JobsError, Queue,
};
use serde::{Deserialize, Serialize};

use crate::AccountService;
use crate::error::agent_error_name;
use crate::refresh::REFRESH_WINDOW;

/// How many due grants one page of the refresh run reads.
pub const REFRESH_BATCH: i64 = 100;

/// The recurring job that refreshes OAuth grants about to expire.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RefreshCredentials {}

impl Job for RefreshCredentials {
    const JOB_TYPE: &'static str = "refresh_credentials";
    const QUEUE: Queue = Queue::Maintenance;
}

/// The recurring job that revokes discarded OAuth results and deletes expired states.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OAuthStatePurge {}

impl Job for OAuthStatePurge {
    const JOB_TYPE: &'static str = "oauth_state_purge";
    const QUEUE: Queue = Queue::Maintenance;
}

/// What one refresh run did. Counts only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefreshReport {
    /// Due grants visited, each once.
    pub visited: u64,
    /// Visits that ended in an error (the grant may now need a reconnect).
    pub failed: u64,
    /// Grants set to `needs_reconnect` because their `reconnect_by` passed.
    pub reconnect_by_passed: u64,
}

/// What one purge run did. Counts only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// Discarded results whose tokens were sent to the provider for revocation.
    pub revoked: u64,
    /// Discarded results deleted without a revoke: the remote user is unknown or still in
    /// use, the platform no longer resolves, or the tokens did not open.
    pub skipped: u64,
    /// Expired states without tokens and expired pending connections deleted.
    pub purged: u64,
}

fn retry(err: impl std::fmt::Display) -> JobError {
    JobError::Retry(err.to_string())
}

/// Refreshes every active OAuth grant that expires within [`REFRESH_WINDOW`] of `now`, one at
/// a time through [`AccountService::refresh_grant`], then moves grants whose `reconnect_by`
/// has passed to `needs_reconnect`. Pages by grant ID, so a grant that stays due (a provider
/// answering 5xx, a grant without a refresh token) is visited once per run. A failure on one
/// grant is logged by its kind and the run moves on.
///
/// # Errors
///
/// [`JobError::Retry`] when the database fails between grants.
pub async fn run_refresh_credentials(
    service: &AccountService,
    now: DateTime<Utc>,
    batch_size: i64,
) -> Result<RefreshReport, JobError> {
    let pool = &service.deps.pool;
    let mut report = RefreshReport::default();
    let mut after: Option<GrantId> = None;
    loop {
        let mut conn = pool.acquire().await.map_err(retry)?;
        let page = GrantRepo::due_for_refresh(&mut conn, now + REFRESH_WINDOW, after, batch_size)
            .await
            .map_err(retry)?;
        drop(conn);
        for &grant in &page {
            report.visited += 1;
            if let Err(error) = service.refresh_grant(grant, None).await {
                report.failed += 1;
                tracing::warn!(%grant, error = agent_error_name(&error), "refreshing a grant did not succeed");
            }
        }
        let short = i64::try_from(page.len()).unwrap_or(i64::MAX) < batch_size;
        match page.last() {
            Some(&last) if !short => after = Some(last),
            _ => break,
        }
    }

    let mut conn = pool.acquire().await.map_err(retry)?;
    let passed = GrantRepo::reconnect_by_passed(&mut conn, now)
        .await
        .map_err(retry)?;
    drop(conn);
    for grant in passed {
        let mut tx = pool.begin().await.map_err(retry)?;
        let Some((_, row)) = GrantRepo::lock_for_refresh(&mut tx, grant)
            .await
            .map_err(retry)?
        else {
            continue;
        };
        // A grant already waiting for a reconnect gets no second set of audit events.
        if row.status == GrantStatus::Active {
            service
                .mark_reconnect(&mut tx, grant, &row.platform)
                .await
                .map_err(retry)?;
            report.reconnect_by_passed += 1;
        }
        GrantRepo::set_reconnect_warned(&mut tx, grant, "expired")
            .await
            .map_err(retry)?;
        tx.commit().await.map_err(retry)?;
    }
    tracing::info!(
        visited = report.visited,
        failed = report.failed,
        reconnect_by_passed = report.reconnect_by_passed,
        "oauth grants refreshed"
    );
    Ok(report)
}

/// Revokes the tokens of every expired result that holds them, then deletes the remaining
/// expired states and pending connections. Each result is taken, checked, and committed
/// before its provider call, so a database failure is retried instead of dropping the revoke,
/// a crash between the commit and the call loses at most that one revoke, and none is
/// repeated. No database connection is held across a provider call.
///
/// A result is deleted without a revoke when it names no remote user, when the remote user is
/// still in use (an active grant of any owner, or another live pending result), when its
/// platform no longer resolves (unknown, disabled, or unconfigured), or when its tokens do not
/// open.
///
/// # Errors
///
/// [`JobError::Retry`] on a database failure.
pub async fn run_oauth_state_purge(
    service: &AccountService,
    now: DateTime<Utc>,
) -> Result<PurgeReport, JobError> {
    let pool = &service.deps.pool;
    let mut report = PurgeReport::default();
    loop {
        // The take, the revoke-skip check, and the platform's config share one transaction,
        // committed before the provider call: a database failure rolls the take back and the
        // job retries, so only a crash between the commit and the call can lose a revoke.
        let mut tx = pool.begin().await.map_err(retry)?;
        let Some(row) = OAuthStateRepo::take_expired_result(&mut tx, now)
            .await
            .map_err(retry)?
        else {
            break;
        };
        let plan = service
            .plan_result_revoke(&mut tx, &row, RemoteUserExcept::State(row.id), now)
            .await
            .map_err(retry)?;
        tx.commit().await.map_err(retry)?;
        match plan {
            Some((tokens, config)) => {
                service.revoke_tokens(&row.platform, &config, &tokens).await;
                report.revoked += 1;
            }
            None => report.skipped += 1,
        }
    }

    let mut conn = pool.acquire().await.map_err(retry)?;
    report.purged = OAuthStateRepo::purge_expired(&mut conn, now)
        .await
        .map_err(retry)?;
    tracing::info!(
        revoked = report.revoked,
        skipped = report.skipped,
        purged = report.purged,
        "oauth states purged"
    );
    Ok(report)
}

/// Registers the recurring `refresh_credentials` and `oauth_state_purge` jobs.
///
/// # Errors
///
/// [`JobsError`] when a schedule is invalid or a job type is already registered.
pub fn register(
    registry: &mut JobRegistry,
    service: AccountService,
    schedules: &JobSchedules,
) -> Result<(), JobsError> {
    let refresh = service.clone();
    registry.register_recurring(
        &schedules.refresh_credentials,
        postit_jobs::maintenance::maintenance_retry(),
        move |_: RefreshCredentials, _: JobContext| {
            let service = refresh.clone();
            async move {
                let now = service.deps.clock.now();
                run_refresh_credentials(&service, now, REFRESH_BATCH)
                    .await
                    .map(drop)
            }
        },
    )?;
    registry.register_recurring(
        &schedules.oauth_state_purge,
        postit_jobs::maintenance::maintenance_retry(),
        move |_: OAuthStatePurge, _: JobContext| {
            let service = service.clone();
            async move {
                let now = service.deps.clock.now();
                run_oauth_state_purge(&service, now).await.map(drop)
            }
        },
    )
}

/// Describes the OAuth jobs to the admin console.
pub fn console(catalog: &mut ConsoleCatalog, schedules: &JobSchedules) {
    catalog.add::<RefreshCredentials>(ConsoleSpec::new().recurring(&schedules.refresh_credentials));
    catalog.add::<OAuthStatePurge>(ConsoleSpec::new().recurring(&schedules.oauth_state_purge));
}
