//! Direct deliveries: an email campaign sent to each recipient of its audience.
//!
//! `publish_delivery` runs the usual checks, then [`start`]s the delivery: a dry run counts the
//! audience and ends `simulated`; a live run fixes the audience in `direct_sends`, moves the
//! delivery to `processing`, and queues the first `direct_send_batch`, all in one transaction.
//! Each [`run_batch`] leases up to `email.batch_size` queued rows within the owner's daily cap,
//! sends them in one call to the agent, and records each result. The delivery is published
//! when no row is left queued.
//!
//! No database connection is held across the agent call, and no address, name, or message text
//! reaches a log, a notice, an audit event, or a job payload.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use postit_core::{
    AccountRef, AgentError, Clock, ContactListId, DeliveryAttemptId, DeliveryEvent, DeliveryId,
    DeliveryState, DirectBatchId, DirectRecipient, DirectSendId, DirectSendOutcome,
    DirectSendResult, GrantId, NotificationKind, ParkReason, Variant,
};
use postit_data::accounts::{AccountRepo, AccountStatus};
use postit_data::limits::{UserLimitDefaults, UserLimitsRepo};
use postit_data::preferences::UserPreferencesRepo;
use postit_data::publishing::direct::{
    Audience, BatchClaim, DirectRecipientRow, DirectRepo, DirectResult, DirectSendRow,
};
use postit_data::publishing::quota::{AccountQuotaKey, EMAIL_CAP_BUCKET, QuotaRepo};
use postit_data::publishing::{
    Applied, AttemptRepo, ClaimedDelivery, Dispatch, JobRepo, Operation, TransitionData,
};
use postit_data::{DataError, OwnerScope};
use postit_jobs::JobError;
use postit_oauth::BoundClient;
use serde_json::{Map, Value, json};
use sqlx::{PgConnection, PgPool};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::backoff;
use crate::outcome::{self, later};
use crate::quota::local_day;
use crate::worker::{self, Unbound, Wrote, data, enqueue, fence, sql};
use crate::{AssembleError, DirectSendBatch, PublishingService, assemble};

/// The variant option naming the contact lists a direct delivery goes to.
pub const AUDIENCE_LISTS: &str = "lists";
/// The variant option naming the tags every recipient must carry.
pub const AUDIENCE_TAGS: &str = "tags";

/// Soft failures a row is sent again after; the next one ends it `failed`.
const MAX_REQUEUES: i16 = 2;

/// Takes the audience out of `variant`'s options, leaving the options the agent knows.
///
/// # Errors
///
/// A `last_error` violation entry (`audience_invalid` with its option's path) when `lists` is
/// not an array of IDs or `tags` not an array of strings.
pub fn take_audience(variant: &mut Variant) -> Result<Audience, Value> {
    let invalid = |key: &str| json!({"code": "audience_invalid", "path": format!("options.{key}")});
    let lists = match variant.options.remove(AUDIENCE_LISTS) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .map(ContactListId::from_uuid)
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid(AUDIENCE_LISTS))?,
        Some(_) => return Err(invalid(AUDIENCE_LISTS)),
    };
    let tags = match variant.options.remove(AUDIENCE_TAGS) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid(AUDIENCE_TAGS))?,
        Some(_) => return Err(invalid(AUDIENCE_TAGS)),
    };
    Ok(Audience { lists, tags })
}

