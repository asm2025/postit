use std::sync::Arc;

use chrono::TimeDelta;
use postit_core::{Clock, IdGenerator, Notice, NotificationId, NotificationKind, Severity, UserId};
use postit_data::delegations::DelegationRepo;
use postit_data::notifications::{NewNotification, NotificationRepo};
use postit_data::preferences::{UserPreferences, UserPreferencesRepo};
use postit_data::push_endpoints::PushEndpointRepo;
use postit_data::users::UsersRepo;
use postit_jobs::JobQueue;
use postit_mail::{MailKind, MailOutbox, MailParams};
use sqlx::PgConnection;

use crate::error::NotifyError;
use crate::push::PushSend;

/// How long a critical notice waits before its alert email goes out, so notices recorded
/// close together share one email.
pub const ALERT_DELAY: TimeDelta = TimeDelta::minutes(2);

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
    /// With push on, a row whose kind the recipient pushes also enqueues one `push_send` job
    /// per registered device, in the same transaction.
    ///
    /// Each recipient's preferences decide whether their row waits for an email. A critical
    /// row that does also enqueues a `notification_alert` email, due [`ALERT_DELAY`] from now;
    /// the first such job to run sends every critical row still waiting, and later ones find
    /// nothing. There is deliberately no "an alert is already pending" check: it could not see
    /// rows a running job has taken but not yet committed, so a row recorded during a send
    /// would never be emailed.
    ///
    /// # Errors
    ///
    /// [`NotifyError::Data`] on a database failure, [`NotifyError::Mail`] if the email
    /// cannot be enqueued.
    pub async fn record(
        &self,
        conn: &mut PgConnection,
        event: NotifyEvent,
    ) -> Result<Vec<NotificationId>, NotifyError> {
        let NotifyEvent { recipients, notice } = event;
        let targets = resolve(conn, recipients, &notice).await?;
        let now = self.deps.clock.now();
        let mut recorded = Vec::with_capacity(targets.len());
        let kind = notice.kind();
        for (user, owner) in targets {
            let id = NotificationId::from(self.deps.ids.generate());
            let prefs = UserPreferencesRepo::get(conn, user).await?;
            let email_wanted = email_wanted(prefs.as_ref(), kind);
            NotificationRepo::insert(
                conn,
                &NewNotification {
                    id,
                    user,
                    owner,
                    notice: notice.clone(),
                    email_wanted,
                    created_at: now,
                },
            )
            .await?;
            if email_wanted && kind.severity() == Severity::Critical {
                self.deps
                    .outbox
                    .send(
                        conn,
                        MailKind::NotificationAlert,
                        user,
                        MailParams::None,
                        Some(now + ALERT_DELAY),
                    )
                    .await?;
            }
            if self.deps.push_enabled && push_wanted(prefs.as_ref(), kind) {
                for device in PushEndpointRepo::list_for_user(conn, user).await? {
                    let job = PushSend {
                        notification: id.as_uuid(),
                        endpoint: device.id.as_uuid(),
                    };
                    self.deps.queue.enqueue_in(conn, &job, None).await?;
                }
            }
            recorded.push(id);
        }
        Ok(recorded)
    }
}

/// Whether a row of `kind` waits for an email. A critical row does when the master switch and
/// the kind's email switch are on; a `new_comments` row does when the master switch and the
/// daily digest are on; no other row ever does. A user with no preferences row gets the column
/// defaults: master switch on, digest off, no per-kind overrides.
fn email_wanted(prefs: Option<&UserPreferences>, kind: NotificationKind) -> bool {
    let (master, digest) =
        prefs.map_or((true, false), |p| (p.email_notifications, p.comment_digest));
    match kind.severity() {
        Severity::Critical => {
            master
                && prefs.map_or_else(
                    || kind.default_email(),
                    |p| p.notification_switches.email(kind),
                )
        }
        Severity::Info => kind == NotificationKind::NewComments && master && digest,
    }
}

/// Whether a row of `kind` is pushed: the user's per-kind choice, else the kind's default.
fn push_wanted(prefs: Option<&UserPreferences>, kind: NotificationKind) -> bool {
    prefs.map_or_else(
        || kind.default_push(),
        |p| p.notification_switches.push(kind),
    )
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
        Self::for_tests_with(pool, false)
    }

    /// Like [`Self::for_tests`], with push on.
    #[must_use]
    pub fn for_tests_with_push(pool: sqlx::PgPool) -> Self {
        Self::for_tests_with(pool, true)
    }

    fn for_tests_with(pool: sqlx::PgPool, push_enabled: bool) -> Self {
        let ids: Arc<dyn IdGenerator> = Arc::new(postit_core::SystemIdGenerator);
        let clock: Arc<dyn Clock> = Arc::new(postit_core::SystemClock);
        let queue = JobQueue::new(pool, Arc::clone(&ids), Arc::clone(&clock));
        Self::new(NotifierDeps {
            ids,
            clock,
            outbox: MailOutbox::new(queue.clone()),
            queue,
            push_enabled,
        })
    }
}

/// Every notification recorded for `user`, newest first.
///
/// # Panics
///
/// On a database failure; this is a test helper.
#[cfg(feature = "testkit")]
#[allow(clippy::expect_used)]
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
    .expect("notifications_for: listing notifications failed")
    .data
}
