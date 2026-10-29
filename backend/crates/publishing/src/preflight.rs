//! The `account_preflight` handler: checks, ahead of time, the accounts that are about to be
//! published to, so a revoked credential is found before the delivery comes due.

use chrono::{DateTime, TimeDelta, Utc};
use postit_data::OwnerScope;
use postit_data::audit::AuditContext;
use postit_data::publishing::sweep::Sweep;
use postit_jobs::JobError;

use crate::PublishingService;
use crate::worker::{data, sql};

/// How far ahead the preflight looks for due deliveries, and how recent a good check must be
/// to skip an account.
pub const PREFLIGHT_WINDOW: TimeDelta = TimeDelta::hours(1);

/// The most accounts one run checks; the next run, ten minutes later, takes the rest.
pub const PREFLIGHT_LIMIT: i64 = 500;

/// Runs one `account_preflight`: every `active` account with a live delivery due within
/// [`PREFLIGHT_WINDOW`] and no good check within it is checked with
/// `AccountService::check`, which stamps `last_checked_at` on a good answer and marks the
/// account's grant `needs_reconnect` on a rejected credential or any status but active (the
/// worker then parks the delivery instead of calling the platform). An account the platform
/// cannot answer for now (transport, rate limit, platform unavailable) is skipped until the
/// next run. Returns how many accounts got an answer.
///
/// No database connection is held while a platform is asked: the list is read and released
/// first, and `check` holds none across its call.
///
/// # Errors
///
/// [`JobError::Retry`] when the list cannot be read.
pub async fn run_preflight(service: &PublishingService) -> Result<usize, JobError> {
    let deps = &service.deps;
    let now = deps.clock.now();
    let accounts = {
        let mut conn = deps.pool.acquire().await.map_err(sql)?;
        Sweep::preflight_accounts(
            &mut conn,
            shifted(now, PREFLIGHT_WINDOW),
            shifted(now, -PREFLIGHT_WINDOW),
            PREFLIGHT_LIMIT,
        )
        .await
        .map_err(data)?
    };
    let mut answered = 0;
    for (owner, account) in accounts {
        match deps
            .accounts
            .check(&OwnerScope::own(owner), account, AuditContext::default())
            .await
        {
            Ok(_) => answered += 1,
            Err(_) => {
                tracing::debug!(%account, "the account preflight could not check an account");
            }
        }
    }
    Ok(answered)
}

/// `now + delta`, saturating at the ends of chrono's range.
fn shifted(now: DateTime<Utc>, delta: TimeDelta) -> DateTime<Utc> {
    now.checked_add_signed(delta)
        .unwrap_or(if delta < TimeDelta::zero() {
            DateTime::<Utc>::MIN_UTC
        } else {
            DateTime::<Utc>::MAX_UTC
        })
}
