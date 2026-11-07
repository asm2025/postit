//! Resolving a client's schedule input: the zone it defaults to, and what a refusal looks like.

use chrono::{DateTime, Utc};
use postit_core::{ScheduleInput, Violation, resolve_schedule};
use postit_data::DataError;
use postit_data::OwnerScope;
use postit_data::preferences::UserPreferencesRepo;

use crate::error::ApiError;
use crate::state::AppState;

/// The zone a local schedule time defaults to: the acting user's own `user_preferences`
/// timezone (a delegate's, through act-as), or `UTC` when they never saved preferences.
pub(crate) async fn actor_zone(state: &AppState, scope: &OwnerScope) -> Result<String, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(DataError::from)?;
    Ok(UserPreferencesRepo::get(&mut conn, scope.actor)
        .await?
        .map_or_else(|| "UTC".to_owned(), |prefs| prefs.timezone))
}

/// Resolves `input` in the actor's zone. `Ok(Err(violation))` is a schedule the client must
/// fix (a DST gap or overlap, an unknown zone); `Ok(Ok(None))` is "now".
pub(crate) async fn resolve_input(
    state: &AppState,
    scope: &OwnerScope,
    input: Option<&ScheduleInput>,
) -> Result<Result<Option<DateTime<Utc>>, Violation>, ApiError> {
    let Some(input) = input else {
        return Ok(Ok(None));
    };
    let zone = actor_zone(state, scope).await?;
    Ok(resolve_schedule(input, &zone).map(Some))
}
