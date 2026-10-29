//! The delivery notices: payloads built from the delivery's IDs and its account's labels, and
//! recorded for the workspace in the caller's transaction.

use postit_core::{
    AccountId, AccountLabel, DeliveryId, DeliveryNotice, Notice, ParkReason, PostId, PublishJobId,
    UserId,
};
use postit_data::accounts::AccountRepo;
use postit_data::publishing::{ClaimedDelivery, JobRepo};
use postit_data::{DataError, OwnerScope};
use postit_notify::{Notifier, NotifyError, NotifyEvent, Recipients};
use sqlx::PgConnection;

/// The delivery a notice is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoticeSubject {
    /// The delivery.
    pub delivery: DeliveryId,
    /// Its job.
    pub job: PublishJobId,
    /// Its post.
    pub post: PostId,
    /// Whose workspace it runs in.
    pub owner: UserId,
    /// The target account.
    pub account: AccountId,
}

impl From<&ClaimedDelivery> for NoticeSubject {
    fn from(claimed: &ClaimedDelivery) -> Self {
        Self {
            delivery: claimed.id,
            job: claimed.job_id,
            post: claimed.post_id,
            owner: claimed.owner_id,
            account: claimed.account_id,
        }
    }
}

/// The payload of a delivery notice, with the account's display name and platform read from
/// `connected_accounts` now, so the notice still reads correctly after the account is gone.
/// `park_reason` is unset; parked and stale notices set it.
///
/// # Errors
///
/// [`DataError::NotFound`] when the account is gone, [`DataError::Sql`] on a database failure.
pub async fn delivery_notice(
    conn: &mut PgConnection,
    subject: impl Into<NoticeSubject>,
) -> Result<DeliveryNotice, DataError> {
    let subject = subject.into();
    let account = AccountRepo::get(conn, &OwnerScope::own(subject.owner), subject.account).await?;
    Ok(DeliveryNotice {
        delivery_id: subject.delivery,
        job_id: subject.job,
        post_id: subject.post,
        account: AccountLabel {
            account_id: subject.account,
            account_name: account.display_name,
            platform: account.platform,
        },
        park_reason: None,
    })
}

/// Records the notice `kind` builds for `subject` in `conn`, the caller's transaction, for the
/// owner and the job's creator when a live grant still reaches the account. The caller skips
/// this for dry runs.
pub(crate) async fn record(
    notifier: &Notifier,
    conn: &mut PgConnection,
    subject: NoticeSubject,
    park_reason: Option<ParkReason>,
    kind: fn(DeliveryNotice) -> Notice,
) -> Result<(), NotifyError> {
    let mut notice = delivery_notice(&mut *conn, subject).await?;
    notice.park_reason = park_reason;
    let actor = JobRepo::created_by(&mut *conn, subject.job).await?;
    notifier
        .record(
            conn,
            NotifyEvent {
                recipients: Recipients::Workspace {
                    owner: subject.owner,
                    actor,
                },
                notice: kind(notice),
            },
        )
        .await?;
    Ok(())
}
