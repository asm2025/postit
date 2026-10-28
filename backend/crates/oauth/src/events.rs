//! Platform events: the agent verifies and reads an event the platform sent, and the service
//! applies the deauthorizations it reports.

use std::sync::Arc;

use postit_core::{
    AgentError, AuditEventId, EventContext, EventResponse, InboundEvent, PlatformId,
};
use postit_data::DataError;
use postit_data::accounts::GrantRepo;
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};

use crate::error::agent_error_name;
use crate::{AccountError, AccountService};

impl AccountService {
    /// Hands an event the platform sent to its agent, then applies the deauthorizations the
    /// agent reports: in one transaction, every grant of the platform and each named remote
    /// user, whoever owns it, becomes `revoked` with its credential cleared, its accounts'
    /// tokens are deleted, its accounts that are not `disconnected` become `revoked`, and
    /// `account_revoked_by_platform` is recorded for each such account. Nothing is sent back
    /// to the provider: the platform has already ended the authorization. Returns the agent's
    /// reply for the platform.
    ///
    /// # Errors
    ///
    /// [`AccountError::NotFound`] for an unknown, disabled, or unconfigured platform and for an
    /// event kind the agent does not handle, so the caller cannot tell which platforms exist;
    /// [`AccountError::EventRejected`] when the agent cannot verify the event; another
    /// [`AccountError`] when the agent fails otherwise or the database fails.
    pub async fn handle_platform_event(
        &self,
        platform: &str,
        event: InboundEvent,
    ) -> Result<EventResponse, AccountError> {
        let platform = PlatformId::new(platform).map_err(|_| AccountError::NotFound)?;
        let resolved = match self.resolve(&platform).await {
            Ok(resolved) => resolved,
            Err(
                AccountError::PlatformNotFound
                | AccountError::PlatformDisabled
                | AccountError::PlatformUnavailable
                | AccountError::NotFound,
            ) => return Err(AccountError::NotFound),
            Err(other) => return Err(other),
        };
        let ctx = EventContext {
            config: resolved.config,
            http: self.deps.http.clone(),
            clock: Arc::clone(&self.deps.clock),
        };
        let outcome = match resolved.agent.handle_event(&ctx, event).await {
            Ok(outcome) => outcome,
            Err(AgentError::Unsupported { .. }) => return Err(AccountError::NotFound),
            Err(AgentError::Authentication { .. }) => {
                tracing::info!(%platform, "a platform event failed verification");
                return Err(AccountError::EventRejected);
            }
            Err(other) => {
                tracing::warn!(%platform, error = agent_error_name(&other), "handling a platform event failed");
                return Err(AccountError::from_agent(other));
            }
        };

        if !outcome.deauthorized.is_empty() {
            let mut tx = self.deps.pool.begin().await?;
            for remote in &outcome.deauthorized {
                for (owner, account) in
                    GrantRepo::revoke_by_remote_user(&mut tx, &platform, remote).await?
                {
                    let event = AuditEvent::new(AuditEventKind::AccountRevokedByPlatform)
                        .owner(owner)
                        .detail("account_id", account.to_string())
                        .and_then(|e| e.detail("platform", platform.as_str()))
                        .map_err(|_| {
                            AccountError::Data(DataError::Conflict("audit detail".to_owned()))
                        })?;
                    AuditLog::record(&mut tx, AuditEventId::from(self.deps.ids.generate()), event)
                        .await?;
                }
            }
            tx.commit().await?;
            tracing::info!(
                %platform,
                remote_users = outcome.deauthorized.len(),
                "the platform deauthorized grants"
            );
        }
        Ok(outcome.response)
    }
}
