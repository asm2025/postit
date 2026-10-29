//! Delegation: owners grant other users access to their workspace, delegates leave grants, and
//! requests acting as another user resolve to a scope.
//!
//! Every owner-side operation takes the caller's [`OwnerScope`] and requires
//! [`Capability::OwnerOnly`] first, so a delegate can never grant, change, revoke, or list
//! grants (no re-delegation). Liveness is decided by the injected [`Clock`], never the
//! database's `now()`.

use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{
    AccountId, AuditEventId, Clock, DelegationEvent, DelegationId, DelegationNotice, IdGenerator,
    Notice, UserId,
};
use postit_data::audit::{AuditContext, AuditError, AuditEvent, AuditEventKind, AuditLog};
use postit_data::delegations::{
    Delegation, DelegationChange, DelegationRepo, DelegationView, EndedReason, NewDelegation,
};
use postit_data::users::{UserStatus, UsersRepo};
use postit_data::{AccountLimit, Capability, DataError, Level, OwnerScope, ScopeError};
use postit_jobs::{Job, Queue};
use postit_notify::{Notifier, NotifyError, NotifyEvent, Recipients};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{PgConnection, PgPool};

use crate::error::IdentityError;

/// How many expired grants [`expire_delegations`] ends per transaction.
const EXPIRY_BATCH: u16 = 200;

/// Why a delegation operation failed.
#[derive(Debug, thiserror::Error)]
pub enum DelegationError {
    /// No such grant for this caller, or it has ended or expired.
    #[error("delegation not found")]
    NotFound,
    /// The pair already has a live grant; change it instead.
    #[error("a live delegation already exists for this user")]
    Exists,
    /// An input field is not acceptable: `delegate_id`, `expires_at`, `account_ids`, or
    /// `email`.
    #[error("invalid {field}")]
    Invalid {
        /// The offending field.
        field: &'static str,
    },
    /// The caller's scope does not allow the operation.
    #[error(transparent)]
    Scope(ScopeError),
    /// A database failure.
    #[error(transparent)]
    Data(DataError),
    /// An audit event could not be built.
    #[error(transparent)]
    Audit(#[from] AuditError),
    /// A notification could not be recorded.
    #[error(transparent)]
    Notify(#[from] NotifyError),
}

impl From<DataError> for DelegationError {
    fn from(error: DataError) -> Self {
        match error {
            DataError::NotFound => Self::NotFound,
            DataError::Scope(scope) => Self::Scope(scope),
            DataError::Conflict(code) => match code.as_str() {
                "account_unavailable" => Self::Invalid {
                    field: "account_ids",
                },
                "delegation_exists" => Self::Exists,
                "self_delegation" => Self::Invalid {
                    field: "delegate_id",
                },
                _ => Self::Data(DataError::Conflict(code)),
            },
            other => Self::Data(other),
        }
    }
}

impl From<ScopeError> for DelegationError {
    fn from(error: ScopeError) -> Self {
        Self::Scope(error)
    }
}

impl From<sqlx::Error> for DelegationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Data(DataError::from(error))
    }
}

/// Which accounts a grant reaches, as the caller asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountsInput {
    /// Every account the owner has.
    All,
    /// Only these of the owner's connected accounts; empty means none.
    Only(Vec<AccountId>),
}

impl From<AccountsInput> for AccountLimit {
    fn from(input: AccountsInput) -> Self {
        match input {
            AccountsInput::All => Self::All,
            AccountsInput::Only(ids) => Self::only(ids),
        }
    }
}

