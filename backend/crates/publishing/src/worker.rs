//! The `publish_delivery` handler: claim, the pre-send checks, quota, and revalidation, up to
//! the moment the delivery would go to the platform.
//!
//! No database connection is held across a call into the agent: each step reads or writes on
//! a short-lived connection or transaction and releases it before the next.

use chrono::{DateTime, TimeDelta, Utc};
use postit_core::{
    AgentError, DeliveryEvent, DeliveryId, DeliveryState, MediaAssetId, Notice, ParkReason,
    PlatformId, PublishSnapshot, Schedule, ValidationInput, Variant, Violation, validate_target,
};
use postit_data::accounts::AccountRepo;
use postit_data::app_settings::AppSettingsRepo;
use postit_data::publishing::{
    Applied, ClaimedDelivery, DeliveryRepo, Fence, JobRepo, Operation, TransitionData,
};
use postit_data::{DataError, OwnerScope};
use postit_jobs::JobError;
use postit_oauth::{AccountError, Availability, BoundClient};
use serde_json::{Value, json};

use crate::notices::{self, NoticeSubject};
use crate::quota::{self, QuotaOp, Reservation, ReservedUnit};
use crate::{AssembleError, Assembled, PublishDelivery, PublishingService, assemble, checks};

/// A claimed delivery that passed every check and holds its quota, ready for the remote call.
#[expect(dead_code, reason = "the remote call reads it in the next task")]
pub(crate) struct Prepared {
    /// The claim.
    pub claimed: ClaimedDelivery,
    /// The client bound to the account's grant.
    pub bound: BoundClient,
    /// The variant and its media.
    pub assembled: Assembled,
    /// The quota units taken for this attempt, given back if no remote call happens.
    pub reserved: Vec<ReservedUnit>,
}

/// Runs one `publish_delivery` job: claims the delivery for the job's generation and runs the
/// pre-send checks, the quota reservation, and the revalidation. A delivery that is not
/// claimable (another generation, not due, already claimed, or its job cancelled) is left
/// alone.
///
/// # Errors
///
/// [`JobError::Retry`] on a database failure after the claim; the claim's lease then runs out
/// and the due sweep hands the delivery on.
pub async fn run_publish(
    service: &PublishingService,
    job: PublishDelivery,
) -> Result<(), JobError> {
    prepare(service, job).await.map(|_| ())
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

    let (reason, available) = pre_send_checks(service, &claimed).await?;
    if let Some(reason) = reason {
        park(service, &claimed, reason, None, now).await?;
        return Ok(None);
    }
    if available == Availability::NotConfigured {
        fail(service, &claimed, unavailable(), &[], now).await?;
        return Ok(None);
    }
    let bound = match deps
        .accounts
        .bound_client(claimed.owner_id, claimed.account_id)
        .await
    {
        Ok(bound) => bound,
        Err(AccountError::PlatformUnavailable) => {
            fail(service, &claimed, unavailable(), &[], now).await?;
            return Ok(None);
        }
        // Something changed since the checks ran; the lease runs out and the next run parks it.
        Err(other) => return Err(JobError::Retry(other.to_string())),
    };

    let Some(reserved) = reserve_quota(service, &claimed, &bound, now).await? else {
        return Ok(None);
    };

    match revalidate(service, &claimed, &bound, now).await {
        Ok(Ok(assembled)) => Ok(Some(Prepared {
            claimed,
            bound,
            assembled,
            reserved,
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

/// The platform's availability and the reason `claimed` must wait, if any.
async fn pre_send_checks(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
) -> Result<(Option<ParkReason>, Availability), JobError> {
    let platform = {
        let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
        AccountRepo::get(
            &mut conn,
            &OwnerScope::own(claimed.owner_id),
            claimed.account_id,
        )
        .await
        .map_err(data)?
        .platform
    };
    let available = availability(service, &platform).await?;
    let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
    let live_switch = match AppSettingsRepo::live_publishing(&mut conn).await {
        Ok(live) => live,
        // Never seeded: off, as the due sweep reads it.
        Err(DataError::NotFound) => false,
        Err(other) => return Err(data(other)),
    };
    let reason = checks::park_reason(&mut conn, claimed, live_switch, available)
        .await
        .map_err(data)?;
    Ok((reason, available))
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
/// the post is no longer valid.
async fn revalidate(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    bound: &BoundClient,
    now: DateTime<Utc>,
) -> Result<Result<Assembled, Value>, JobError> {
    let snapshot = {
        let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
        JobRepo::snapshot(&mut conn, claimed.job_id)
            .await
            .map_err(data)?
    };
    let assembled = match assemble(
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
            return Ok(Err(revalidation_failed(&missing_media(
                &snapshot, claimed, &ids,
            ))));
        }
        Err(AssembleError::Data(error)) => return Err(data(error)),
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
        Ok(Ok(assembled))
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

/// One `media_missing` entry per missing asset, at its place in the variant.
fn missing_media(
    snapshot: &PublishSnapshot,
    claimed: &ClaimedDelivery,
    ids: &[MediaAssetId],
) -> Vec<Value> {
    let media = snapshot
        .variant_for(claimed.account_id)
        .map(|variant: &Variant| variant.media.as_slice())
        .unwrap_or_default();
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

/// Parks `claimed` for `reason`, and records `delivery_parked` for a live delivery in the same
/// transaction. A stale fence changes nothing.
async fn park(
    service: &PublishingService,
    claimed: &ClaimedDelivery,
    reason: ParkReason,
    parked_until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let applied = DeliveryRepo::apply(
        &mut tx,
        fence(claimed),
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
    if matches!(applied, Applied::Done { .. }) && !claimed.dry_run {
        notices::record(
            &deps.notifier,
            &mut tx,
            NoticeSubject::from(claimed),
            Some(reason),
            Notice::DeliveryParked,
        )
        .await
        .map_err(|e| JobError::Retry(e.to_string()))?;
    }
    tx.commit().await.map_err(sql)?;
    Ok(())
}

/// Ends `claimed` as `failed` through `RevalidationFailed` with `last_error`, gives back
/// `reserved` (no remote call happened), and records `delivery_failed` for a live delivery,
/// all in one transaction.
async fn fail(
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
        fence(claimed),
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
        .map_err(|e| JobError::Retry(e.to_string()))?;
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

fn fence(claimed: &ClaimedDelivery) -> Fence {
    Fence {
        id: claimed.id,
        dispatch_gen: claimed.dispatch_gen,
        expected: DeliveryState::Claimed,
    }
}

#[expect(clippy::needless_pass_by_value, reason = "used as a map_err adapter")]
fn data(error: DataError) -> JobError {
    JobError::Retry(error.to_string())
}

#[expect(clippy::needless_pass_by_value, reason = "used as a map_err adapter")]
fn sql(error: sqlx::Error) -> JobError {
    JobError::Retry(error.to_string())
}