/// Starts a claimed direct delivery that passed its checks. A dry run counts the audience,
/// records the count in its attempt, and ends `simulated`, writing no `direct_sends` row. A
/// live run materializes the audience, moves the delivery to `processing` (`Accepted`), and
/// queues the first batch, in one transaction. A stale fence changes nothing.
pub(crate) async fn start(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    bound: &BoundClient,
    audience: Audience,
) -> Result<(), JobError> {
    let deps = &service.deps;
    if bound.client.direct().is_none() {
        let now = deps.clock.now();
        return worker::fail(service, claimed, worker::capability_unsupported(), &[], now).await;
    }
    let attempt = DeliveryAttemptId::from(deps.ids.generate());
    if !worker::start_remote(service, claimed, attempt, deps.clock.now()).await? {
        return Ok(());
    }
    let in_flight = fence(claimed, DeliveryState::InFlight);
    let mode = if claimed.dry_run { "dry_run" } else { "live" };
    if claimed.dry_run {
        let count = {
            let mut conn = deps.pool.acquire().await.map_err(sql)?;
            DirectRepo::audience_count(&mut conn, claimed.owner_id, &audience)
                .await
                .map_err(data)?
        };
        let now = deps.clock.now();
        let mut tx = deps.pool.begin().await.map_err(sql)?;
        let summary =
            json!({"operation": Operation::Direct.as_str(), "mode": mode, "audience": count});
        AttemptRepo::finish(&mut tx, attempt, "simulated", None, Some(summary), now)
            .await
            .map_err(data)?;
        worker::apply_and_tell(
            service,
            &mut tx,
            claimed,
            in_flight,
            DeliveryEvent::Simulated,
            TransitionData::default(),
            None,
            now,
        )
        .await?;
        return tx.commit().await.map_err(sql);
    }
    let now = deps.clock.now();
    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let size = DirectRepo::materialize(&mut tx, claimed, &audience, now)
        .await
        .map_err(data)?;
    let summary = json!({"operation": Operation::Direct.as_str(), "mode": mode, "audience": size});
    AttemptRepo::finish(&mut tx, attempt, "processing", None, Some(summary), now)
        .await
        .map_err(data)?;
    let wrote = worker::apply_and_tell(
        service,
        &mut tx,
        claimed,
        in_flight,
        DeliveryEvent::Accepted,
        TransitionData::default(),
        None,
        now,
    )
    .await?;
    if matches!(wrote, Wrote::Stale) {
        // Cancelled or taken over since `StartRemote`: the materialized rows roll back too.
        return Ok(());
    }
    enqueue_batch(service, &mut tx, claimed, now).await?;
    tx.commit().await.map_err(sql)
}

/// Runs one `direct_send_batch` job.
///
/// It leaves the delivery alone unless it is a live direct delivery still `processing` under
/// the job's generation, and exits without claiming while the account is not `active` (the
/// sweep queues the batch again once it is). Otherwise it leases up to `email.batch_size`
/// queued rows within the owner's daily cap for the owner's local day, sends them in one call
/// with the batch lease renewed in the background, and records each result in one
/// transaction that also publishes the delivery when nothing is left queued, or queues the
/// next batch. When the cap is used up the delivery waits, still `processing`, until the next
/// local midnight.
///
/// A soft failure (a temporary bounce or a retryable failure) is sent again by a later batch
/// up to twice; the third ends the row `failed`. A whole-batch failure leaves the rows leased
/// for their lease to run out, except a rejected credential, which returns the rows and the cap
/// units, marks the account for a reconnect, and queues nothing.
///
/// # Errors
///
/// [`JobError::Retry`] on a database failure; the sweep queues the batch again.
pub async fn run_batch(service: &PublishingService, job: DirectSendBatch) -> Result<(), JobError> {
    let deps = &service.deps;
    // A stopping server claims no new batch; the due sweep queues it again after the restart.
    if deps.shutdown.is_cancelled() {
        return Ok(());
    }
    let Some(target) = worker::check_target(service, DeliveryId::from_uuid(job.delivery)).await?
    else {
        return Ok(());
    };
    let claimed = target.delivery;
    if target.state != DeliveryState::Processing
        || claimed.dispatch_gen != job.dispatch_gen
        || claimed.operation != Operation::Direct
        || claimed.dry_run
    {
        return Ok(());
    }
    let now = deps.clock.now();
    let sender = match Sender::bind(service, &claimed).await? {
        Ok(sender) => sender,
        Err(blocked) => return blocked.settle(service, &claimed).await,
    };
    let variant = match variant(service, &claimed).await? {
        Ok(variant) => variant,
        Err(blocked) => return blocked.settle(service, &claimed).await,
    };
    let cap = daily_cap(service, &claimed, &sender.bound, now).await?;
    let lease = lease(service)?;
    let claim = {
        let mut conn = deps.pool.acquire().await.map_err(sql)?;
        DirectRepo::claim_batch(
            &mut conn,
            fence(&claimed, DeliveryState::Processing),
            i64::from(deps.email_batch_size),
            now,
            lease,
            Some((&cap.key, cap.limit)),
        )
        .await
        .map_err(data)?
    };
    match claim {
        BatchClaim::Stale => Ok(()),
        BatchClaim::Empty => {
            let mut tx = deps.pool.begin().await.map_err(sql)?;
            finish_or_continue(service, &mut tx, &claimed, None).await?;
            tx.commit().await.map_err(sql)
        }
        BatchClaim::CapReached => {
            let mut tx = deps.pool.begin().await.map_err(sql)?;
            let waiting = DirectRepo::wait_for_cap(
                &mut tx,
                fence(&claimed, DeliveryState::Processing),
                cap.resume_at,
                now,
            )
            .await
            .map_err(data)?;
            if waiting {
                enqueue_batch(service, &mut tx, &claimed, cap.resume_at).await?;
            }
            tx.commit().await.map_err(sql)
        }
        BatchClaim::Rows { batch, rows } => {
            Leased {
                service,
                claimed: &claimed,
                sender: &sender,
                variant: &variant,
                cap: &cap,
                batch,
                rows: &rows,
            }
            .send()
            .await
        }
    }
}

