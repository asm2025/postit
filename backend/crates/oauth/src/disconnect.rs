//! Disconnecting an account.

use async_trait::async_trait;
use postit_core::{
    AccountId, AuditEventId, DelegationEvent, DelegationId, DelegationNotice, GrantId, Notice,
    UserId,
};
use postit_data::accounts::{AccountRepo, GrantRepo};
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::publishing::deliveries::{CancelOutcome, CancelResult, DeliveryRepo};
use postit_data::publishing::sweep::Sweep;
use postit_data::users::UsersRepo;
use postit_data::{Capability, DataError, OwnerScope};
use postit_notify::{NotifyEvent, Recipients};
use sqlx::PgConnection;

use crate::{AccountError, AccountService};

/// What the publishing engine did to an account's natively scheduled uploads before the
/// account is disconnected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeCleanup {
    /// Uploads deleted on the platform and cancelled.
    pub deleted: u32,
    /// Uploads the agent could not delete; the platform still holds them.
    pub remaining: u32,
}

/// The publishing engine's part of disconnecting an account. `postit-server` injects it; this
/// crate never depends on the engine.
#[async_trait]
pub trait DisconnectHook: Send + Sync {
    /// Deletes the account's natively scheduled uploads through its agent and cancels them,
    /// while its credentials are still valid. Called with no connection held; it reports the
    /// uploads it could not delete in [`NativeCleanup::remaining`] instead of failing.
    ///
    /// # Errors
    ///
    /// An [`AccountError`] for a failure of its own, such as the database; the disconnect
    /// fails with it before its transaction starts.
    async fn before_disconnect(
        &self,
        owner: UserId,
        account: AccountId,
    ) -> Result<NativeCleanup, AccountError>;

    /// Called in the disconnect transaction with what cancelling the account's deliveries
    /// did, so the engine records its notices in the same transaction. Does nothing by
    /// default.
    ///
    /// # Errors
    ///
    /// An [`AccountError`]; the disconnect rolls back with it.
    async fn deliveries_cancelled(
        &self,
        tx: &mut PgConnection,
        owner: UserId,
        cancelled: &[CancelResult],
    ) -> Result<(), AccountError> {
        let _ = (tx, owner, cancelled);
        Ok(())
    }
}

/// The current display name of `user`.
async fn display_name(conn: &mut PgConnection, user: UserId) -> Result<String, AccountError> {
    Ok(UsersRepo::find_by_id(conn, user)
        .await?
        .ok_or(DataError::NotFound)?
        .display_name)
}

/// What disconnecting an account did to its deliveries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisconnectOutcome {
    /// Pending and parked deliveries and running direct campaigns that were cancelled.
    pub cancelled_deliveries: u32,
    /// Deliveries still held for a later publish time on the platform: those the publishing
    /// engine's [`DisconnectHook`] could not delete, or all of them without a hook.
    pub native_scheduled_remaining: u32,
}

impl AccountService {
    /// Disconnects an account: deletes its natively scheduled uploads through the
    /// [`DisconnectHook`] when one is set, cancels its pending and parked deliveries and stops
    /// its running direct campaigns, drops its links to the app's data, keeps the row and its
    /// delivery history, and revokes and deletes the credential when no other account uses it.
    ///
    /// A delivery being published refuses the disconnect before the platform is touched; the
    /// transaction checks again, so only a claim landing in between can follow the hook's
    /// deletes with a refusal.
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
        {
            let mut conn = self.deps.pool.acquire().await?;
            if DeliveryRepo::account_has_in_flight(&mut conn, id).await? {
                return Err(AccountError::JobActive);
            }
        }
        let hook = self.deps.disconnect_hook.as_deref();
        let cleanup = match hook {
            Some(hook) => Some(hook.before_disconnect(scope.owner, id).await?),
            None => None,
        };

        let mut tx = self.deps.pool.begin().await?;
        // Cancelling locks the pending rows, so no worker can claim one past this point; the
        // in-flight check after it then sees every delivery a worker already holds. Returning
        // early rolls the cancellations back.
        let results =
            DeliveryRepo::cancel_for_account_with_status(&mut tx, id, self.deps.clock.now())
                .await?;
        if DeliveryRepo::account_has_in_flight(&mut tx, id).await? {
            return Err(AccountError::JobActive);
        }
        if let Some(hook) = hook {
            hook.deliveries_cancelled(&mut tx, scope.owner, &results)
                .await?;
        }
        let cancelled = results
            .iter()
            .filter(|result| result.outcome == CancelOutcome::Cancelled)
            .count();
        let native = match cleanup {
            Some(cleanup) => i64::from(cleanup.remaining),
            None => Sweep::native_scheduled_count_for_account(&mut tx, id).await?,
        };
        let unlinked = AccountRepo::disconnect(&mut tx, scope, id, self.deps.clock.now()).await?;
        self.notify_emptied_limits(&mut tx, scope.owner, &unlinked.emptied_limits)
            .await?;
        let event = AuditEvent::new(AuditEventKind::AccountDisconnected)
            .actor(scope.actor)
            .owner(scope.owner)
            .context(audit)
            .detail("account_id", id.to_string())
            .and_then(|e| e.detail("platform", account.platform.as_str()))
            .map_err(|_| AccountError::Data(DataError::Conflict("audit detail".to_owned())))?;
        AuditLog::record(&mut tx, AuditEventId::from(self.deps.ids.generate()), event).await?;
        tx.commit().await?;

        if let (true, Some(grant)) = (unlinked.grant_now_unused, account.grant_id) {
            self.revoke_and_delete(scope, grant).await;
        }
        Ok(DisconnectOutcome {
            cancelled_deliveries: u32::try_from(cancelled).unwrap_or(u32::MAX),
            native_scheduled_remaining: u32::try_from(native).unwrap_or(u32::MAX),
        })
    }

    /// Tells both parties of each limited delegation the disconnect left with no account,
    /// each with the other's display name, in the disconnect transaction.
    async fn notify_emptied_limits(
        &self,
        tx: &mut PgConnection,
        owner: UserId,
        emptied: &[(DelegationId, UserId)],
    ) -> Result<(), AccountError> {
        if emptied.is_empty() {
            return Ok(());
        }
        let owner_name = display_name(tx, owner).await?;
        for &(delegation_id, delegate) in emptied {
            let delegate_name = display_name(tx, delegate).await?;
            for (user, party_name) in [(delegate, owner_name.clone()), (owner, delegate_name)] {
                self.deps
                    .notifier
                    .record(
                        tx,
                        NotifyEvent {
                            recipients: Recipients::User {
                                user,
                                owner: Some(owner),
                            },
                            notice: Notice::DelegationChanged(DelegationNotice {
                                delegation_id,
                                event: DelegationEvent::LimitEmptied,
                                party_name,
                            }),
                        },
                    )
                    .await?;
            }
        }
        Ok(())
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
                // This function holds no connection of its own across the provider call.
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