/// A new grant in the caller's workspace.
#[derive(Debug, Clone)]
pub struct GrantRequest {
    /// The user to let in.
    pub delegate_id: UserId,
    /// What they may do.
    pub level: Level,
    /// Which accounts they reach.
    pub accounts: AccountsInput,
    /// When it stops working; `None` for never. Must be in the future.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Changes to a grant. `None` leaves a field as it is; `expires_at: Some(None)` clears the
/// expiry.
#[derive(Debug, Clone, Default)]
pub struct ChangeRequest {
    /// A new level.
    pub level: Option<Level>,
    /// A new account limit, replacing the old one.
    pub accounts: Option<AccountsInput>,
    /// A new expiry (in the future), or `Some(None)` for none.
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

/// A workspace a user can act in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// The workspace's owner.
    pub owner_id: UserId,
    /// The owner's display name.
    pub owner_display_name: String,
    /// The grant, level, and account limit the user acts under; `None` for their own.
    pub access: Option<(DelegationId, Level, AccountLimit)>,
}

/// One side of a grant: the user's current name and status.
#[derive(Debug, Clone)]
pub struct DelegationParty {
    /// The user.
    pub id: UserId,
    /// Their display name.
    pub display_name: String,
    /// Their status.
    pub status: UserStatus,
}

impl DelegationParty {
    fn new(record: postit_data::users::UserRecord) -> Self {
        Self {
            id: record.id,
            display_name: record.display_name,
            status: record.status,
        }
    }
}

/// A grant with both parties, read in the transaction that wrote it, so the caller needs no
/// second read that a concurrent user deletion could turn into an error.
#[derive(Debug, Clone)]
pub struct DelegationDetail {
    /// The grant.
    pub delegation: Delegation,
    /// The user whose workspace it opens.
    pub owner: DelegationParty,
    /// The user it lets in.
    pub delegate: DelegationParty,
}

/// Grants, changes, revokes, and leaves delegations, and lists them, with one audit event per
/// change.
#[derive(Clone)]
pub struct DelegationService {
    pool: PgPool,
    ids: Arc<dyn IdGenerator>,
    clock: Arc<dyn Clock>,
    notifier: Notifier,
}

impl DelegationService {
    /// A service over `pool`, taking event IDs from `ids` and the time from `clock`, and
    /// recording the other party's notification through `notifier`.
    #[must_use]
    pub fn new(
        pool: PgPool,
        ids: Arc<dyn IdGenerator>,
        clock: Arc<dyn Clock>,
        notifier: Notifier,
    ) -> Self {
        Self {
            pool,
            ids,
            clock,
            notifier,
        }
    }

    /// Grants `req.delegate_id` access to the scope owner's workspace. A grant for the pair
    /// that has expired but not been ended yet is ended first, with its own
    /// `delegation_expired` event.
    ///
    /// # Errors
    ///
    /// [`DelegationError::Scope`] for a delegated scope; [`DelegationError::Invalid`] for a
    /// delegate that is the owner, unknown, or not active (`delegate_id`), an expiry not in
    /// the future (`expires_at`), or an account that is not the owner's connected account
    /// (`account_ids`); [`DelegationError::Exists`] when the pair has a live grant.
    pub async fn grant(
        &self,
        scope: &OwnerScope,
        req: GrantRequest,
        ctx: AuditContext,
    ) -> Result<DelegationDetail, DelegationError> {
        scope.require(Capability::OwnerOnly)?;
        let now = self.clock.now();
        if req.delegate_id == scope.owner {
            return Err(DelegationError::Invalid {
                field: "delegate_id",
            });
        }
        if req.expires_at.is_some_and(|at| at <= now) {
            return Err(DelegationError::Invalid {
                field: "expires_at",
            });
        }
        let mut tx = self.pool.begin().await?;
        // Key-share locks hold both rows until the commit: a concurrent deletion waits, so
        // the insert cannot hit a foreign-key violation and the names read here stay valid.
        let delegate = match UsersRepo::share_by_id(&mut tx, req.delegate_id).await? {
            Some(user) if user.status == UserStatus::Active => user,
            _ => {
                return Err(DelegationError::Invalid {
                    field: "delegate_id",
                });
            }
        };
        let owner = UsersRepo::share_by_id(&mut tx, scope.owner)
            .await?
            .ok_or(DelegationError::NotFound)?;
        if let Some(existing) =
            DelegationRepo::lock_live_for_pair(&mut tx, scope, req.delegate_id).await?
        {
            if existing.is_live(now) {
                return Err(DelegationError::Exists);
            }
            self.end(&mut tx, &existing, EndedReason::Expired, None, now, ctx)
                .await?;
        }
        let delegation = DelegationRepo::insert(
            &mut tx,
            scope,
            NewDelegation {
                id: DelegationId::from(self.ids.generate()),
                delegate_id: req.delegate_id,
                level: req.level,
                accounts: req.accounts.into(),
                expires_at: req.expires_at,
            },
        )
        .await?;
        let event = with_state(
            AuditEvent::new(AuditEventKind::DelegationGranted)
                .context(ctx)
                .actor(scope.actor),
            &delegation,
        )?;
        self.record(&mut tx, event).await?;
        notify(
            &self.notifier,
            &mut tx,
            &delegation,
            DelegationEvent::Granted,
            Side::Delegate,
            &owner.display_name,
        )
        .await?;
        tx.commit().await?;
        Ok(DelegationDetail {
            delegation,
            owner: DelegationParty::new(owner),
            delegate: DelegationParty::new(delegate),
        })
    }

