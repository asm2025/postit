//! The `publish_delivery` handler: claim, the pre-send checks, quota, and revalidation, then
//! the remote call and what its result does to the delivery.
//!
//! No database connection is held across a call into the agent: each step reads or writes on
//! a short-lived connection or transaction and releases it before the next.

use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use postit_core::{
    AgentError, DeliveryAttemptId, DeliveryEvent, DeliveryId, DeliveryState, GrantId, MediaItem,
    Notice, NotificationKind, ParkReason, PlatformId, PublishMode, PublishOutcome, PublishRequest,
    Schedule, ValidationInput, Violation, validate_target,
};
use postit_data::accounts::AccountRepo;
use postit_data::app_settings::AppSettingsRepo;
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::publishing::direct::Audience;
use postit_data::publishing::{
    Applied, AttemptRepo, CheckTarget, ClaimedDelivery, DeliveryRepo, Dispatch, Fence, JobRepo,
    JobStatusChange, Operation, Receipt, TransitionData,
};
use postit_data::{DataError, OwnerScope};
use postit_jobs::JobError;
use postit_oauth::{AccountError, Availability, BoundClient};
use serde_json::{Value, json};
use sqlx::PgConnection;

use crate::notices::{self, NoticeSubject};
use crate::outcome::{self, CancelCause, FollowUp, Mapped};
use crate::progress::DeliveryProgress;
use crate::quota::{self, QuotaOp, Reservation, ReservedUnit};
use crate::{
    AssembleError, Assembled, PollDeliveryStatus, PublishDelivery, PublishingService,
    ReconcileDelivery, assemble, checks, direct,
};

/// A claimed delivery that passed every check and holds its quota, ready for the remote call.
pub(crate) struct Prepared {
    /// The claim.
    pub claimed: ClaimedDelivery,
    /// The client bound to the account's grant.
    pub bound: BoundClient,
    /// The variant and its media.
    pub assembled: Assembled,
    /// The quota units taken for this attempt, given back if no remote call happens.
    pub reserved: Vec<ReservedUnit>,
    /// Who a direct delivery goes to, taken out of the variant's options.
    pub audience: Option<Audience>,
}

/// Runs one `publish_delivery` job: claims the delivery for the job's generation, runs the
/// pre-send checks, the quota reservation, and the revalidation, then calls the agent and
/// writes what came back under the delivery's fence. A delivery that is not claimable (another
/// generation, not due, already claimed, or its job cancelled) is left alone.
///
/// # Errors
///
/// [`JobError::Retry`] on a database failure after the claim; the claim's lease then runs out
/// and the due sweep hands the delivery on.
pub async fn run_publish(
    service: &PublishingService,
    job: PublishDelivery,
) -> Result<(), JobError> {
    let Some(prepared) = prepare(service, job).await? else {
        return Ok(());
    };
    publish(service, prepared).await
}

/// Claims and checks the delivery; `Some` when it may go to the platform now.
pub(crate) async fn prepare(
    service: &PublishingService,
    job: PublishDelivery,
) -> Result<Option<Prepared>, JobError> {
    let deps = &service.deps;
    let now = deps.clock.now();
    let lease = TimeDelta::from_std(deps.settings.lease)
        .map_err(|_| JobError::Fatal("publishing.lease is out of range".to_owned()))?;
    let claimed = {
        let mut conn = deps.pool.acquire().await.map_err(sql)?;
        DeliveryRepo::claim(
            &mut conn,
            DeliveryId::from_uuid(job.delivery),
            job.dispatch_gen,
            now,
            lease,
        )
        .await
        .map_err(data)?
    };
    let Some(claimed) = claimed else {
        return Ok(None);
    };

    let checked = pre_send_checks(service, &claimed).await?;
    if let Some(reason) = checked.park {
        park(service, &claimed, reason, None, None, now).await?;
        return Ok(None);
    }
    if checked.available == Availability::NotConfigured {
        fail(service, &claimed, unavailable(), &[], now).await?;
        return Ok(None);
    }
    let bound = match deps
        .accounts
        .bound_client(claimed.owner_id, claimed.account_id)
        .await
    {
        Ok(bound) => bound,
        Err(error) => {
            match unbound(error) {
                Unbound::Park(reason) => {
                    // Mark the grant with the park, so the sweep never sees the account
                    // `active` beside a delivery parked for it.
                    let mark = (reason == ParkReason::AccountNeedsReconnect)
                        .then_some(checked.grant)
                        .flatten();
                    park(service, &claimed, reason, None, mark, now).await?;
                }
                Unbound::Fail(code) => {
                    fail(service, &claimed, json!({"code": code}), &[], now).await?;
                }
                // A database failure: the lease runs out and the sweep hands the delivery on.
                Unbound::Retry(message) => return Err(JobError::Retry(message)),
            }
            return Ok(None);
        }
    };

    let Some(reserved) = reserve_quota(service, &claimed, &bound, now).await? else {
        return Ok(None);
    };

    match revalidate(service, &claimed, &bound, now).await {
        Ok(Ok((assembled, audience))) => Ok(Some(Prepared {
            claimed,
            bound,
            assembled,
            reserved,
            audience,
        })),
        Ok(Err(error)) => {
            fail(service, &claimed, error, &reserved, now).await?;
            Ok(None)
        }
        Err(error) => {
            give_back(service, &reserved).await;
            Err(error)
        }
    }
}

