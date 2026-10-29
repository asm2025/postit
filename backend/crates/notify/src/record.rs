use std::sync::Arc;

use postit_core::{Clock, IdGenerator, Notice, NotificationId, UserId};
use postit_data::delegations::DelegationRepo;
use postit_data::notifications::{NewNotification, NotificationRepo};
use postit_data::users::UsersRepo;
use postit_jobs::JobQueue;
use postit_mail::MailOutbox;
use sqlx::PgConnection;

use crate::error::NotifyError;

/// Who a notice is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipients {
    /// The workspace owner, plus `actor` when the actor is someone else whose live grant
    /// reaches the notice's account.
    Workspace {
        /// The workspace the notice is about.
        owner: UserId,
        /// Who caused it, when a person did.
        actor: Option<UserId>,
    },
    /// One user. `owner` names the workspace the notice is about, when it is one.
    User {
        /// The recipient.
        user: UserId,
        /// The workspace the notice is about.
        owner: Option<UserId>,
    },
    /// Every active admin; the rows have no owner.
    Admins,
}

/// A notice and who it reaches.
#[derive(Debug, Clone)]
pub struct NotifyEvent {
    /// Who it reaches.
    pub recipients: Recipients,
    /// What happened.
    pub notice: Notice,
}

/// What a [`Notifier`] needs.
#[derive(Clone)]
pub struct NotifierDeps {
    /// Mints notification IDs.
    pub ids: Arc<dyn IdGenerator>,
    /// Stamps `created_at`.
    pub clock: Arc<dyn Clock>,
    /// Enqueues the alert and push jobs.
    pub queue: JobQueue,
    /// Enqueues emails.
    pub outbox: MailOutbox,
    /// Whether web push is configured.
    pub push_enabled: bool,
}

/// Records notifications and starts their delivery. Cheap to clone.
#[derive(Clone)]
pub struct Notifier {
    deps: NotifierDeps,
}

impl Notifier {
    /// A notifier over `deps`.
    #[must_use]
    pub fn new(deps: NotifierDeps) -> Self {
        Self { deps }
    }

    /// Records `event` in `conn`, the caller's transaction, one row per recipient, and returns
    /// the new rows' IDs. Recipients are deduplicated.
    ///
    /// # Errors
    ///
    /// [`NotifyError::Data`] on a database failure.
    pub async fn record(
        &self,
        conn: &mut PgConnection,
        event: NotifyEvent,
    ) -> Result<Vec<NotificationId>, NotifyError> {
        let NotifyEvent { recipients, notice } = event;
        let targets = resolve(conn, recipients, &notice).await?;
        let now = self.deps.clock.now();
        let mut recorded = Vec::with_capacity(targets.len());
        for (user, owner) in targets {
            let id = NotificationId::from(self.deps.ids.generate());
            NotificationRepo::insert(
                conn,
                &NewNotification {
                    id,
                    user,
                    owner,
                    notice: notice.clone(),
                    // Task 17 decides this from the user's preferences.
                    email_wanted: false,
                    created_at: now,
                },
            )
            .await?;
            recorded.push(id);
        }
        Ok(recorded)
    }
}

/// The distinct `(recipient, owner)` pairs for `recipients`, in a stable order.
async fn resolve(
    conn: &mut PgConnection,
    recipients: Recipients,
    notice: &Notice,
) -> Result<Vec<(UserId, Option<UserId>)>, NotifyError> {
    let mut targets: Vec<(UserId, Option<UserId>)> = Vec::new();
    match recipients {
        Recipients::Workspace { owner, actor } => {
            targets.push((owner, Some(owner)));
            if let Some(actor) = actor
                && actor != owner
                && DelegationRepo::live_grant_covers(conn, owner, actor, notice.account()).await?
            {
                targets.push((actor, Some(owner)));
            }
        }
        Recipients::User { user, owner } => targets.push((user, owner)),
        Recipients::Admins => {
            for admin in UsersRepo::list_active_admin_ids(conn).await? {
                targets.push((admin, None));
            }
        }
    }
    let mut seen = Vec::with_capacity(targets.len());
    targets.retain(|(user, _)| {
        let fresh = !seen.contains(user);
        if fresh {
            seen.push(*user);
        }
        fresh
    });
    Ok(targets)
}

#[cfg(feature = "testkit")]
impl Notifier {
    /// A notifier over `pool` with system IDs and clock and push off.
    #[must_use]
    pub fn for_tests(pool: sqlx::PgPool) -> Self {
        let ids: Arc<dyn IdGenerator> = Arc::new(postit_core::SystemIdGenerator);
        let clock: Arc<dyn Clock> = Arc::new(postit_core::SystemClock);
        let queue = JobQueue::new(pool, Arc::clone(&ids), Arc::clone(&clock));
        Self::new(NotifierDeps {
            ids,
            clock,
            outbox: MailOutbox::new(queue.clone()),
            queue,
            push_enabled: false,
        })
    }
}

/// Every notification recorded for `user`, newest first.
///
/// # Panics
///
/// On a database failure; this is a test helper.
#[cfg(feature = "testkit")]
pub async fn notifications_for(
    conn: &mut PgConnection,
    user: UserId,
) -> Vec<postit_data::notifications::NotificationRow> {
    use emixdb::dto::Pagination;
    NotificationRepo::list(
        conn,
        user,
        Pagination {
            page: 1,
            page_size: 1000,
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("notifications_for: {e}"))
    .data
}