    /// Changes the level, account limit, or expiry of one of the scope owner's live grants,
    /// with one `delegation_changed` event holding the values before and after. A request
    /// that changes nothing returns the grant as it is and records no event.
    ///
    /// # Errors
    ///
    /// [`DelegationError::Scope`] for a delegated scope; [`DelegationError::NotFound`] for an
    /// unknown, ended, or expired grant; [`DelegationError::Invalid`] for an expiry not in
    /// the future (`expires_at`) or an account that is not the owner's connected account
    /// (`account_ids`).
    pub async fn change(
        &self,
        scope: &OwnerScope,
        id: DelegationId,
        req: ChangeRequest,
        ctx: AuditContext,
    ) -> Result<DelegationDetail, DelegationError> {
        scope.require(Capability::OwnerOnly)?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let before = DelegationRepo::lock(&mut tx, scope, id).await?;
        if !before.is_live(now) {
            return Err(DelegationError::NotFound);
        }
        if req.expires_at.flatten().is_some_and(|at| at <= now) {
            return Err(DelegationError::Invalid {
                field: "expires_at",
            });
        }
        if req.level.is_none() && req.accounts.is_none() && req.expires_at.is_none() {
            // Nothing to change: no write and no audit event.
            return Self::detail(&mut tx, before).await;
        }
        let after = DelegationRepo::update(
            &mut tx,
            scope,
            id,
            DelegationChange {
                level: req.level,
                accounts: req.accounts.map(Into::into),
                expires_at: req.expires_at,
            },
        )
        .await?;
        let event = AuditEvent::new(AuditEventKind::DelegationChanged)
            .context(ctx)
            .actor(scope.actor)
            .owner(after.owner_id)
            .subject(after.delegate_id)
            .detail("delegation_id", after.id.as_uuid().to_string())?
            .detail("before", Value::Object(state(&before)))?
            .detail("after", Value::Object(state(&after)))?;
        self.record(&mut tx, event).await?;
        let detail = Self::detail(&mut tx, after).await?;
        notify(
            &self.notifier,
            &mut tx,
            &detail.delegation,
            DelegationEvent::Changed,
            Side::Delegate,
            &detail.owner.display_name,
        )
        .await?;
        tx.commit().await?;
        Ok(detail)
    }

    /// `delegation` with both parties' current names, read on `conn`.
    async fn detail(
        conn: &mut PgConnection,
        delegation: Delegation,
    ) -> Result<DelegationDetail, DelegationError> {
        let owner = UsersRepo::find_by_id(conn, delegation.owner_id)
            .await?
            .ok_or(DelegationError::NotFound)?;
        let delegate = UsersRepo::find_by_id(conn, delegation.delegate_id)
            .await?
            .ok_or(DelegationError::NotFound)?;
        Ok(DelegationDetail {
            delegation,
            owner: DelegationParty::new(owner),
            delegate: DelegationParty::new(delegate),
        })
    }

