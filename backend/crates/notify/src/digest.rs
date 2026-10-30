//! The daily comment digest: a recurring job that enqueues one email per user at local hour
//! [`DIGEST_HOUR`], and the loader that builds it.

use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Tz;
use postit_core::{AccountId, CommentsNotice, Notice, UserId};
use postit_data::notifications::NotificationRepo;
use postit_data::preferences::{DigestCandidate, UserPreferencesRepo};
use postit_data::users::UserRecord;
use postit_jobs::{Job, Queue};
use postit_mail::{
    LoadOutcome, MailContent, MailContextLoader, MailError, MailKind, MailOutbox, MailParams,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};

use crate::error::NotifyError;

/// The local hour (0 to 23, in the user's time zone) in which the digest goes out. The cron
/// runs at minute 0 UTC, so the run that falls in this hour is at 08:00 local in a whole-hour
/// zone and at 08:30 or 08:45 in a zone with a 30- or 45-minute offset.
///
/// The hour is checked when the run executes, not when it was scheduled. A run delayed past
/// the end of the local hour skips the users whose hour it was, and they get no digest that
/// day. A zone at +x:45 has the least slack: its hour ends 15 minutes after the cron time, so
/// a run more than 15 minutes late misses it (a +x:30 zone allows 30 minutes, a whole-hour
/// zone 59). The next day's run sends the digest as usual.
pub const DIGEST_HOUR: u32 = 8;

/// Recurring job (`notification_digest`) that enqueues the comment digest for every user
/// whose local hour is [`DIGEST_HOUR`] and who has not had one yet today.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct NotificationDigest {}

impl Job for NotificationDigest {
    const JOB_TYPE: &'static str = "notification_digest";
    const QUEUE: Queue = Queue::Maintenance;
}

/// The time zone named `name`, or UTC when it names none (the preference is validated on
/// write, but a stale or hand-edited value must never fail the run).
fn zone(name: &str) -> Tz {
    name.parse().unwrap_or(Tz::UTC)
}

/// Whether `candidate`'s digest is due at `now`: the local hour is [`DIGEST_HOUR`] and the
/// last digest was before today's local date, or never.
///
/// Comparing local dates instead of computing local midnight keeps this correct across
/// DST changes, including zones whose midnight does not exist on a transition day.
fn is_due(candidate: &DigestCandidate, now: DateTime<Utc>) -> bool {
    let tz = zone(&candidate.timezone);
    let local = now.with_timezone(&tz);
    if local.hour() != DIGEST_HOUR {
        return false;
    }
    candidate
        .last_comment_digest_at
        .is_none_or(|last| last.with_timezone(&tz).date_naive() < local.date_naive())
}

/// Enqueues a `comment_digest` email for each user whose digest is due at `now`, and returns
/// how many. All enqueues commit together, so a failed run enqueues none and is retried whole.
///
/// # Errors
///
/// [`NotifyError`] on a database failure or if an email cannot be enqueued.
pub async fn enqueue_due_digests(
    pool: &PgPool,
    outbox: &MailOutbox,
    now: DateTime<Utc>,
) -> Result<u64, NotifyError> {
    let mut tx = pool.begin().await?;
    let mut enqueued = 0;
    for candidate in UserPreferencesRepo::digest_candidates(&mut tx).await? {
        if is_due(&candidate, now) {
            outbox
                .send(
                    &mut tx,
                    MailKind::CommentDigest,
                    candidate.user_id,
                    MailParams::None,
                    None,
                )
                .await?;
            enqueued += 1;
        }
    }
    tx.commit().await?;
    Ok(enqueued)
}

/// `comment_digest`: takes every `new_comments` row of the recipient still waiting for the
/// digest and lists one line per account with the summed count, under the label of the
/// account's newest row; skips when there are none.
///
/// Like the alert loader, the rows are marked in [`MailContextLoader::load`], inside the
/// handler's transaction, which rolls back when the send fails. A row whose transaction
/// commits after this read is not in the take, so it goes out in the next digest.
/// `mark_sent` only records the time, which gates the once-per-local-day check.
pub struct CommentDigestLoader;

#[async_trait]
impl MailContextLoader for CommentDigestLoader {
    async fn load(
        &self,
        conn: &mut PgConnection,
        recipient: &UserRecord,
        _params: &MailParams,
        now: DateTime<Utc>,
    ) -> Result<LoadOutcome, MailError> {
        let taken = NotificationRepo::take_undigested_comments(conn, recipient.id, now).await?;
        let mut comments: Vec<CommentsNotice> = Vec::new();
        let mut seen: Vec<AccountId> = Vec::new();
        for row in taken {
            let Notice::NewComments(n) = row.notice else {
                continue;
            };
            if let Some(i) = seen.iter().position(|id| *id == n.account.account_id) {
                // The rows come oldest first: the newest row's label is the account's name now.
                if let Some(entry) = comments.get_mut(i) {
                    entry.count = entry.count.saturating_add(n.count);
                    entry.account = n.account;
                }
            } else {
                seen.push(n.account.account_id);
                comments.push(n);
            }
        }
        if comments.is_empty() {
            return Ok(LoadOutcome::Skip);
        }
        Ok(LoadOutcome::Send(MailContent::CommentDigest { comments }))
    }

    async fn mark_sent(
        &self,
        conn: &mut PgConnection,
        recipient: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), MailError> {
        UserPreferencesRepo::set_last_comment_digest_at(conn, recipient, at).await?;
        Ok(())
    }
}