/// What a failed `bound_client` does to the claimed delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unbound {
    /// Park it; the sweep resumes it once the cause is gone.
    Park(ParkReason),
    /// Fail it with this `last_error` code: retrying cannot help.
    Fail(&'static str),
    /// Let the lease run out: a database failure that may pass.
    Retry(String),
}

/// Every persistent `bound_client` error ends the claim (a park or a failure), because a
/// claim's lease running out counts no attempt, so a retry here would loop unbounded.
fn unbound(error: AccountError) -> Unbound {
    match error {
        AccountError::Data(error) => Unbound::Retry(error.to_string()),
        AccountError::CredentialsInvalid { .. } => Unbound::Park(ParkReason::AccountNeedsReconnect),
        AccountError::PlatformDisabled => Unbound::Park(ParkReason::PlatformDisabled),
        AccountError::AccountDisconnected => Unbound::Fail("account_disconnected"),
        AccountError::PlatformUnavailable | AccountError::PlatformNotFound => {
            Unbound::Fail("platform_unavailable")
        }
        _ => Unbound::Fail("account_unusable"),
    }
}

/// What the pre-send checks found.
struct Checked {
    /// Why the delivery must wait, if it must.
    park: Option<ParkReason>,
    /// The platform's availability.
    available: Availability,
    /// The account's grant.
    grant: Option<GrantId>,
}

async fn pre_send_checks(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
) -> Result<Checked, JobError> {
    let account = {
        let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
        AccountRepo::get(
            &mut conn,
            &OwnerScope::own(claimed.owner_id),
            claimed.account_id,
        )
        .await
        .map_err(data)?
    };
    let available = availability(service, &account.platform).await?;
    let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
    let live_switch = match AppSettingsRepo::live_publishing(&mut conn).await {
        Ok(live) => live,
        // Never seeded: off, as the due sweep reads it.
        Err(DataError::NotFound) => false,
        Err(other) => return Err(data(other)),
    };
    let park = checks::park_reason(&mut conn, claimed, live_switch, available)
        .await
        .map_err(data)?;
    Ok(Checked {
        park,
        available,
        grant: account.grant_id,
    })
}

/// Reserves the quota the delivery's operation spends. `None` when a bucket is used up: the
/// delivery is then parked until the bucket's next period.
async fn reserve_quota(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    bound: &BoundClient,
    now: DateTime<Utc>,
) -> Result<Option<Vec<ReservedUnit>>, JobError> {
    let Some(op) = quota_op(claimed.operation) else {
        return Ok(Some(Vec::new()));
    };
    let reservation = {
        let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
        quota::reserve(
            &mut conn,
            claimed,
            &bound.descriptor,
            op,
            &bound.config,
            now,
        )
        .await
        .map_err(data)?
    };
    match reservation {
        Reservation::Reserved(units) => Ok(Some(units)),
        Reservation::Exhausted { retry_at } => {
            park(
                service,
                claimed,
                ParkReason::QuotaExceeded,
                Some(retry_at),
                None,
                now,
            )
            .await?;
            Ok(None)
        }
    }
}

/// The platform's availability as this build sees it: a platform outside
/// `compiled_platforms` is not compiled in, the same predicate the due sweep uses.
async fn availability(
    service: &PublishingService,
    platform: &PlatformId,
) -> Result<Availability, JobError> {
    if !service.deps.compiled_platforms.contains(platform) {
        return Ok(Availability::NotCompiledIn);
    }
    service
        .deps
        .accounts
        .platform_availability(platform)
        .await
        .map_err(|e| JobError::Retry(e.to_string()))
}

