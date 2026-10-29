//! The publishing engine's part of disconnecting an account, injected into
//! `postit-oauth` as its [`DisconnectHook`].

use async_trait::async_trait;
use postit_core::{AccountId, UserId};
use postit_data::DataError;
use postit_data::publishing::CancelResult;
use postit_oauth::{AccountError, DisconnectHook, NativeCleanup};
use sqlx::PgConnection;

use crate::{PublishingError, PublishingService};

/// A database failure that is not one of the account's own: an internal error, never "not
/// found" or a conflict.
fn internal(message: String) -> AccountError {
    AccountError::Data(DataError::Sql(sqlx::Error::Protocol(message)))
}

/// Deletes an account's natively scheduled uploads through its agent before the account is
/// disconnected, and records `job_partial` for the live jobs the disconnect's cancels first
/// make `partial`.
pub struct PublishingDisconnect {
    service: PublishingService,
}

impl PublishingDisconnect {
    /// The hook over `service`.
    #[must_use]
    pub fn new(service: PublishingService) -> Self {
        Self { service }
    }
}

/// An engine failure as the account service reports it.
fn account_error(error: PublishingError) -> AccountError {
    match error {
        PublishingError::Data(error) => AccountError::Data(error),
        // The account was found before the hook ran; a missing job or delivery here is an
        // inconsistency, not the caller's "account not found".
        other => internal(other.to_string()),
    }
}

#[async_trait]
impl DisconnectHook for PublishingDisconnect {
    async fn before_disconnect(
        &self,
        owner: UserId,
        account: AccountId,
    ) -> Result<NativeCleanup, AccountError> {
        let cleanup = self
            .service
            .delete_natives(owner, Some(account))
            .await
            .map_err(account_error)?
            .cleanup();
        if cleanup.deleted + cleanup.remaining > 0 {
            tracing::info!(
                %account,
                deleted = cleanup.deleted,
                remaining = cleanup.remaining,
                "natively scheduled uploads handled before a disconnect"
            );
        }
        Ok(cleanup)
    }

    async fn deliveries_cancelled(
        &self,
        tx: &mut PgConnection,
        owner: UserId,
        cancelled: &[CancelResult],
    ) -> Result<(), AccountError> {
        self.service
            .record_cancel_partials(tx, owner, cancelled)
            .await
            .map_err(account_error)
    }
}