/// Why a batch cannot send now.
enum Blocked {
    /// The account is not usable yet: nothing is claimed, and the sweep queues the batch again
    /// once the account is `active`.
    Wait,
    /// The account's credentials were refused while binding: mark this grant, then wait.
    Reconnect(GrantId),
    /// Waiting cannot help: the delivery fails with this `last_error`.
    Fail(Value),
}

impl Blocked {
    async fn settle(
        self,
        service: &PublishingService,
        claimed: &ClaimedDelivery,
    ) -> Result<(), JobError> {
        match self {
            Self::Wait => Ok(()),
            Self::Reconnect(grant) => {
                let mut tx = service.deps.pool.begin().await.map_err(sql)?;
                worker::mark_grant(service, &mut tx, claimed, grant).await?;
                tx.commit().await.map_err(sql)
            }
            Self::Fail(last_error) => fail(service, claimed, last_error).await,
        }
    }
}

/// Ends a `processing` direct delivery `failed` with `last_error` and records
/// `delivery_failed` (and `job_partial` when the job first becomes partial), in one
/// transaction. The data layer stops its queued rows and stores the counts.
async fn fail(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    last_error: Value,
) -> Result<(), JobError> {
    let now = service.deps.clock.now();
    let mut tx = service.deps.pool.begin().await.map_err(sql)?;
    worker::apply_and_tell(
        service,
        &mut tx,
        claimed,
        fence(claimed, DeliveryState::Processing),
        DeliveryEvent::PermanentFailure,
        TransitionData {
            last_error: Some(last_error),
            ..TransitionData::default()
        },
        Some(NotificationKind::DeliveryFailed),
        now,
    )
    .await?;
    tx.commit().await.map_err(sql)
}

/// The account's bound client, when it can send.
struct Sender {
    bound: BoundClient,
}

impl Sender {
    /// Binds the account's client. The account being gone or not `active`, or its platform
    /// switched off, waits; refused credentials mark the grant and wait; an account whose agent
    /// cannot send direct messages, or one that can never be bound again, fails the delivery.
    async fn bind(
        service: &PublishingService,
        claimed: &ClaimedDelivery,
    ) -> Result<Result<Self, Blocked>, JobError> {
        let deps = &service.deps;
        let account = {
            let mut conn = deps.pool.acquire().await.map_err(sql)?;
            match AccountRepo::get(
                &mut conn,
                &OwnerScope::own(claimed.owner_id),
                claimed.account_id,
            )
            .await
            {
                Ok(account) => account,
                Err(DataError::NotFound) => return Ok(Err(Blocked::Wait)),
                Err(other) => return Err(data(other)),
            }
        };
        if account.status != AccountStatus::Active {
            return Ok(Err(Blocked::Wait));
        }
        let error = match deps
            .accounts
            .bound_client(claimed.owner_id, claimed.account_id)
            .await
        {
            Ok(bound) if bound.client.direct().is_some() => return Ok(Ok(Self { bound })),
            Ok(_) => return Ok(Err(Blocked::Fail(worker::capability_unsupported()))),
            Err(error) => error,
        };
        Ok(Err(match worker::unbound(error) {
            Unbound::Retry(message) => return Err(JobError::Retry(message)),
            Unbound::Park(ParkReason::AccountNeedsReconnect) => match account.grant_id {
                Some(grant) => Blocked::Reconnect(grant),
                None => Blocked::Wait,
            },
            Unbound::Park(_) => Blocked::Wait,
            Unbound::Fail(code) => Blocked::Fail(json!({"code": code, "class": "permanent"})),
        }))
    }
}