/// The quota a delivery's operation spends; direct sends count against their own daily cap.
fn quota_op(operation: Operation) -> Option<QuotaOp> {
    match operation {
        Operation::Publish => Some(QuotaOp::Publish),
        Operation::Comment => Some(QuotaOp::Comment),
        Operation::Direct => None,
    }
}

/// Assembles the request and runs postit's checks and the agent's. `Ok(Err(last_error))` when
/// the post is no longer valid. A direct delivery's audience is taken out of the variant's
/// options first: it is postit's, not the agent's.
async fn revalidate(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    bound: &BoundClient,
    now: DateTime<Utc>,
) -> Result<Result<(Assembled, Option<Audience>), Value>, JobError> {
    let snapshot = {
        let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
        JobRepo::snapshot(&mut conn, claimed.job_id)
            .await
            .map_err(data)?
    };
    let mut assembled = match assemble(
        &snapshot,
        claimed.account_id,
        &service.deps.media,
        claimed.owner_id,
    )
    .await
    {
        Ok(assembled) => assembled,
        Err(AssembleError::VariantMissing) => {
            return Ok(Err(revalidation_failed(&[
                json!({"code": "variant_missing"}),
            ])));
        }
        Err(AssembleError::MediaMissing(ids)) => {
            let media = snapshot
                .variant_for(claimed.account_id)
                .map(|variant| variant.media.as_slice())
                .unwrap_or_default();
            return Ok(Err(revalidation_failed(&missing_media(media, &ids))));
        }
        Err(AssembleError::Data(error)) => return Err(data(error)),
    };
    let audience = if claimed.operation == Operation::Direct {
        match direct::take_audience(&mut assembled.variant) {
            Ok(audience) => Some(audience),
            Err(violation) => return Ok(Err(revalidation_failed(&[violation]))),
        }
    } else {
        None
    };
    let descriptor = &bound.descriptor;
    let mut violations = validate_target(&ValidationInput {
        descriptor,
        options_schema: &descriptor.options_schema,
        variant: &assembled.variant,
        media: &assembled.assets,
        // The schedule was checked when the job was created; it is due now.
        schedule: Schedule::Now,
        now,
    });
    match bound
        .client
        .validate(&assembled.variant, &assembled.assets)
        .await
    {
        Ok(found) | Err(AgentError::Validation(found)) => violations.extend(found),
        // The check itself could not run; the publish call reports whatever is wrong.
        Err(error) => tracing::warn!(
            delivery = %claimed.id,
            class = ?error.retry_class(),
            "the agent's validation could not run; publishing without it"
        ),
    }
    if violations.is_empty() {
        Ok(Ok((assembled, audience)))
    } else {
        let listed: Vec<Value> = violations.iter().map(code_and_path).collect();
        Ok(Err(revalidation_failed(&listed)))
    }
}

/// A violation as `last_error` keeps it: its code and path, never its parameters.
fn code_and_path(violation: &Violation) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert("code".to_owned(), json!(violation.code.as_str()));
    if let Some(path) = &violation.path {
        entry.insert("path".to_owned(), json!(path));
    }
    Value::Object(entry)
}

/// One `media_missing` entry per missing asset, at its place in the variant's `media`; a
/// single entry without a path when no asset is named.
fn missing_media(media: &[MediaItem], ids: &[postit_core::MediaAssetId]) -> Vec<Value> {
    if ids.is_empty() {
        return vec![json!({"code": "media_missing"})];
    }
    ids.iter()
        .map(|id| match media.iter().position(|item| item.asset == *id) {
            Some(index) => json!({"code": "media_missing", "path": format!("media[{index}]")}),
            None => json!({"code": "media_missing"}),
        })
        .collect()
}

fn revalidation_failed(violations: &[Value]) -> Value {
    json!({"code": "revalidation_failed", "violations": violations})
}

fn unavailable() -> Value {
    json!({"code": "platform_unavailable"})
}

/// The `last_error` of a delivery whose account's agent lacks the operation's capability.
pub(crate) fn capability_unsupported() -> Value {
    json!({"code": "capability_unsupported", "class": "permanent"})
}

