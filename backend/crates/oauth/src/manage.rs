//! Listing, renaming, and checking connected accounts.

use emixdb::dto::{Pagination, ResultSet};
use postit_core::{
    AccountId, AccountLabel, AccountNotice, AccountRef, AccountStatus as PlatformStatus,
    AgentError, AuditEventId, ConnectMethod, GrantId, Notice, PlatformId, UserId, Violation,
    ViolationCode,
};
use postit_data::accounts::{Account, AccountRepo, AccountStatus, GrantRepo, GrantStatus};
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::{Capability, OwnerScope};
use postit_notify::{NotifyEvent, Recipients};
use sqlx::{Connection, PgConnection};

use crate::{AccountError, AccountService};

/// The longest display name an account may be given.
const DISPLAY_NAME_MAX: usize = 100;

/// How an account was connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectKind {
    /// With credentials the user pasted.
    Credentials,
    /// With an OAuth sign-in.
    OAuth,
}

/// An account with what the agent says about how it connects.
#[derive(Debug, Clone)]
pub struct AccountView {
    /// The stored account. Never carries the grant's credential or connection config.
    pub account: Account,
    /// `None` when this build has no agent for the platform.
    pub connect: Option<ConnectKind>,
}

impl AccountService {
    fn view(&self, account: Account) -> AccountView {
        let connect = self
            .deps
            .catalog
            .registry()
            .get(&account.platform)
            .map(|agent| match agent.descriptor().connect {
                ConnectMethod::Credentials(_) => ConnectKind::Credentials,
                ConnectMethod::OAuth(_) => ConnectKind::OAuth,
            });
        AccountView { account, connect }
    }

    /// The accounts the scope reaches, newest first. Disconnected ones are hidden unless
    /// `include_disconnected`.
    ///
    /// # Errors
    ///
    /// [`AccountError::Data`] on a database failure.
    pub async fn list(
        &self,
        scope: &OwnerScope,
        platform: Option<&PlatformId>,
        include_disconnected: bool,
        page: Pagination,
    ) -> Result<ResultSet<AccountView>, AccountError> {
        let mut conn = self.deps.pool.acquire().await?;
        let accounts =
            AccountRepo::list(&mut conn, scope, page, platform, include_disconnected).await?;
        Ok(ResultSet {
            data: accounts.data.into_iter().map(|a| self.view(a)).collect(),
            total: accounts.total,
            pagination: accounts.pagination,
        })
    }

    /// One account the scope reaches.
    ///
    /// # Errors
    ///
    /// [`AccountError::NotFound`] for a missing account or one outside the scope's limit.
    pub async fn get(
        &self,
        scope: &OwnerScope,
        id: AccountId,
    ) -> Result<AccountView, AccountError> {
        let mut conn = self.deps.pool.acquire().await?;
        Ok(self.view(AccountRepo::get(&mut conn, scope, id).await?))
    }

