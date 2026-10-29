//! The pre-send checks: what parks a claimed delivery before any platform call.
//!
//! Each reason here is one `Sweep::unpark` checks again, so a parked delivery resumes exactly
//! when its blocker is gone and never parks twice for the same cause.

use postit_core::ParkReason;
use postit_data::accounts::{AccountRepo, AccountStatus};
use postit_data::publishing::ClaimedDelivery;
use postit_data::users::{UserStatus, UsersRepo};
use postit_data::{DataError, OwnerScope};
use postit_oauth::Availability;
use sqlx::PgConnection;

/// Why `claimed` must wait, or `None` when it may go on. In order: the owner is not `active`
/// (`OwnerDisabled`); the account is not `active` (`AccountNeedsReconnect`); the platform's
/// agent is not in this build (`PlatformUnavailable`); an admin switched the platform off
/// (`PlatformDisabled`); a live delivery while the runtime switch `live_switch` is off
/// (`LivePublishDisabled`). Dry runs skip only the last check.
///
/// `available` is the platform's availability. `NotConfigured` parks nothing: the sweep cannot
/// see a missing deployment config, so the worker fails the delivery instead.
///
/// # Errors
///
/// [`DataError`] on a database failure, or when the owner or account is gone.
pub async fn park_reason(
    conn: &mut PgConnection,
    claimed: &ClaimedDelivery,
    live_switch: bool,
    available: Availability,
) -> Result<Option<ParkReason>, DataError> {
    let owner = UsersRepo::find_by_id(conn, claimed.owner_id)
        .await?
        .ok_or(DataError::NotFound)?;
    if owner.status != UserStatus::Active {
        return Ok(Some(ParkReason::OwnerDisabled));
    }
    let account =
        AccountRepo::get(conn, &OwnerScope::own(claimed.owner_id), claimed.account_id).await?;
    if account.status != AccountStatus::Active {
        return Ok(Some(ParkReason::AccountNeedsReconnect));
    }
    match available {
        Availability::NotCompiledIn => return Ok(Some(ParkReason::PlatformUnavailable)),
        Availability::Disabled => return Ok(Some(ParkReason::PlatformDisabled)),
        Availability::Available | Availability::NotConfigured => {}
    }
    if !claimed.dry_run && !live_switch {
        return Ok(Some(ParkReason::LivePublishDisabled));
    }
    Ok(None)
}