/// Parks `claimed` for `reason`, marks `mark` (the account's grant) as needing a reconnect,
/// and records `delivery_parked` for a live delivery, all in one transaction. A stale fence
/// changes nothing.
async fn park(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    reason: ParkReason,
    parked_until: Option<DateTime<Utc>>,
    mark: Option<GrantId>,
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let applied = DeliveryRepo::apply(
        &mut tx,
        fence(claimed, DeliveryState::Claimed),
        DeliveryEvent::Park(reason),
        TransitionData {
            last_error: Some(json!({"code": reason.as_str()})),
            parked_until,
            ..TransitionData::default()
        },
        now,
    )
    .await
    .map_err(data)?;
    if matches!(applied, Applied::Done { .. }) {
        if let Some(grant) = mark {
            mark_grant(service, &mut tx, claimed, grant).await?;
        }
        if !claimed.dry_run {
            notices::record(
                &deps.notifier,
                &mut tx,
                NoticeSubject::from(claimed),
                Some(reason),
                Notice::DeliveryParked,
            )
            .await
            .map_err(notify)?;
        }
    }
    tx.commit().await.map_err(sql)?;
    Ok(())
}

/// Ends `claimed` as `failed` through `RevalidationFailed` with `last_error`, gives back
/// `reserved` (no remote call happened), and records `delivery_failed` for a live delivery,
/// all in one transaction. When that transaction fails, `reserved` is still given back.
pub(crate) async fn fail(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    last_error: Value,
    reserved: &[ReservedUnit],
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let failed = fail_in_one_transaction(service, claimed, last_error, reserved, now).await;
    if failed.is_err() {
        give_back(service, reserved).await;
    }
    failed
}

async fn fail_in_one_transaction(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    last_error: Value,
    reserved: &[ReservedUnit],
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let mut tx = deps.pool.begin().await.map_err(sql)?;
    quota::release(&mut tx, reserved).await.map_err(data)?;
    let applied = DeliveryRepo::apply(
        &mut tx,
        fence(claimed, DeliveryState::Claimed),
        DeliveryEvent::RevalidationFailed,
        TransitionData {
            last_error: Some(last_error),
            ..TransitionData::default()
        },
        now,
    )
    .await
    .map_err(data)?;
    if matches!(applied, Applied::Done { .. }) && !claimed.dry_run {
        notices::record(
            &deps.notifier,
            &mut tx,
            NoticeSubject::from(claimed),
            None,
            Notice::DeliveryFailed,
        )
        .await
        .map_err(notify)?;
    }
    tx.commit().await.map_err(sql)?;
    Ok(())
}

/// Gives back `reserved` after a failure that leaves the claim to run out; best effort.
async fn give_back(service: &PublishingService, reserved: &[ReservedUnit]) {
    if reserved.is_empty() {
        return;
    }
    let released = match service.deps.pool.acquire().await {
        Ok(mut conn) => quota::release(&mut conn, reserved).await,
        Err(error) => Err(DataError::from(error)),
    };
    if released.is_err() {
        tracing::warn!("could not give back the quota a failed delivery run reserved");
    }
}

/// What the agent call returned, or that no call could be made.
enum Called {
    /// The agent's result.
    Agent(Result<PublishOutcome, AgentError>),
    /// The account's agent lacks the operation's capability; nothing was called.
    Unsupported,
}