    /// Changes the test flag and display name. A `None` leaves a setting as it is.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, [`AccountError::Validation`]
    /// for a blank or over-long name, and [`AccountError::NotFound`].
    pub async fn update(
        &self,
        scope: &OwnerScope,
        id: AccountId,
        test_account: Option<bool>,
        display_name: Option<String>,
    ) -> Result<AccountView, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let display_name = match display_name {
            Some(name) => {
                let trimmed = name.trim();
                let code = if trimmed.is_empty() {
                    Some(ViolationCode::OptionTooShort)
                } else if trimmed.chars().count() > DISPLAY_NAME_MAX {
                    Some(ViolationCode::OptionTooLong)
                } else {
                    None
                };
                if let Some(code) = code {
                    return Err(AccountError::Validation(vec![
                        Violation::new(code).at("display_name"),
                    ]));
                }
                Some(trimmed.to_owned())
            }
            None => None,
        };
        let mut conn = self.deps.pool.acquire().await?;
        let account =
            AccountRepo::update_settings(&mut conn, scope, id, test_account, display_name).await?;
        Ok(self.view(account))
    }

    /// Asks the platform whether the account still works, and records the answer: a good
    /// answer stamps `last_checked_at`; an authentication failure, or an answer reporting any
    /// status but active, moves the account's grant, and every account on it, to
    /// `needs_reconnect`.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the caller is the owner, [`AccountError::NotFound`],
    /// [`AccountError::AccountDisconnected`], the catalog's errors, and
    /// [`AccountError::PlatformUnavailable`] or [`AccountError::RateLimited`] when the
    /// platform cannot answer.
    pub async fn check(
        &self,
        scope: &OwnerScope,
        id: AccountId,
        audit: AuditContext,
    ) -> Result<AccountView, AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let account = self.get(scope, id).await?.account;
        let Some(grant) = account.grant_id else {
            return Err(AccountError::AccountDisconnected);
        };
        if account.status == AccountStatus::Disconnected {
            return Err(AccountError::AccountDisconnected);
        }
        let resolved = self.resolve(&account.platform).await?;
        let ctx = self.agent_context(scope.owner, grant, resolved.config.clone());
        let client = resolved
            .agent
            .create_client(ctx)
            .map_err(AccountError::from_agent)?;
        let answer = client
            .account_info(&AccountRef {
                platform: account.platform.clone(),
                account: id,
                remote_id: account.remote_account_id.clone(),
            })
            .await;
        match answer {
            Ok(info) if info.status == PlatformStatus::Active => {
                let mut conn = self.deps.pool.acquire().await?;
                AccountRepo::mark_checked(&mut conn, scope, id, self.deps.clock.now()).await?;
            }
            // The platform answered but says the connection no longer works.
            Ok(_)
            | Err(AgentError::Authentication { .. } | AgentError::InvalidCredentials { .. }) => {
                self.mark_needs_reconnect(scope, grant, audit).await?;
            }
            Err(other) => return Err(AccountError::from_agent(other)),
        }
        self.get(scope, id).await
    }

    /// Moves a grant and every account on it to `needs_reconnect` and records one
    /// `account_needs_reconnect` event per account. The one entry point for the worker and
    /// the preflight check when a platform rejects a stored credential: system callers pass
    /// `OwnerScope::own(grant_owner)`.
    ///
    /// # Errors
    ///
    /// [`AccountError::Scope`] unless the scope is the owner's, [`AccountError::NotFound`]
    /// for a grant the owner does not have.
    pub async fn mark_needs_reconnect(
        &self,
        scope: &OwnerScope,
        grant: GrantId,
        audit: AuditContext,
    ) -> Result<(), AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let mut conn = self.deps.pool.acquire().await?;
        let mut tx = conn.begin().await?;
        self.mark_needs_reconnect_in(&mut tx, scope, grant, audit)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// [`Self::mark_needs_reconnect`] inside the caller's transaction, so the grant's mark
    /// commits or rolls back with whatever else the caller writes (the worker parks a
    /// delivery in the same transaction).
    ///
    /// # Errors
    ///
    /// The same as [`Self::mark_needs_reconnect`].
    pub async fn mark_needs_reconnect_in(
        &self,
        conn: &mut PgConnection,
        scope: &OwnerScope,
        grant: GrantId,
        audit: AuditContext,
    ) -> Result<(), AccountError> {
        scope.require(Capability::OwnerOnly)?;
        let affected = AccountRepo::ids_on_grant(conn, scope, grant).await?;
        GrantRepo::set_status(conn, scope, grant, GrantStatus::NeedsReconnect).await?;
        let labels = AccountRepo::labels(conn, scope.owner, &affected).await?;
        for account in affected {
            let event = AuditEvent::new(AuditEventKind::AccountNeedsReconnect)
                .actor(scope.actor)
                .owner(scope.owner)
                .context(audit)
                .detail("account_id", account.to_string())
                .map_err(|_| {
                    AccountError::Data(postit_data::DataError::Conflict("audit detail".to_owned()))
                })?;
            AuditLog::record(conn, AuditEventId::from(self.deps.ids.generate()), event).await?;
        }
        // The owner is told; a delegate who triggered a check is not.
        self.notify_owner(conn, scope.owner, labels, |account| {
            Notice::AccountNeedsReconnect(AccountNotice { account })
        })
        .await
    }

    /// Records `notice` for each of the owner's `labels` in `conn`, with no actor, so a
    /// delegate is never told about a connection.
    pub(crate) async fn notify_owner(
        &self,
        conn: &mut PgConnection,
        owner: UserId,
        labels: Vec<AccountLabel>,
        notice: impl Fn(AccountLabel) -> Notice,
    ) -> Result<(), AccountError> {
        for label in labels {
            self.deps
                .notifier
                .record(
                    conn,
                    NotifyEvent {
                        recipients: Recipients::Workspace { owner, actor: None },
                        notice: notice(label),
                    },
                )
                .await?;
        }
        Ok(())
    }
}