/// The variant to send, without its audience options. When it can no longer be built (its
/// media went away since the campaign started) the delivery fails with `revalidation_failed`.
async fn variant(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
) -> Result<Result<Variant, Blocked>, JobError> {
    let snapshot = {
        let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
        JobRepo::snapshot(&mut conn, claimed.job_id)
            .await
            .map_err(data)?
    };
    match assemble(
        &snapshot,
        claimed.account_id,
        &service.deps.media,
        claimed.owner_id,
    )
    .await
    {
        Ok(mut assembled) => {
            // Validated when the campaign started; only the options the agent knows are sent.
            let _ = take_audience(&mut assembled.variant);
            Ok(Ok(assembled.variant))
        }
        Err(AssembleError::Data(error)) => Err(data(error)),
        Err(AssembleError::VariantMissing) => {
            Ok(Err(Blocked::Fail(worker::revalidation_failed(&[
                json!({"code": "variant_missing"}),
            ]))))
        }
        Err(AssembleError::MediaMissing(_)) => {
            Ok(Err(Blocked::Fail(worker::revalidation_failed(&[
                json!({"code": "media_missing"}),
            ]))))
        }
    }
}

/// The owner's daily cap for the local day holding `now`.
struct DailyCap {
    key: AccountQuotaKey,
    /// `0` is unlimited.
    limit: i64,
    /// The next local midnight, when a used-up cap has room again.
    resume_at: DateTime<Utc>,
}

async fn daily_cap(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    bound: &BoundClient,
    now: DateTime<Utc>,
) -> Result<DailyCap, JobError> {
    let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
    let limit = match UserLimitsRepo::get(&mut conn, claimed.owner_id).await {
        Ok(limits) => limits.email_daily_cap,
        // Every provisioned user has a row; without one, the shipped default applies.
        Err(DataError::NotFound) => UserLimitDefaults::default().email_daily_cap,
        Err(other) => return Err(data(other)),
    };
    let zone = UserPreferencesRepo::get(&mut conn, claimed.owner_id)
        .await
        .map_err(data)?
        .map_or_else(|| "UTC".to_owned(), |preferences| preferences.timezone);
    let (today, resume_at) = local_day(&zone, now);
    Ok(DailyCap {
        key: AccountQuotaKey {
            owner: claimed.owner_id,
            account: claimed.account_id,
            platform: bound.descriptor.platform.clone(),
            bucket: EMAIL_CAP_BUCKET.to_owned(),
            period_start: today,
        },
        limit: i64::from(limit),
        resume_at,
    })
}

fn lease(service: &PublishingService) -> Result<TimeDelta, JobError> {
    TimeDelta::from_std(service.deps.settings.lease)
        .map_err(|_| JobError::Fatal("publishing.lease is out of range".to_owned()))
}

/// One leased batch on its way to the agent.
struct Leased<'a> {
    service: &'a PublishingService,
    claimed: &'a ClaimedDelivery,
    sender: &'a Sender,
    variant: &'a Variant,
    cap: &'a DailyCap,
    batch: DirectBatchId,
    rows: &'a [DirectSendRow],
}