/// Moves the prepared delivery to `in_flight`, calls the agent with the lease renewed in the
/// background, and writes the result.
async fn publish(service: &PublishingService, prepared: Prepared) -> Result<(), JobError> {
    let deps = &service.deps;
    let Prepared {
        claimed,
        bound,
        assembled,
        reserved,
        audience,
    } = prepared;
    if claimed.operation == Operation::Direct {
        // A direct delivery goes out in batches through `direct_send_batch`, not one call; it
        // reserves no per-bucket quota, only the owner's daily cap per batch.
        return direct::start(service, &claimed, &bound, audience.unwrap_or_default()).await;
    }
    let native_publish_at = match native_publish_at(service, &claimed).await {
        Ok(at) => at,
        Err(error) => {
            give_back(service, &reserved).await;
            return Err(error);
        }
    };
    let attempt = DeliveryAttemptId::from(deps.ids.generate());
    match start_remote(service, &claimed, attempt, deps.clock.now()).await {
        Ok(true) => {}
        // Cancelled or taken over since the claim: nothing went out.
        Ok(false) => {
            give_back(service, &reserved).await;
            return Ok(());
        }
        Err(error) => {
            give_back(service, &reserved).await;
            return Err(error);
        }
    }

    let fence = fence(&claimed, DeliveryState::InFlight);
    let progress = DeliveryProgress::new(
        deps.pool.clone(),
        fence,
        claimed.cancel_requested,
        deps.shutdown.clone(),
    );
    let called = {
        let _renewer = progress.renew_every_third(deps.settings.lease, Arc::clone(&deps.clock));
        let request = PublishRequest {
            variant: &assembled.variant,
            mode: if claimed.dry_run {
                PublishMode::DryRun
            } else {
                PublishMode::Live
            },
            native_publish_at,
            container_ref: claimed.container_ref.as_deref(),
            idempotency_key: claimed.id,
        };
        match claimed.operation {
            Operation::Comment => match bound.client.commenter() {
                Some(commenter) => Called::Agent(commenter.comment(request, &progress).await),
                None => Called::Unsupported,
            },
            _ => Called::Agent(bound.client.publish(request, &progress).await),
        }
    };
    // Give the units back when the result proves the platform did nothing: an unsupported
    // operation, a rejected credential, or a stop before anything was uploaded.
    let no_remote_call = match &called {
        Called::Unsupported | Called::Agent(Err(AgentError::Authentication { .. })) => true,
        Called::Agent(Err(AgentError::Cancelled)) => !progress.saved_container_ref(),
        Called::Agent(_) => false,
    };
    let released: &[ReservedUnit] = if no_remote_call { &reserved } else { &[] };
    let written = Written {
        claimed: &claimed,
        bound: &bound,
        assembled: &assembled,
        attempt,
        fence,
    };
    let recorded = written
        .record(service, &called, progress.cause(), released)
        .await;
    if recorded.is_err() {
        // The release rolled back with the write; give the units back on their own.
        give_back(service, released).await;
    }
    recorded
}

/// When the platform should publish a natively scheduled post: the job's time, for a job
/// that schedules on the platform.
async fn native_publish_at(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
) -> Result<Option<DateTime<Utc>>, JobError> {
    if !claimed.native_schedule {
        return Ok(None);
    }
    let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
    let job = JobRepo::get(
        &mut conn,
        &OwnerScope::own(claimed.owner_id),
        claimed.job_id,
    )
    .await
    .map_err(data)?;
    Ok(Some(job.scheduled_at))
}

/// `StartRemote` and the attempt's row, in one transaction. `false` when the fence is stale.
pub(crate) async fn start_remote(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    attempt: DeliveryAttemptId,
    now: DateTime<Utc>,
) -> Result<bool, JobError> {
    let mut tx = service.deps.pool.begin().await.map_err(sql)?;
    let applied = DeliveryRepo::apply(
        &mut tx,
        fence(claimed, DeliveryState::Claimed),
        DeliveryEvent::StartRemote,
        TransitionData::default(),
        now,
    )
    .await
    .map_err(data)?;
    if applied == Applied::Stale {
        return Ok(false);
    }
    AttemptRepo::start(&mut tx, attempt, claimed.id, now)
        .await
        .map_err(data)?;
    tx.commit().await.map_err(sql)?;
    Ok(true)
}

/// One finished remote call, ready to be written.
struct Written<'a> {
    claimed: &'a ClaimedDelivery,
    bound: &'a BoundClient,
    assembled: &'a Assembled,
    attempt: DeliveryAttemptId,
    fence: Fence,
}

