//! Startup checks for the first-admin rule (plan 02, `postit-identity`).

use postit_config::{BootstrapSettings, Environment};
use postit_data::DataError;
use postit_data::users::UsersRepo;
use sqlx::PgPool;

use crate::error::IdentityError;

/// Refuses a production start that could never get an admin: no active admin exists and
/// neither `admin_email` nor `admin_subject` is configured. Other environments always pass.
///
/// # Errors
///
/// Returns [`IdentityError::BootstrapRequired`] in that case, or [`IdentityError::Data`] on
/// a database failure.
pub async fn check_startup(
    pool: &PgPool,
    bootstrap: &BootstrapSettings,
    env: Environment,
) -> Result<(), IdentityError> {
    if env != Environment::Production
        || bootstrap.admin_email.is_some()
        || bootstrap.admin_subject.is_some()
    {
        return Ok(());
    }
    // `count_active_admins` takes `FOR UPDATE` row locks, hence the short transaction.
    let mut tx = pool.begin().await.map_err(DataError::from)?;
    let admins = UsersRepo::count_active_admins(&mut tx).await?;
    tx.rollback().await.map_err(DataError::from)?;
    if admins == 0 {
        return Err(IdentityError::BootstrapRequired);
    }
    Ok(())
}
