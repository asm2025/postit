use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use postit_core::UserId;
use postit_data::preferences::UserPreferencesRepo;
use postit_data::users::{UserRecord, UserRole, UsersRepo};
use postit_mail::{
    LoadOutcome, MailContent, MailContextLoader, MailError, MailKind, MailOutbox, MailParams,
    PendingUser,
};
use sqlx::PgConnection;

use crate::error::IdentityError;

const PENDING_LIST_CAP: i64 = 50;

fn chrono_interval(interval: Duration) -> chrono::Duration {
    chrono::Duration::from_std(interval).unwrap_or(chrono::Duration::MAX)
}

/// Enqueues one `user_pending_approval` email per active admin, in the provisioning
/// transaction, scheduled for the admin's next allowed slot. Duplicate jobs for one admin
/// are expected: [`PendingApprovalLoader`] collapses them.
pub(crate) async fn enqueue_pending_approval_emails(
    conn: &mut PgConnection,
    outbox: &MailOutbox,
    interval: Duration,
    now: DateTime<Utc>,
) -> Result<(), IdentityError> {
    for admin in UsersRepo::list_active_admin_ids(conn).await? {
        let marker = UserPreferencesRepo::get(conn, admin)
            .await?
            .and_then(|p| p.last_approval_email_at);
        let run_at = marker
            .map(|m| m + chrono_interval(interval))
            .filter(|slot| *slot > now);
        outbox
            .send(
                conn,
                MailKind::UserPendingApproval,
                admin,
                MailParams::None,
                run_at,
            )
            .await?;
    }
    Ok(())
}

/// `user_approved`: always sent; no sent marker (a one-off account email, at-least-once).
pub struct ApprovedLoader;

#[async_trait]
impl MailContextLoader for ApprovedLoader {
    async fn load(
        &self,
        _conn: &mut PgConnection,
        recipient: &UserRecord,
        _params: &MailParams,
        _now: DateTime<Utc>,
    ) -> Result<LoadOutcome, MailError> {
        Ok(LoadOutcome::Send(MailContent::UserApproved {
            display_name: recipient.display_name.clone(),
        }))
    }

    async fn mark_sent(
        &self,
        _conn: &mut PgConnection,
        _recipient: UserId,
        _at: DateTime<Utc>,
    ) -> Result<(), MailError> {
        Ok(())
    }
}

/// `user_pending_approval`: every user still `pending` who signed up since this admin's
/// last approval email (or since they became admin), at most one email per interval.
pub struct PendingApprovalLoader {
    interval: Duration,
}

impl PendingApprovalLoader {
    #[must_use]
    pub fn new(interval: Duration) -> Self {
        Self { interval }
    }
}

#[async_trait]
impl MailContextLoader for PendingApprovalLoader {
    async fn load(
        &self,
        conn: &mut PgConnection,
        recipient: &UserRecord,
        _params: &MailParams,
        now: DateTime<Utc>,
    ) -> Result<LoadOutcome, MailError> {
        if recipient.role != UserRole::Admin {
            return Ok(LoadOutcome::Skip);
        }
        let marker = UserPreferencesRepo::get(conn, recipient.id)
            .await?
            .and_then(|p| p.last_approval_email_at);
        let since = marker
            .or(recipient.approved_at)
            .unwrap_or(recipient.created_at);
        let pending = UsersRepo::list_pending_created_after(conn, since, PENDING_LIST_CAP).await?;
        if pending.is_empty() {
            return Ok(LoadOutcome::Skip);
        }
        if let Some(sent) = marker {
            let next_slot = sent + chrono_interval(self.interval);
            if now < next_slot {
                return Ok(LoadOutcome::Defer(next_slot));
            }
        }
        let total = UsersRepo::count_pending_created_after(conn, since).await?;
        let listed = i64::try_from(pending.len()).unwrap_or(i64::MAX);
        Ok(LoadOutcome::Send(MailContent::UserPendingApproval {
            pending: pending
                .into_iter()
                .map(|u| PendingUser {
                    display_name: u.display_name,
                    email: if u.email_verified { u.email } else { None },
                })
                .collect(),
            more: u64::try_from(total - listed).unwrap_or(0),
        }))
    }

    async fn mark_sent(
        &self,
        conn: &mut PgConnection,
        recipient: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), MailError> {
        UserPreferencesRepo::set_last_approval_email_at(conn, recipient, at).await?;
        Ok(())
    }
}