/// What becomes of one leased row.
enum Fate {
    /// Recorded with this result, which the provider gave for the row.
    Record(DirectResult),
    /// Recorded with this result without a send (the contact is suppressed or gone). Final,
    /// but no provider call answered it, so it does not count as the provider making progress.
    Unsent(DirectResult),
    /// A soft failure: sent again by a later batch while its tries allow, else recorded
    /// `failed` with this code.
    Soft(&'static str),
}

impl Leased<'_> {
    async fn send(self) -> Result<(), JobError> {
        let deps = &self.service.deps;
        let ids: Vec<DirectSendId> = self.rows.iter().map(|row| row.id).collect();
        let found = {
            let mut conn = deps.pool.acquire().await.map_err(sql)?;
            DirectRepo::recipients(&mut conn, self.claimed.owner_id, &ids)
                .await
                .map_err(data)?
        };
        let found: HashMap<DirectSendId, DirectRecipientRow> =
            found.into_iter().map(|row| (row.id, row)).collect();
        let mut fates: HashMap<DirectSendId, Fate> = HashMap::new();
        let mut recipients = Vec::new();
        for row in self.rows {
            let contact = found.get(&row.id);
            match contact.map(|contact| (contact, contact.email.as_deref())) {
                Some((contact, _)) if contact.suppressed => {
                    fates.insert(row.id, Fate::Unsent(DirectResult::Suppressed));
                }
                Some((contact, Some(address))) => recipients.push(recipient(contact, address)),
                // The contact was deleted since the audience was fixed.
                _ => {
                    fates.insert(
                        row.id,
                        Fate::Unsent(DirectResult::Failed {
                            code: "contact_missing".to_owned(),
                        }),
                    );
                }
            }
        }
        // The rows recorded without a send used no provider capacity; give their units back.
        let unsent = i64::try_from(fates.len()).unwrap_or(i64::MAX);

        let called = if recipients.is_empty() {
            Ok(Vec::new())
        } else {
            let _renewer = BatchRenewer::start(
                deps.pool.clone(),
                self.claimed.id,
                self.batch,
                deps.settings.lease,
                Arc::clone(&deps.clock),
            );
            let account: &AccountRef = &self.sender.bound.account;
            match self.sender.bound.client.direct() {
                Some(direct) => direct.send_batch(account, &recipients, self.variant).await,
                None => Err(AgentError::Unsupported {
                    capability: postit_core::Capability::DirectSender,
                }),
            }
        };
        match called {
            Ok(results) => {
                let mut results: HashMap<DirectSendId, DirectSendResult> = results
                    .into_iter()
                    .map(|result| (result.send_id, result))
                    .collect();
                for recipient in &recipients {
                    let fate = match results.remove(&recipient.send_id) {
                        Some(result) => fate(result.outcome),
                        // The provider said nothing about it: try it again later.
                        None => Fate::Soft("no_result"),
                    };
                    fates.insert(recipient.send_id, fate);
                }
                self.record(fates, unsent).await
            }
            Err(
                error @ (AgentError::Authentication { .. } | AgentError::InvalidCredentials { .. }),
            ) => {
                tracing::warn!(
                    delivery = %self.claimed.id,
                    code = error.code(),
                    class = error.retry_class().as_str(),
                    "the provider refused a direct batch's credentials"
                );
                self.refuse(fates).await
            }
            Err(error) => {
                tracing::warn!(
                    delivery = %self.claimed.id,
                    code = error.code(),
                    class = error.retry_class().as_str(),
                    "a direct batch failed as a whole"
                );
                // Refused for good, or failing for now (a provider answering 503, a timeout):
                // each row counts a soft failure either way, so its tries bound the calls, the
                // next batch waits out the backoff (or the provider's Retry-After), and the
                // delivery finishes with counts once the tries run out. Leaving the rows leased
                // for the sweep instead would call a provider that never recovers every lease
                // plus grace, for good, with no try counted and no owner notice.
                for recipient in &recipients {
                    fates.insert(recipient.send_id, Fate::Soft(error.code()));
                }
                let unsent = i64::try_from(self.rows.len()).unwrap_or(i64::MAX);
                let after = outcome::retry_after_at(&error, self.service.deps.clock.now());
                self.record_after(fates, unsent, after).await
            }
        }
    }

    /// Records every row's fate, gives back the units of rows that were not sent, and then
    /// publishes the delivery or queues the next batch, in one transaction.
    async fn record(
        &self,
        fates: HashMap<DirectSendId, Fate>,
        unsent: i64,
    ) -> Result<(), JobError> {
        self.record_after(fates, unsent, None).await
    }