impl Written<'_> {
    /// Ends the attempt, gives back `released`, and applies the mapped result under the
    /// fence with its follow-up work, notices, and audit events, all in one transaction. A
    /// stale fence holding a `Published` receipt records it as a late receipt; anything else
    /// stale is dropped.
    async fn record(
        &self,
        service: &PublishingService,
        called: &Called,
        cause: CancelCause,
        released: &[ReservedUnit],
    ) -> Result<(), JobError> {
        let deps = &service.deps;
        let now = deps.clock.now();
        let claimed = self.claimed;
        let mapped = match called {
            Called::Agent(result) => outcome::map(
                result,
                claimed.attempt_count,
                deps.settings.max_attempts,
                now,
                cause,
                &self.bound.descriptor,
            ),
            Called::Unsupported => Some(Mapped {
                event: DeliveryEvent::PermanentFailure,
                data: TransitionData {
                    last_error: Some(capability_unsupported()),
                    ..TransitionData::default()
                },
                follow_up: FollowUp::None,
                notice: Some(NotificationKind::DeliveryFailed),
            }),
        };
        let mut tx = deps.pool.begin().await.map_err(sql)?;
        let (name, error, summary) = self.attempt_row(called);
        AttemptRepo::finish(&mut tx, self.attempt, name, error, Some(summary), now)
            .await
            .map_err(data)?;
        quota::release(&mut tx, released).await.map_err(data)?;
        // A lost lease writes nothing: the sweep owns the delivery.
        if let Some(mapped) = mapped {
            let follow_up = mapped.follow_up;
            let notice = mapped.notice;
            let event = mapped.event;
            let applied = DeliveryRepo::apply(&mut tx, self.fence, event, mapped.data, now)
                .await
                .map_err(data)?;
            match applied {
                Applied::Done {
                    state,
                    redispatch,
                    job,
                } => {
                    self.after_write(service, &mut tx, follow_up, redispatch, now)
                        .await?;
                    tell(service, &mut tx, claimed, notice, state, job).await?;
                }
                Applied::Stale => {
                    if let Called::Agent(Ok(PublishOutcome::Published(receipt))) = called {
                        late_receipt(service, &mut tx, claimed, receipt, now).await?;
                    }
                }
            }
        }
        tx.commit().await.map_err(sql)?;
        Ok(())
    }

    /// The follow-up work of a landed write: the redispatch, the first poll or reconcile, or
    /// the grant mark.
    async fn after_write(
        &self,
        service: &PublishingService,
        tx: &mut PgConnection,
        follow_up: FollowUp,
        redispatch: Option<Dispatch>,
        now: DateTime<Utc>,
    ) -> Result<(), JobError> {
        let deps = &service.deps;
        let claimed = self.claimed;
        if let Some(dispatch) = redispatch {
            deps.queue
                .enqueue_in(tx, &PublishDelivery::from(dispatch), Some(dispatch.run_at))
                .await
                .map_err(enqueue)?;
        }
        let dispatch = Dispatch {
            delivery: claimed.id,
            dispatch_gen: claimed.dispatch_gen,
            run_at: now,
        };
        match follow_up {
            FollowUp::Poll { at } => {
                deps.queue
                    .enqueue_in(tx, &PollDeliveryStatus::from(dispatch), Some(at))
                    .await
                    .map_err(enqueue)?;
            }
            FollowUp::Reconcile => {
                deps.queue
                    .enqueue_in(tx, &ReconcileDelivery::from(dispatch), Some(now))
                    .await
                    .map_err(enqueue)?;
            }
            FollowUp::MarkGrant => mark_grant(service, tx, claimed, self.bound.grant).await?,
            FollowUp::None | FollowUp::Redispatch => {}
        }
        Ok(())
    }

    /// The attempt row's outcome, its error, and a summary of the request, without its text.
    fn attempt_row(&self, called: &Called) -> (&'static str, Option<Value>, Value) {
        let claimed = self.claimed;
        let mut summary = json!({
            "operation": claimed.operation.as_str(),
            "mode": if claimed.dry_run { "dry_run" } else { "live" },
            "media": self.assembled.assets.len(),
            "resumed": claimed.container_ref.is_some(),
        });
        let (name, error) = match called {
            Called::Unsupported => ("permanent", Some(capability_unsupported())),
            Called::Agent(Ok(PublishOutcome::Published(_))) => ("published", None),
            Called::Agent(Ok(PublishOutcome::Processing(_))) => ("processing", None),
            Called::Agent(Ok(PublishOutcome::Simulated { planned })) => {
                summary["planned"] = json!(planned);
                ("simulated", None)
            }
            Called::Agent(Err(error)) => (
                error.retry_class().as_str(),
                Some(outcome::last_error(error)),
            ),
        };
        (name, error, summary)
    }
}