    /// Revokes one of the scope owner's grants. Revoking an ended grant does nothing; a grant
    /// that has expired but not been ended yet is ended as expired.
    ///
    /// # Errors
    ///
    /// [`DelegationError::Scope`] for a delegated scope, and [`DelegationError::NotFound`]
    /// when the owner has no such grant.
    pub async fn revoke(
        &self,
        scope: &OwnerScope,
        id: DelegationId,
        ctx: AuditContext,
    ) -> Result<(), DelegationError> {
        scope.require(Capability::OwnerOnly)?;
        let mut tx = self.pool.begin().await?;
        let delegation = DelegationRepo::lock(&mut tx, scope, id).await?;
        self.finish(&mut tx, &delegation, EndedReason::Revoked, scope.actor, ctx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// `delegate` leaves a grant they hold. Leaving an ended grant does nothing; a grant that
    /// has expired but not been ended yet is ended as expired.
    ///
    /// # Errors
    ///
    /// [`DelegationError::NotFound`] when `delegate` holds no such grant.
    pub async fn leave(
        &self,
        delegate: UserId,
        id: DelegationId,
        ctx: AuditContext,
    ) -> Result<(), DelegationError> {
        let mut tx = self.pool.begin().await?;
        let delegation = DelegationRepo::lock_received(&mut tx, delegate, id).await?;
        self.finish(&mut tx, &delegation, EndedReason::Left, delegate, ctx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The scope owner's grants with each delegate's name and status, newest first; ended
    /// and expired grants only when `include_ended`.
    ///
    /// # Errors
    ///
    /// [`DelegationError::Scope`] for a delegated scope.
    pub async fn list_granted(
        &self,
        scope: &OwnerScope,
        include_ended: bool,
        pagination: Pagination,
    ) -> Result<ResultSet<DelegationView>, DelegationError> {
        scope.require(Capability::OwnerOnly)?;
        let mut conn = self.pool.acquire().await?;
        Ok(DelegationRepo::list_granted(
            &mut conn,
            scope,
            include_ended,
            self.clock.now(),
            pagination,
        )
        .await?)
    }

    /// The grants `delegate` holds that have neither ended nor expired, with each owner's
    /// name and status, newest first.
    ///
    /// # Errors
    ///
    /// [`DelegationError::Data`] on a database failure.
    pub async fn list_received(
        &self,
        delegate: UserId,
        pagination: Pagination,
    ) -> Result<ResultSet<DelegationView>, DelegationError> {
        let mut conn = self.pool.acquire().await?;
        Ok(
            DelegationRepo::list_received(&mut conn, delegate, self.clock.now(), pagination)
                .await?,
        )
    }

    /// The workspaces `user` can act in: their own first, then each received grant that has
    /// not ended or expired and whose owner is active, newest grant first.
    ///
    /// # Errors
    ///
    /// [`DelegationError::NotFound`] when `user` does not exist.
    pub async fn workspaces(&self, user: UserId) -> Result<Vec<Workspace>, DelegationError> {
        let now = self.clock.now();
        let mut conn = self.pool.acquire().await?;
        let own = UsersRepo::find_by_id(&mut conn, user)
            .await?
            .ok_or(DelegationError::NotFound)?;
        let mut workspaces = vec![Workspace {
            owner_id: user,
            owner_display_name: own.display_name,
            access: None,
        }];
        // One unpaged query: a delegate's received grants are a small set, and paging with
        // OFFSET outside a transaction could skip or repeat rows when grants change.
        workspaces.extend(
            DelegationRepo::list_all_received(&mut conn, user, now)
                .await?
                .into_iter()
                .filter(|v| v.other_status == UserStatus::Active && v.delegation.is_live(now))
                .map(|v| Workspace {
                    owner_id: v.delegation.owner_id,
                    owner_display_name: v.other_display_name,
                    access: Some((v.delegation.id, v.delegation.level, v.delegation.accounts)),
                }),
        );
        Ok(workspaces)
    }

    /// Finds the one active user with the verified email `email` (trimmed, ignoring case), as
    /// `(id, display_name)`. `None` when no user or more than one matches, or the match is
    /// `caller`.
    ///
    /// # Errors
    ///
    /// [`DelegationError::Invalid`] (`email`) when `email` is blank.
    pub async fn lookup(
        &self,
        caller: UserId,
        email: &str,
    ) -> Result<Option<(UserId, String)>, DelegationError> {
        let email = email.trim();
        if email.is_empty() {
            return Err(DelegationError::Invalid { field: "email" });
        }
        let mut conn = self.pool.acquire().await?;
        let mut found = UsersRepo::find_active_verified_by_email(&mut conn, email).await?;
        if found.len() != 1 {
            return Ok(None);
        }
        Ok(found.pop().filter(|(id, _)| *id != caller))
    }

    /// Ends a locked grant at the clock's time for `reason` on behalf of `actor`: nothing for
    /// an ended grant, `expired` with no actor for one past its expiry.
    async fn finish(
        &self,
        conn: &mut PgConnection,
        delegation: &Delegation,
        reason: EndedReason,
        actor: UserId,
        ctx: AuditContext,
    ) -> Result<(), DelegationError> {
        if delegation.ended_at.is_some() {
            return Ok(());
        }
        let now = self.clock.now();
        if delegation.is_live(now) {
            self.end(conn, delegation, reason, Some(actor), now, ctx)
                .await
        } else {
            self.end(conn, delegation, EndedReason::Expired, None, now, ctx)
                .await
        }
    }

    /// Ends `delegation` for `reason` at `now` and records the matching event, with `actor`
    /// as its actor (`None` for an expiry).
    async fn end(
        &self,
        conn: &mut PgConnection,
        delegation: &Delegation,
        reason: EndedReason,
        actor: Option<UserId>,
        now: DateTime<Utc>,
        ctx: AuditContext,
    ) -> Result<(), DelegationError> {
        let (kind, revoked_by) = match reason {
            EndedReason::Revoked => (AuditEventKind::DelegationRevoked, actor),
            EndedReason::Left => (AuditEventKind::DelegationLeft, None),
            EndedReason::Expired => (AuditEventKind::DelegationExpired, None),
        };
        DelegationRepo::end(conn, delegation.id, reason, revoked_by, now).await?;
        let mut event = AuditEvent::new(kind).context(ctx);
        if let Some(actor) = actor {
            event = event.actor(actor);
        }
        self.record(conn, with_state(event, delegation)?).await?;
        match reason {
            EndedReason::Revoked => {
                let name = display_name(conn, delegation.owner_id).await?;
                notify(
                    &self.notifier,
                    conn,
                    delegation,
                    DelegationEvent::Revoked,
                    Side::Delegate,
                    &name,
                )
                .await?;
                Ok(())
            }
            EndedReason::Left => {
                let name = display_name(conn, delegation.delegate_id).await?;
                notify(
                    &self.notifier,
                    conn,
                    delegation,
                    DelegationEvent::Left,
                    Side::Owner,
                    &name,
                )
                .await?;
                Ok(())
            }
            EndedReason::Expired => notify_expired(&self.notifier, conn, delegation).await,
        }
    }

    async fn record(
        &self,
        conn: &mut PgConnection,
        event: AuditEvent,
    ) -> Result<(), DelegationError> {
        let id = AuditEventId::from(self.ids.generate());
        AuditLog::record(conn, id, event).await?;
        Ok(())
    }
}

/// Which party of a grant a notice is for.
#[derive(Clone, Copy)]
enum Side {
    Owner,
    Delegate,
}

/// The current display name of `user`.
async fn display_name(conn: &mut PgConnection, user: UserId) -> Result<String, DelegationError> {
    Ok(UsersRepo::find_by_id(conn, user)
        .await?
        .ok_or(DelegationError::NotFound)?
        .display_name)
}

/// Records a `delegation_changed` notice about `delegation` for `side`, naming the other
/// party as `party_name`, in the caller's transaction.
async fn notify(
    notifier: &Notifier,
    conn: &mut PgConnection,
    delegation: &Delegation,
    event: DelegationEvent,
    side: Side,
    party_name: &str,
) -> Result<(), NotifyError> {
    let user = match side {
        Side::Owner => delegation.owner_id,
        Side::Delegate => delegation.delegate_id,
    };
    notifier
        .record(
            conn,
            NotifyEvent {
                recipients: Recipients::User {
                    user,
                    owner: Some(delegation.owner_id),
                },
                notice: Notice::DelegationChanged(DelegationNotice {
                    delegation_id: delegation.id,
                    event,
                    party_name: party_name.to_owned(),
                }),
            },
        )
        .await?;
    Ok(())
}

/// Tells both parties of an expired grant, each with the other's name.
async fn notify_expired(
    notifier: &Notifier,
    conn: &mut PgConnection,
    delegation: &Delegation,
) -> Result<(), DelegationError> {
    let owner = display_name(conn, delegation.owner_id).await?;
    let delegate = display_name(conn, delegation.delegate_id).await?;
    notify(
        notifier,
        conn,
        delegation,
        DelegationEvent::Expired,
        Side::Delegate,
        &owner,
    )
    .await?;
    notify(
        notifier,
        conn,
        delegation,
        DelegationEvent::Expired,
        Side::Owner,
        &delegate,
    )
    .await?;
    Ok(())
}

/// The audited state of a grant: level, account limit, and expiry.
fn state(delegation: &Delegation) -> Map<String, Value> {
    let (all_accounts, account_ids) = match &delegation.accounts {
        AccountLimit::All => (true, Vec::new()),
        AccountLimit::Only(ids) => (
            false,
            ids.iter().map(|id| id.as_uuid().to_string()).collect(),
        ),
    };
    let mut fields = Map::new();
    fields.insert("level".into(), delegation.level.as_str().into());
    fields.insert("all_accounts".into(), all_accounts.into());
    fields.insert("account_ids".into(), account_ids.into());
    fields.insert(
        "expires_at".into(),
        delegation
            .expires_at
            .map(|at| at.to_rfc3339_opts(SecondsFormat::AutoSi, true))
            .into(),
    );
    fields
}

/// `event` about `delegation`: its owner and delegate, its ID, and its state.
fn with_state(event: AuditEvent, delegation: &Delegation) -> Result<AuditEvent, AuditError> {
    let mut event = event
        .owner(delegation.owner_id)
        .subject(delegation.delegate_id)
        .detail("delegation_id", delegation.id.as_uuid().to_string())?;
    for (key, value) in state(delegation) {
        event = event.detail(&key, value)?;
    }
    Ok(event)
}

/// A [`DelegationError`] from the expiry job, as the [`IdentityError`] it reports.
fn identity_error(error: DelegationError) -> IdentityError {
    match error {
        DelegationError::Data(error) => IdentityError::Data(error),
        DelegationError::Audit(error) => IdentityError::Audit(error),
        DelegationError::Notify(error) => IdentityError::Notify(error),
        DelegationError::NotFound => IdentityError::Data(DataError::NotFound),
        DelegationError::Scope(error) => IdentityError::Data(DataError::Scope(error)),
        other => IdentityError::Data(DataError::Conflict(other.to_string())),
    }
}

/// Recurring job that ends the delegations whose expiry has passed.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DelegationExpiry {}

impl Job for DelegationExpiry {
    const JOB_TYPE: &'static str = "delegation_expiry";
    const QUEUE: Queue = Queue::Maintenance;
}

/// Ends every delegation whose expiry is at or before `now`, in batches of
/// [`EXPIRY_BATCH`]. Each batch is one transaction that ends the grants and records one
/// `delegation_expired` event per grant with no actor. Returns how many grants it ended.
///
/// # Errors
///
/// [`IdentityError`] on a database failure or an event that cannot be built. A failed batch
/// is rolled back; earlier batches stay ended.
pub async fn expire_delegations(
    pool: &PgPool,
    ids: &dyn IdGenerator,
    notifier: &Notifier,
    now: DateTime<Utc>,
) -> Result<u64, IdentityError> {
    let mut total = 0_u64;
    loop {
        let mut tx = pool.begin().await.map_err(DataError::from)?;
        let batch = DelegationRepo::take_expired(&mut tx, now, i64::from(EXPIRY_BATCH)).await?;
        for delegation in &batch {
            let event = with_state(
                AuditEvent::new(AuditEventKind::DelegationExpired),
                delegation,
            )?;
            AuditLog::record(&mut tx, AuditEventId::from(ids.generate()), event).await?;
            notify_expired(notifier, &mut tx, delegation)
                .await
                .map_err(identity_error)?;
        }
        tx.commit().await.map_err(DataError::from)?;
        total = total.saturating_add(u64::try_from(batch.len()).unwrap_or(u64::MAX));
        if batch.len() < usize::from(EXPIRY_BATCH) {
            return Ok(total);
        }
    }
}

/// The scope `actor` gets in `owner`'s workspace at `now`, or `None` when no live,
/// unexpired grant joins two active users (always `None` when `actor` is `owner`). The one
/// entry point the API uses to turn an act-as request into a scope.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn resolve_act_as(
    pool: &PgPool,
    actor: UserId,
    owner: UserId,
    now: DateTime<Utc>,
) -> Result<Option<OwnerScope>, DataError> {
    let mut conn = pool.acquire().await?;
    DelegationRepo::resolve(&mut conn, actor, owner, now).await
}