    /// [`Self::record`], with the provider's `Retry-After` (when it gave one) winning over the
    /// computed wait before the next batch of rows sent back.
    async fn record_after(
        &self,
        mut fates: HashMap<DirectSendId, Fate>,
        unsent: i64,
        retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), JobError> {
        let deps = &self.service.deps;
        let now = deps.clock.now();
        let mut tx = deps.pool.begin().await.map_err(sql)?;
        // Whether a row got its final result, whether the provider gave one (a row recorded
        // without a send, or failed once out of tries, does not count), and the most tries
        // among the rows sent back.
        let mut recorded = false;
        let mut answered = false;
        let mut requeued: Option<i16> = None;
        for row in self.rows {
            let Some(fate) = fates.remove(&row.id) else {
                continue;
            };
            let (result, from_provider) = match fate {
                Fate::Record(result) => (result, true),
                Fate::Unsent(result) => (result, false),
                Fate::Soft(code) => {
                    if row.tries < MAX_REQUEUES
                        && DirectRepo::requeue(&mut tx, row.id, self.batch, now)
                            .await
                            .map_err(data)?
                    {
                        let tries = row.tries.saturating_add(1);
                        requeued = Some(requeued.map_or(tries, |most| most.max(tries)));
                        continue;
                    }
                    // Out of tries, or a cancel stopped the row: the failure replaces it.
                    (
                        DirectResult::Failed {
                            code: code.to_owned(),
                        },
                        false,
                    )
                }
            };
            let done = DirectRepo::record(&mut tx, row.id, self.batch, result, now)
                .await
                .map_err(data)?;
            recorded |= done;
            answered |= done && from_provider;
        }
        // A run that sent rows back with no final result from the provider waits before the
        // next batch (the backoff, or the provider's Retry-After), so the tries of a soft
        // failure are spread out, even when it also recorded rows without a send or out of
        // tries. A run the provider made progress on, or one with nothing sent back, continues
        // at once.
        let next = match requeued {
            Some(tries) if !answered => {
                Some(retry_at.unwrap_or_else(|| later(now, backoff::retry_delay(i32::from(tries)))))
            }
            _ if recorded => Some(now),
            _ => None,
        };
        if self.cap.limit != 0 && unsent > 0 {
            QuotaRepo::release(&mut tx, &self.cap.key, unsent)
                .await
                .map_err(data)?;
        }
        finish_or_continue(self.service, &mut tx, self.claimed, next).await?;
        tx.commit().await.map_err(sql)
    }

    /// The provider refused the credentials, so nothing was sent: the rows go back to the queue
    /// without a try, the batch's cap units are returned, and the account is marked for a
    /// reconnect, in one transaction. No next batch is queued; the sweep queues one once the
    /// account is `active` again.
    ///
    /// Unlike a broadcast delivery, the delivery does not park: it stays `processing` with its
    /// rows `queued`, so `park_grace` and `delivery_stale` never apply to it. The owner already
    /// has the critical `account_needs_reconnect` notice from the grant mark, and the campaign
    /// resumes on reconnect however long that takes (or the owner cancels it). The cost of not
    /// parking is that no `delivery_stale` reminder is recorded while it waits.
    async fn refuse(&self, fates: HashMap<DirectSendId, Fate>) -> Result<(), JobError> {
        let deps = &self.service.deps;
        let now = deps.clock.now();
        let mut tx = deps.pool.begin().await.map_err(sql)?;
        // Rows settled without a send (suppressed, contact gone) keep their result.
        for (id, fate) in fates {
            if let Fate::Unsent(result) = fate {
                DirectRepo::record(&mut tx, id, self.batch, result, now)
                    .await
                    .map_err(data)?;
            }
        }
        DirectRepo::release_batch(&mut tx, self.claimed.id, self.batch)
            .await
            .map_err(data)?;
        if self.cap.limit != 0 {
            let leased = i64::try_from(self.rows.len()).unwrap_or(i64::MAX);
            QuotaRepo::release(&mut tx, &self.cap.key, leased)
                .await
                .map_err(data)?;
        }
        worker::mark_grant(self.service, &mut tx, self.claimed, self.sender.bound.grant).await?;
        tx.commit().await.map_err(sql)
    }
}