/// The notices and audit events of a landed write that left `claimed` in `state`; none for a
/// dry run. A parked state's reason goes into the notice. `job_partial` is recorded only by
/// the write that first made the job `partial`.
pub(crate) async fn tell(
    service: &PublishingService,
    tx: &mut PgConnection,
    claimed: &ClaimedDelivery,
    notice: Option<NotificationKind>,
    state: DeliveryState,
    job: JobStatusChange,
) -> Result<(), JobError> {
    if claimed.dry_run {
        return Ok(());
    }
    if let Some(build) = notice.and_then(notices::delivery_kind) {
        let park_reason = match state {
            DeliveryState::Parked(reason) => Some(reason),
            _ => None,
        };
        notices::record(
            &service.deps.notifier,
            tx,
            NoticeSubject::from(claimed),
            park_reason,
            build,
        )
        .await
        .map_err(notify)?;
    }
    let audited = match state {
        DeliveryState::Published => Some(AuditEventKind::DeliveryPublished),
        DeliveryState::DeadLetter => Some(AuditEventKind::DeliveryDeadLettered),
        DeliveryState::OutcomeUnknown => Some(AuditEventKind::DeliveryOutcomeUnknown),
        _ => None,
    };
    if let Some(kind) = audited {
        audit(service, tx, kind, claimed).await?;
    }
    if job.became_partial() {
        notices::record_job_partial(&service.deps.notifier, tx, claimed)
            .await
            .map_err(notify)?;
    }
    Ok(())
}

/// A success receipt whose writer found its fence stale: it publishes the delivery while it is
/// still `outcome_unknown` (whatever the generation), and changes nothing otherwise.
pub(crate) async fn late_receipt(
    service: &PublishingService,
    tx: &mut PgConnection,
    claimed: &ClaimedDelivery,
    receipt: &postit_core::PublishReceipt,
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let Some(remote_id) = receipt.remote_id.clone() else {
        return Ok(());
    };
    let late = Receipt {
        remote_id,
        remote_url: receipt.remote_url.as_ref().map(ToString::to_string),
    };
    let Some(job) = DeliveryRepo::record_late_receipt(&mut *tx, claimed.id, late, now)
        .await
        .map_err(data)?
    else {
        return Ok(());
    };
    tell(
        service,
        tx,
        claimed,
        Some(NotificationKind::DeliveryPublished),
        DeliveryState::Published,
        job,
    )
    .await
}

/// What a fenced write by a poll or reconcile job did.
pub(crate) enum Wrote {
    /// It landed; `redispatch` is set when the delivery went back to `pending`.
    Done { redispatch: Option<Dispatch> },
    /// The fence no longer matched; nothing changed.
    Stale,
}

/// Applies `event` to `claimed` under `fence` in `tx`, and records the notice and audit
/// events of a landed write (see [`tell`]).
#[allow(clippy::too_many_arguments, reason = "one fenced write and its notice")]
pub(crate) async fn apply_and_tell(
    service: &PublishingService,
    tx: &mut PgConnection,
    claimed: &ClaimedDelivery,
    fence: Fence,
    event: DeliveryEvent,
    transition: TransitionData,
    notice: Option<NotificationKind>,
    now: DateTime<Utc>,
) -> Result<Wrote, JobError> {
    match DeliveryRepo::apply(&mut *tx, fence, event, transition, now)
        .await
        .map_err(data)?
    {
        Applied::Done {
            state,
            redispatch,
            job,
        } => {
            tell(service, tx, claimed, notice, state, job).await?;
            Ok(Wrote::Done { redispatch })
        }
        Applied::Stale => Ok(Wrote::Stale),
    }
}

/// The delivery as a poll or reconcile job sees it, read on a short-lived connection.
pub(crate) async fn check_target(
    service: &PublishingService,
    id: DeliveryId,
) -> Result<Option<CheckTarget>, JobError> {
    let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
    DeliveryRepo::check_target(&mut conn, id)
        .await
        .map_err(data)
}

/// Takes step `seq` of the delivery's chain and records `next_due` as its backstop, committed
/// before any platform call. `false` when a duplicate job already took it or the fence moved.
pub(crate) async fn take_check(
    service: &PublishingService,
    fence: Fence,
    seq: i32,
    next_due: DateTime<Utc>,
) -> Result<bool, JobError> {
    let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
    DeliveryRepo::take_check(&mut conn, fence, seq, next_due)
        .await
        .map_err(data)
}

/// Enqueues `job`, the chain's step `seq`, at `at` in `tx`; a platform's `retry_at` replaces
/// `at`, and then moves the backstop `take_check` recorded to it under `fence`.
pub(crate) async fn enqueue_successor<J: postit_jobs::Job>(
    service: &PublishingService,
    tx: &mut PgConnection,
    fence: Fence,
    seq: i32,
    job: &J,
    at: DateTime<Utc>,
    retry_at: Option<DateTime<Utc>>,
) -> Result<(), JobError> {
    let at = match retry_at {
        Some(retry_at) => {
            DeliveryRepo::set_check_due(&mut *tx, fence, seq, retry_at)
                .await
                .map_err(data)?;
            retry_at
        }
        None => at,
    };
    service
        .deps
        .queue
        .enqueue_in(tx, job, Some(at))
        .await
        .map(|_| ())
        .map_err(enqueue)
}

