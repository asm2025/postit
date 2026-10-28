//! Disconnecting an account.

use postit_core::{AccountId, AuditEventId, GrantId};
use postit_data::accounts::{AccountRepo, GrantRepo};
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::publishing::deliveries::{CancelOutcome, DeliveryRepo};
use postit_data::publishing::sweep::Sweep;
use postit_data::{Capability, DataError, OwnerScope};

use crate::{AccountError, AccountService};

/// What disconnecting an account did to its deliveries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisconnectOutcome {
    /// Pending and parked deliveries that were cancelled.
    pub cancelled_deliveries: u32,
    /// Deliveries still held for a later publish time on the platform. Deleting them there
    /// is the publishing engine's job.
    pub native_scheduled_remaining: u32,
}

impl AccountService {
    /// Disconnects an account: cancels its pending and parked deliveries, drops its links to
    /// the app's data, keeps the row for history, and revokes and deletes the credential when
    /// no other account uses it.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, [`AccountError::NotFound`],
    /// and [`AccountError::JobActive`] while a delivery of the account is being published.
    pub async fn disconnect(
        &self,
        scope: &OwnerScope,
        id: AccountId,
        audit: AuditContext,
    ) -> Result<DisconnectOutcome, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let account = self.get(scope, id).await?.account;

        let mut tx = self.deps.pool.begin().await?;
        // Cancelling locks the pending rows, so no worker can claim one past this point; the
        // in-flight check after it then sees every delivery a worker already holds. Returning
        // early rolls the cancellations back.
        let cancelled = DeliveryRepo::cancel_for_account(&mut tx, id, self.deps.clock.now())
            .await?
            .into_iter()
            .filter(|(_, outcome)| *outcome == CancelOutcome::Cancelled)
            .count();
        if DeliveryRepo::account_has_in_flight(&mut tx, id).await? {
            return Err(AccountError::JobActive);
        }
        let native = Sweep::native_scheduled_count_for_account(&mut tx, id).await?;
        let cleanup = AccountRepo::disconnect(&mut tx, scope, id).await?;
        let event = AuditEvent::new(AuditEventKind::AccountDisconnected)
            .actor(scope.actor)
            .owner(scope.owner)
            .context(audit)
            .detail("account_id", id.to_string())
            .and_then(|e| e.detail("platform", account.platform.as_str()))
            .map_err(|_| AccountError::Data(DataError::Conflict("audit detail".to_owned())))?;
        AuditLog::record(&mut tx, AuditEventId::from(self.deps.ids.generate()), event).await?;
        tx.commit().await?;

        if let (true, Some(grant)) = (cleanup.grant_now_unused, account.grant_id) {
            self.revoke_and_delete(scope, grant).await;
        }
        Ok(DisconnectOutcome {
            cancelled_deliveries: u32::try_from(cancelled).unwrap_or(u32::MAX),
            native_scheduled_remaining: u32::try_from(native).unwrap_or(u32::MAX),
        })
    }

    /// Best effort: revokes the grant through [`AccountService::revoke_grant`], then deletes
    /// it either way. A failure never undoes the disconnect; the next disconnect or a cleanup
    /// removes a grant left behind by a crash between the commit and here.
    async fn revoke_and_delete(&self, scope: &OwnerScope, grant: GrantId) {
        let mut conn = match self.deps.pool.acquire().await {
            Ok(conn) => conn,
            Err(error) => {
                tracing::warn!(?error, "could not delete the unused grant");
                return;
            }
        };
        match GrantRepo::get(&mut conn, scope, grant).await {
            Ok(row) => {
                // No connection is held across the provider call.
                drop(conn);
                self.revoke_grant(scope.owner, &row).await;
                conn = match self.deps.pool.acquire().await {
                    Ok(conn) => conn,
                    Err(error) => {
                        tracing::warn!(?error, "could not delete the unused grant");
                        return;
                    }
                };
            }
            Err(error) => tracing::warn!(?error, "could not read the unused grant to revoke it"),
        }
        if let Err(error) = GrantRepo::delete_if_unused(&mut conn, scope, grant).await {
            tracing::warn!(?error, "could not delete the unused grant");
        }
    }
}