/// How one agent result is handled.
fn fate(outcome: DirectSendOutcome) -> Fate {
    match outcome {
        DirectSendOutcome::Sent { remote_id } => Fate::Record(DirectResult::Sent {
            provider_message_id: remote_id,
        }),
        DirectSendOutcome::Bounced {
            permanent: true,
            reason,
        } => Fate::Record(DirectResult::Bounced { code: reason }),
        DirectSendOutcome::Bounced {
            permanent: false, ..
        } => Fate::Soft("soft_bounce"),
        DirectSendOutcome::Failed {
            retryable: true, ..
        } => Fate::Soft("send_failed"),
        DirectSendOutcome::Failed {
            retryable: false, ..
        } => Fate::Record(DirectResult::Failed {
            code: "rejected".to_owned(),
        }),
    }
}

/// The agent's view of one recipient: the address and the mail-merge fields, the contact's
/// own fields first and its names over any field of the same name.
fn recipient(found: &DirectRecipientRow, address: &str) -> DirectRecipient {
    let mut merge_fields = BTreeMap::new();
    if let Value::Object(fields) = &found.fields {
        merge_fields.extend(text_fields(fields));
    }
    if let Some(first) = &found.first_name {
        merge_fields.insert("first_name".to_owned(), first.clone());
    }
    if let Some(last) = &found.last_name {
        merge_fields.insert("last_name".to_owned(), last.clone());
    }
    DirectRecipient {
        send_id: found.id,
        address: address.to_owned(),
        merge_fields,
        locale: found.locale.clone().unwrap_or_else(|| "en".to_owned()),
    }
}

/// Custom fields as text: strings as they are, other values as JSON, nulls left out.
fn text_fields(fields: &Map<String, Value>) -> impl Iterator<Item = (String, String)> + '_ {
    fields.iter().filter_map(|(key, value)| match value {
        Value::Null => None,
        Value::String(text) => Some((key.clone(), text.clone())),
        other => Some((key.clone(), other.to_string())),
    })
}

/// Publishes the delivery when no row is left queued (recording `delivery_published` and, when
/// the job first becomes partial, `job_partial`); otherwise queues the next batch at `next`,
/// when this run changed a row. A delivery that moved on changes nothing.
async fn finish_or_continue(
    service: &PublishingService,
    tx: &mut PgConnection,
    claimed: &ClaimedDelivery,
    next: Option<DateTime<Utc>>,
) -> Result<(), JobError> {
    let now = service.deps.clock.now();
    match DirectRepo::finish_if_done(&mut *tx, fence(claimed, DeliveryState::Processing), now)
        .await
        .map_err(data)?
    {
        Some(Applied::Done { state, job, .. }) => {
            worker::tell(
                service,
                tx,
                claimed,
                Some(NotificationKind::DeliveryPublished),
                state,
                job,
            )
            .await
        }
        // Rows are left: a batch that changed some queues the next. One that changed none
        // (the rows went to another batch) leaves them to the batch that holds them.
        None => match next {
            Some(at) => enqueue_batch(service, tx, claimed, at).await,
            None => Ok(()),
        },
        Some(Applied::Stale) => Ok(()),
    }
}

/// Queues the delivery's next `direct_send_batch` at `at`.
async fn enqueue_batch(
    service: &PublishingService,
    tx: &mut PgConnection,
    claimed: &ClaimedDelivery,
    at: DateTime<Utc>,
) -> Result<(), JobError> {
    let job = DirectSendBatch::from(Dispatch {
        delivery: claimed.id,
        dispatch_gen: claimed.dispatch_gen,
        run_at: at,
    });
    service
        .deps
        .queue
        .enqueue_in(tx, &job, Some(at))
        .await
        .map(|_| ())
        .map_err(enqueue)
}

/// Renews a batch's lease every `lease / 3` while its send runs; stops when dropped.
struct BatchRenewer(JoinHandle<()>);