/// Marks `grant` as needing a reconnect, in the caller's transaction.
pub(crate) async fn mark_grant(
    service: &PublishingService,
    tx: &mut PgConnection,
    claimed: &ClaimedDelivery,
    grant: GrantId,
) -> Result<(), JobError> {
    service
        .deps
        .accounts
        .mark_needs_reconnect_in(
            tx,
            &OwnerScope::own(claimed.owner_id),
            grant,
            AuditContext::default(),
        )
        .await
        .map_err(|e| JobError::Retry(e.to_string()))
}

/// Records an audit event about `claimed`, carrying its IDs only.
pub(crate) async fn audit(
    service: &PublishingService,
    tx: &mut PgConnection,
    kind: AuditEventKind,
    claimed: &ClaimedDelivery,
) -> Result<(), JobError> {
    let detail = |error: postit_data::audit::AuditError| JobError::Fatal(error.to_string());
    let event = AuditEvent::new(kind)
        .owner(claimed.owner_id)
        .detail("delivery_id", claimed.id.to_string())
        .map_err(detail)?
        .detail("job_id", claimed.job_id.to_string())
        .map_err(detail)?
        .detail("account_id", claimed.account_id.to_string())
        .map_err(detail)?;
    AuditLog::record(
        tx,
        postit_core::AuditEventId::from(service.deps.ids.generate()),
        event,
    )
    .await
    .map_err(data)
}

pub(crate) fn fence(claimed: &ClaimedDelivery, expected: DeliveryState) -> Fence {
    Fence {
        id: claimed.id,
        dispatch_gen: claimed.dispatch_gen,
        expected,
    }
}

#[expect(clippy::needless_pass_by_value, reason = "used as a map_err adapter")]
pub(crate) fn data(error: DataError) -> JobError {
    JobError::Retry(error.to_string())
}

#[expect(clippy::needless_pass_by_value, reason = "used as a map_err adapter")]
pub(crate) fn sql(error: sqlx::Error) -> JobError {
    JobError::Retry(error.to_string())
}

#[expect(clippy::needless_pass_by_value, reason = "used as a map_err adapter")]
pub(crate) fn notify(error: postit_notify::NotifyError) -> JobError {
    JobError::Retry(error.to_string())
}

#[expect(clippy::needless_pass_by_value, reason = "used as a map_err adapter")]
pub(crate) fn enqueue(error: postit_jobs::JobsError) -> JobError {
    JobError::Retry(error.to_string())
}

#[cfg(test)]
mod tests {
    use postit_core::MediaAssetId;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn persistent_bound_client_errors_end_the_claim() {
        assert_eq!(
            unbound(AccountError::CredentialsInvalid {
                field: None,
                missing_scopes: Vec::new(),
            }),
            Unbound::Park(ParkReason::AccountNeedsReconnect)
        );
        assert_eq!(
            unbound(AccountError::PlatformDisabled),
            Unbound::Park(ParkReason::PlatformDisabled)
        );
        assert_eq!(
            unbound(AccountError::AccountDisconnected),
            Unbound::Fail("account_disconnected")
        );
        assert_eq!(
            unbound(AccountError::PlatformNotFound),
            Unbound::Fail("platform_unavailable")
        );
        assert_eq!(
            unbound(AccountError::NotFound),
            Unbound::Fail("account_unusable")
        );
        assert!(matches!(
            unbound(AccountError::Data(DataError::NotFound)),
            Unbound::Retry(_)
        ));
    }

    #[test]
    fn missing_media_names_each_asset_or_reports_one_entry() {
        let a = MediaAssetId::from_uuid(Uuid::now_v7());
        let b = MediaAssetId::from_uuid(Uuid::now_v7());
        let media = [MediaItem {
            asset: a,
            alt_text: None,
        }];
        assert_eq!(
            missing_media(&media, &[a, b]),
            vec![
                json!({"code": "media_missing", "path": "media[0]"}),
                json!({"code": "media_missing"}),
            ]
        );
        assert_eq!(
            missing_media(&media, &[]),
            vec![json!({"code": "media_missing"})]
        );
    }
}