impl BatchRenewer {
    fn start(
        pool: PgPool,
        delivery: DeliveryId,
        batch: DirectBatchId,
        lease: Duration,
        clock: Arc<dyn Clock>,
    ) -> Self {
        // `interval` refuses a zero period; settings never configure a lease that short.
        let period = (lease / 3).max(Duration::from_millis(10));
        let lease = TimeDelta::from_std(lease).unwrap_or(TimeDelta::MAX);
        let handle = tokio::spawn(async move {
            let mut ticks = tokio::time::interval(period);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // The first tick completes at once; the claim has just set the lease.
            ticks.tick().await;
            loop {
                ticks.tick().await;
                let renewed = match pool.acquire().await {
                    Ok(mut conn) => {
                        DirectRepo::renew_batch(&mut conn, delivery, batch, clock.now(), lease)
                            .await
                    }
                    Err(error) => Err(error.into()),
                };
                match renewed {
                    // The batch lost its rows (recorded, cancelled, or taken over); the send
                    // still records what it did.
                    Ok(0) => return,
                    Ok(_) => {}
                    Err(_) => tracing::warn!(%delivery, "could not renew a direct batch's lease"),
                }
            }
        });
        Self(handle)
    }
}

impl Drop for BatchRenewer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::{AccountId, PlatformId, PostKind};

    use super::*;

    fn variant(options: Value) -> Variant {
        let Value::Object(options) = options else {
            unreachable!()
        };
        Variant {
            target: AccountRef {
                platform: PlatformId::new("mock_direct").unwrap(),
                account: AccountId::from_uuid(Uuid::now_v7()),
                remote_id: "r".to_owned(),
            },
            kind: PostKind::Post,
            text: String::new(),
            media: Vec::new(),
            link: None,
            tags: Vec::new(),
            disclose_ai_generated: false,
            reply_to: None,
            email: None,
            options,
        }
    }

    #[test]
    fn audience_is_taken_out_of_the_options() {
        let list = Uuid::now_v7();
        let mut v = variant(json!({"lists": [list], "tags": ["vip"], "outcome": "published"}));
        let audience = take_audience(&mut v).unwrap();
        assert_eq!(audience.lists, vec![ContactListId::from_uuid(list)]);
        assert_eq!(audience.tags, vec!["vip".to_owned()]);
        assert_eq!(Value::Object(v.options), json!({"outcome": "published"}));
    }

    #[test]
    fn a_malformed_audience_names_its_option() {
        let mut v = variant(json!({"lists": ["not-an-id"]}));
        assert_eq!(
            take_audience(&mut v).unwrap_err(),
            json!({"code": "audience_invalid", "path": "options.lists"})
        );
        let mut v = variant(json!({"tags": "vip"}));
        assert_eq!(
            take_audience(&mut v).unwrap_err(),
            json!({"code": "audience_invalid", "path": "options.tags"})
        );
    }

    #[test]
    fn soft_failures_are_retried_and_hard_ones_recorded() {
        assert!(matches!(
            fate(DirectSendOutcome::Bounced {
                permanent: false,
                reason: String::new()
            }),
            Fate::Soft("soft_bounce")
        ));
        assert!(matches!(
            fate(DirectSendOutcome::Failed {
                retryable: true,
                reason: String::new()
            }),
            Fate::Soft("send_failed")
        ));
        assert!(matches!(
            fate(DirectSendOutcome::Failed {
                retryable: false,
                reason: "the reason".to_owned()
            }),
            Fate::Record(DirectResult::Failed { code }) if code == "rejected"
        ));
    }

    #[test]
    fn names_win_over_custom_fields() {
        let row = DirectRecipientRow {
            id: DirectSendId::from_uuid(Uuid::now_v7()),
            email: Some("a@example.com".to_owned()),
            first_name: Some("Ada".to_owned()),
            last_name: None,
            fields: json!({"first_name": "x", "plan": "pro", "seats": 3, "gone": null}),
            locale: None,
            suppressed: false,
        };
        let r = recipient(&row, "a@example.com");
        assert_eq!(
            r.merge_fields.get("first_name").map(String::as_str),
            Some("Ada")
        );
        assert_eq!(r.merge_fields.get("plan").map(String::as_str), Some("pro"));
        assert_eq!(r.merge_fields.get("seats").map(String::as_str), Some("3"));
        assert!(!r.merge_fields.contains_key("gone"));
        assert_eq!(r.locale, "en");
    }
}
