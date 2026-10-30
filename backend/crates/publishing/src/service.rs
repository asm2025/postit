//! The publishing service: what the API's publish routes call. It creates jobs, reschedules,
//! cancels, retries, and resolves them, under the caller's scope.
//!
//! No database connection or transaction is held across a platform call: a reschedule or a
//! cancel that must reach an agent (`NativeScheduler::reschedule` or `delete`) commits its
//! database part first, calls the agent with no connection held, and writes each result in its
//! own transaction fenced on the delivery's generation and `processing` state.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use postit_core::{
    AccountId, AgentKind, AuditEventId, DeliveryEvent, DeliveryId, DeliveryState, MediaAsset,
    MediaAssetId, MediaItem, PostId, PostKind, PostOverride, PostVersion, PublishJobId,
    PublishReceipt, PublishSnapshot, ReplyTo, ResolvedOutcome, RetryClass, Schedule, UserId,
    ValidationInput, Variant, Violation, ViolationCode, resolve_variant, validate_target,
};
use postit_data::audit::{AuditContext, AuditEvent, AuditEventKind, AuditLog};
use postit_data::posts::{Post, PostOverrideRow, PostRepo};
use postit_data::publishing::{
    Applied, CancelOutcome, CancelResult, CreatedJob, Delivery, DeliveryRepo, Dispatch, Fence,
    JobRepo, JobStatusChange, NewDelivery, NewJob, Operation, PublishJob, TransitionData,
};
use postit_data::{Capability, OwnerScope};
use postit_media::MediaServiceError;
use postit_oauth::{AccountError, BoundClient, NativeCleanup};
use serde_json::{Value, json};
use sqlx::PgConnection;

use crate::error::{PublishingError, reschedule_error, resolve_error, retry_error};
use crate::{
    AssembleError, PollDeliveryStatus, PublishDelivery, PublishingDeps, assemble, direct, notices,
};

/// How far in the past a schedule time may be and still mean "now".
const SCHEDULE_GRACE: TimeDelta = TimeDelta::seconds(60);

/// The publishing engine. Cheap to clone.
#[derive(Clone)]
pub struct PublishingService {
    pub(crate) deps: Arc<PublishingDeps>,
}

/// What a publish asks for.
#[derive(Debug, Clone)]
pub struct PublishInput {
    /// The accounts to publish to, one delivery each.
    pub targets: Vec<AccountId>,
    /// When to publish; `None` (or up to a minute in the past) is now.
    pub at: Option<DateTime<Utc>>,
    /// Whether the targets whose agents can hold the post until `at` do so. The others are
    /// scheduled by postit.
    pub native_schedule: bool,
    /// Whether the job only simulates.
    pub dry_run: bool,
    /// The caller's confirmation of a live publish.
    pub confirm: bool,
    /// The post version the caller reviewed (`If-Match`).
    pub if_match: PostVersion,
}

/// What a reschedule moved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RescheduleOutcome {
    /// Deliveries now at the new time: `pending` ones, and native uploads the platform moved.
    pub moved: Vec<DeliveryId>,
    /// Native uploads the platform did not move; they keep their old time.
    pub failed: Vec<DeliveryId>,
}

/// What a cancel did to each delivery that had not ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CancelReport {
    /// Now `cancelled`.
    pub cancelled: Vec<DeliveryId>,
    /// Running: the worker stops at its next safe checkpoint.
    pub requested: Vec<DeliveryId>,
    /// Left as they were: an unknown outcome (resolved instead), a platform still processing,
    /// or a native upload the agent could not delete.
    pub not_cancellable: Vec<DeliveryId>,
}

/// What deleting one natively scheduled upload did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeDelete {
    /// Deleted on the platform, and the delivery is now `cancelled`.
    Cancelled,
    /// Left on the platform for good: the agent cannot delete it (no `NativeScheduler`, a
    /// permanent or authorization error, the account disconnected or its platform off), or
    /// the delivery changed meanwhile.
    Kept,
    /// Left on the platform for now: a transient platform error or a database failure; a
    /// later try may delete it.
    Transient,
}

/// How many natively scheduled uploads ended each way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NativeTally {
    /// Deleted and cancelled.
    pub(crate) deleted: u32,
    /// Left on the platform for good.
    pub(crate) kept: u32,
    /// Left on the platform for now.
    pub(crate) transient: u32,
}

impl NativeTally {
    /// The tally as a disconnect reports it: every upload left on the platform remains.
    pub(crate) fn cleanup(self) -> NativeCleanup {
        NativeCleanup {
            deleted: self.deleted,
            remaining: self.kept + self.transient,
        }
    }
}

/// One target of a new job, checked.
struct Target {
    account: AccountId,
    variant: Variant,
    operation: Operation,
    native: bool,
}

impl PublishingService {
    /// A service over `deps`.
    #[must_use]
    pub fn new(deps: PublishingDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }

    /// Creates a publish job for `post`.
    ///
    /// A live job needs [`Capability::Publish`] and `confirm`, a dry run [`Capability::Edit`],
    /// and every target must be in the caller's account limit. `at` up to a minute in the past
    /// is now; earlier is a `schedule_in_past` violation. Every target is validated and every
    /// violation returned at once. The post is locked at `if_match` while the job, its
    /// deliveries, their `publish_delivery` jobs, and the live job's `publish_confirmed` audit
    /// event are written in one transaction. With `native_schedule`, a target whose agent can
    /// hold the post uploads now with the publish time; the others wait for `at`.
    ///
    /// # Errors
    ///
    /// [`PublishingError::Scope`], [`PublishingError::AccountNotInScope`],
    /// [`PublishingError::ConfirmationRequired`], [`PublishingError::NotFound`] (post or
    /// account), [`PublishingError::VersionConflict`], [`PublishingError::PostLocked`],
    /// [`PublishingError::ValidationFailed`], [`PublishingError::PlatformDisabled`] and
    /// [`PublishingError::PlatformUnavailable`], and data or queue failures.
    pub async fn create_job(
        &self,
        scope: &OwnerScope,
        post: PostId,
        input: PublishInput,
        audit: AuditContext,
    ) -> Result<CreatedJob, PublishingError> {
        let deps = &self.deps;
        scope.require(if input.dry_run {
            Capability::Edit
        } else {
            Capability::Publish
        })?;
        if input
            .targets
            .iter()
            .any(|account| !scope.allows_account(*account))
        {
            return Err(PublishingError::AccountNotInScope);
        }
        if !input.dry_run && !input.confirm {
            return Err(PublishingError::ConfirmationRequired);
        }
        let now = deps.clock.now();
        let at = due_time(input.at, now)?;
        let (row, base, overrides) = self.read_post(scope, post, input.if_match).await?;
        let targets = self
            .check_targets(scope, &base, &overrides, &input, at, now)
            .await?;

        let scheduled_at = at.unwrap_or(now);
        let snapshot = PublishSnapshot {
            post_id: post,
            post_version: input.if_match,
            variants: targets.iter().map(|t| t.variant.clone()).collect(),
        };
        let new = NewJob {
            id: PublishJobId::from(deps.ids.generate()),
            snapshot,
            targets: targets
                .iter()
                .map(|t| NewDelivery {
                    id: DeliveryId::from(deps.ids.generate()),
                    account: t.account,
                    operation: t.operation,
                    due_at: t.native.then_some(now),
                })
                .collect(),
            scheduled_at,
            native_schedule: input.native_schedule,
            dry_run: input.dry_run,
            recurrence_id: None,
        };
        let mut tx = deps.pool.begin().await?;
        let locked = PostRepo::lock(&mut tx, scope, post, Some(input.if_match.0)).await?;
        let created = JobRepo::create(&mut tx, scope, &locked, new).await?;
        for dispatch in &created.deliveries {
            deps.queue
                .enqueue_in(
                    &mut tx,
                    &PublishDelivery::from(*dispatch),
                    Some(dispatch.run_at),
                )
                .await?;
        }
        if !input.dry_run {
            let accounts: Vec<Value> = input
                .targets
                .iter()
                .map(|account| json!(account.to_string()))
                .collect();
            self.audit(
                &mut tx,
                AuditEventKind::PublishConfirmed,
                scope,
                audit,
                &[
                    ("job_id", json!(created.job.to_string())),
                    ("post_id", json!(post.to_string())),
                    ("post_kind", json!(row.kind.as_str())),
                    ("account_ids", Value::Array(accounts)),
                ],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(created)
    }

    /// Moves a job that has not started to `at` (up to a minute in the past is now). Its
    /// `pending` deliveries get the new time and the next generation, and are enqueued again;
    /// then each natively scheduled upload is moved through its agent, one at a time. An upload
    /// the agent does not move, or one that changed meanwhile, is listed in `failed`; the
    /// others keep their new time.
    ///
    /// # Errors
    ///
    /// [`PublishingError::VersionConflict`] when the job's version is not `if_match`,
    /// [`PublishingError::NotReschedulable`] when a delivery is past `pending` and not a
    /// native upload, or the job was cancelled, [`PublishingError::ValidationFailed`] for a time
    /// in the past, and the scope and not-found errors.
    pub async fn reschedule(
        &self,
        scope: &OwnerScope,
        job: PublishJobId,
        at: DateTime<Utc>,
        if_match: i64,
        audit: AuditContext,
    ) -> Result<RescheduleOutcome, PublishingError> {
        let deps = &self.deps;
        scope.require(Capability::Publish)?;
        let now = deps.clock.now();
        let at = due_time(Some(at), now)?.unwrap_or(now);
        let current = {
            let mut conn = deps.pool.acquire().await?;
            JobRepo::get(&mut conn, scope, job).await?
        };
        if current.version != if_match {
            return Err(PublishingError::VersionConflict);
        }
        let upload_now = self.native_targets(scope, &current).await?;

        let mut tx = deps.pool.begin().await?;
        let moved = JobRepo::reschedule_pending_with(&mut tx, scope, job, at, now, &upload_now)
            .await
            .map_err(reschedule_error)?;
        // The reschedule bumps the job's version once; anything more means another write
        // landed after the caller read it.
        let after = JobRepo::get(&mut tx, scope, job).await?.version;
        if Some(after) != if_match.checked_add(1) {
            return Err(PublishingError::VersionConflict);
        }
        for dispatch in &moved.dispatches {
            deps.queue
                .enqueue_in(
                    &mut tx,
                    &PublishDelivery::from(*dispatch),
                    Some(dispatch.run_at),
                )
                .await?;
        }
        let mut natives = Vec::with_capacity(moved.needs_agent.len());
        for id in &moved.needs_agent {
            natives.push(DeliveryRepo::get(&mut tx, scope, *id).await?);
        }
        self.audit(
            &mut tx,
            AuditEventKind::PublishJobRescheduled,
            scope,
            audit,
            &[
                ("job_id", json!(job.to_string())),
                ("pending", json!(moved.dispatches.len())),
                ("native", json!(natives.len())),
            ],
        )
        .await?;
        tx.commit().await?;

        let mut outcome = RescheduleOutcome {
            moved: moved.dispatches.iter().map(|d| d.delivery).collect(),
            failed: Vec::new(),
        };
        for delivery in natives {
            if self.move_native(scope.owner, &delivery, at).await? {
                outcome.moved.push(delivery.id);
            } else {
                outcome.failed.push(delivery.id);
            }
        }
        Ok(outcome)
    }

    /// Cancels every delivery of `job` that has not ended (see [`CancelReport`]); the whole
    /// job must be in the caller's account limit. A natively scheduled upload is deleted
    /// through its agent and then cancelled.
    ///
    /// # Errors
    ///
    /// [`PublishingError::Scope`], [`PublishingError::NotFound`],
    /// [`PublishingError::AccountNotInScope`], and data failures.
    pub async fn cancel_job(
        &self,
        scope: &OwnerScope,
        job: PublishJobId,
        audit: AuditContext,
    ) -> Result<CancelReport, PublishingError> {
        let deps = &self.deps;
        scope.require(Capability::Publish)?;
        let now = deps.clock.now();
        let mut tx = deps.pool.begin().await?;
        let (outcomes, change) = JobRepo::cancel_with_status(&mut tx, scope, job, now).await?;
        let row = JobRepo::get(&mut tx, scope, job).await?;
        let mut report = CancelReport::default();
        let mut natives = Vec::new();
        for (id, outcome) in outcomes {
            match outcome {
                CancelOutcome::Cancelled => report.cancelled.push(id),
                CancelOutcome::Requested => report.requested.push(id),
                CancelOutcome::NotCancellable => report.not_cancellable.push(id),
                CancelOutcome::NeedsAgent { .. } => {
                    natives.push(DeliveryRepo::get(&mut tx, scope, id).await?);
                }
            }
        }
        self.job_partial(&mut tx, &row, scope.owner, change).await?;
        self.audit(
            &mut tx,
            AuditEventKind::PublishJobCancelled,
            scope,
            audit,
            &[
                ("job_id", json!(job.to_string())),
                ("cancelled", json!(report.cancelled.len())),
                ("requested", json!(report.requested.len())),
                ("native", json!(natives.len())),
            ],
        )
        .await?;
        tx.commit().await?;
        for delivery in natives {
            self.cancel_native(scope, &row, &delivery, &mut report)
                .await;
        }
        Ok(report)
    }

    /// Cancels one delivery (see [`CancelReport`]).
    ///
    /// # Errors
    ///
    /// [`PublishingError::Scope`], [`PublishingError::NotFound`], and data failures.
    pub async fn cancel_delivery(
        &self,
        scope: &OwnerScope,
        delivery: DeliveryId,
        audit: AuditContext,
    ) -> Result<CancelReport, PublishingError> {
        let deps = &self.deps;
        scope.require(Capability::Publish)?;
        let now = deps.clock.now();
        let mut tx = deps.pool.begin().await?;
        let (outcome, change) =
            DeliveryRepo::cancel_with_status(&mut tx, scope, delivery, now).await?;
        let found = DeliveryRepo::get(&mut tx, scope, delivery).await?;
        let row = JobRepo::get(&mut tx, scope, found.job_id).await?;
        let mut report = CancelReport::default();
        let mut native = None;
        let name = match outcome {
            CancelOutcome::Cancelled => {
                report.cancelled.push(delivery);
                Some("cancelled")
            }
            CancelOutcome::Requested => {
                report.requested.push(delivery);
                Some("requested")
            }
            CancelOutcome::NeedsAgent { .. } => {
                native = Some(found.clone());
                Some("native")
            }
            CancelOutcome::NotCancellable => {
                report.not_cancellable.push(delivery);
                None
            }
        };
        if let Some(change) = change {
            self.job_partial(&mut tx, &row, scope.owner, change).await?;
        }
        if let Some(name) = name {
            self.audit(
                &mut tx,
                AuditEventKind::DeliveryCancelled,
                scope,
                audit,
                &[
                    ("delivery_id", json!(delivery.to_string())),
                    ("job_id", json!(found.job_id.to_string())),
                    ("account_id", json!(found.account_id.to_string())),
                    ("outcome", json!(name)),
                ],
            )
            .await?;
        }
        tx.commit().await?;
        if let Some(found) = native {
            self.cancel_native(scope, &row, &found, &mut report).await;
        }
        Ok(report)
    }

    /// Sends a failed, dead-lettered, parked, or outcome-unknown delivery through again with a
    /// fresh attempt count: a new publish of the job's snapshot, so it needs `confirm`, and
    /// `acknowledge_outcome_unknown` when the post may already be live. The snapshot's variant
    /// is validated again with its media as they are now.
    ///
    /// A direct campaign that already materialized its audience is refused
    /// ([`PublishingError::NotRetryable`]): some recipients may have been sent to and the rest
    /// were stopped, so a retry would either send nothing or send twice. One that ended before
    /// materializing materializes its audience afresh.
    ///
    /// # Errors
    ///
    /// [`PublishingError::ConfirmationRequired`],
    /// [`PublishingError::AcknowledgementRequired`], [`PublishingError::ValidationFailed`],
    /// [`PublishingError::JobActive`], [`PublishingError::NotRetryable`], and the scope and
    /// not-found errors.
    pub async fn retry(
        &self,
        scope: &OwnerScope,
        delivery: DeliveryId,
        confirm: bool,
        acknowledge_outcome_unknown: bool,
        audit: AuditContext,
    ) -> Result<Dispatch, PublishingError> {
        let deps = &self.deps;
        if !confirm {
            return Err(PublishingError::ConfirmationRequired);
        }
        scope.require(Capability::Publish)?;
        let (found, snapshot) = {
            let mut conn = deps.pool.acquire().await?;
            let found = DeliveryRepo::get(&mut conn, scope, delivery).await?;
            let snapshot = JobRepo::snapshot(&mut conn, found.job_id).await?;
            (found, snapshot)
        };
        // A delivery that cannot be retried in its state is refused before its post is
        // validated again: a published delivery with deleted media is not retryable, not
        // invalid. The repository checks the state again under its lock.
        if let Err(postit_core::TransitionError::NotAllowed { .. }) = postit_core::transition(
            found.state,
            DeliveryEvent::UserRetry {
                acknowledged_outcome_unknown: true,
            },
        ) {
            return Err(PublishingError::NotRetryable);
        }
        self.revalidate(scope, &found, &snapshot).await?;

        let now = deps.clock.now();
        let mut tx = deps.pool.begin().await?;
        let dispatch =
            DeliveryRepo::retry(&mut tx, scope, delivery, acknowledge_outcome_unknown, now)
                .await
                .map_err(retry_error)?;
        deps.queue
            .enqueue_in(
                &mut tx,
                &PublishDelivery::from(dispatch),
                Some(dispatch.run_at),
            )
            .await?;
        self.audit(
            &mut tx,
            AuditEventKind::DeliveryRetried,
            scope,
            audit,
            &[
                ("delivery_id", json!(delivery.to_string())),
                ("job_id", json!(found.job_id.to_string())),
                ("account_id", json!(found.account_id.to_string())),
                ("acknowledged", json!(acknowledge_outcome_unknown)),
            ],
        )
        .await?;
        tx.commit().await?;
        Ok(dispatch)
    }

    /// Settles a dead-lettered, parked, or outcome-unknown delivery by the caller's word that
    /// the post did or did not go out, and returns its new state.
    ///
    /// # Errors
    ///
    /// [`PublishingError::Scope`], [`PublishingError::NotFound`], and
    /// [`PublishingError::NotResolvable`] in a state that cannot be resolved.
    pub async fn resolve(
        &self,
        scope: &OwnerScope,
        delivery: DeliveryId,
        outcome: ResolvedOutcome,
        audit: AuditContext,
    ) -> Result<DeliveryState, PublishingError> {
        let deps = &self.deps;
        scope.require(Capability::Publish)?;
        let now = deps.clock.now();
        let mut tx = deps.pool.begin().await?;
        let (state, change) =
            DeliveryRepo::resolve_with_status(&mut tx, scope, delivery, outcome, now)
                .await
                .map_err(resolve_error)?;
        let found = DeliveryRepo::get(&mut tx, scope, delivery).await?;
        let row = JobRepo::get(&mut tx, scope, found.job_id).await?;
        self.job_partial(&mut tx, &row, scope.owner, change).await?;
        self.audit(
            &mut tx,
            AuditEventKind::DeliveryResolved,
            scope,
            audit,
            &[
                ("delivery_id", json!(delivery.to_string())),
                ("job_id", json!(found.job_id.to_string())),
                ("account_id", json!(found.account_id.to_string())),
                ("outcome", json!(state.as_str())),
            ],
        )
        .await?;
        tx.commit().await?;
        Ok(state)
    }

    /// The post as the caller sees it, checked against `if_match`, with its media and its
    /// overrides. The job's transaction locks it at the same version, so what is read here is
    /// what the snapshot freezes.
    async fn read_post(
        &self,
        scope: &OwnerScope,
        post: PostId,
        if_match: PostVersion,
    ) -> Result<(Post, postit_core::Post, BTreeMap<AccountId, PostOverride>), PublishingError> {
        let mut conn = self.deps.pool.acquire().await?;
        let row = PostRepo::get(&mut conn, scope, post).await?;
        if row.version != if_match.0 {
            return Err(PublishingError::VersionConflict);
        }
        let media = PostRepo::media(&mut conn, scope, post).await?;
        let overrides = PostRepo::overrides(&mut conn, scope, post).await?;
        drop(conn);
        let base = postit_core::Post {
            kind: row.kind,
            text: row.text.clone(),
            media: media
                .into_iter()
                .map(|item| MediaItem {
                    asset: item.media_id,
                    alt_text: item.alt_text,
                })
                .collect(),
            link: row
                .link
                .as_deref()
                .map(url::Url::parse)
                .transpose()
                .map_err(|_| corrupt("post_link_invalid"))?,
            tags: row.tags.clone(),
            disclose_ai_generated: row.disclose_ai_generated,
            reply_to: row
                .reply_to
                .clone()
                .map(serde_json::from_value::<ReplyTo>)
                .transpose()
                .map_err(|_| corrupt("post_reply_to_invalid"))?,
        };
        let overrides = overrides
            .into_iter()
            .map(|row| Ok((row.account_id, override_of(row)?)))
            .collect::<Result<_, PublishingError>>()?;
        Ok((row, base, overrides))
    }

    /// Resolves and validates every target of a new job. All violations are returned at once,
    /// each naming its account in an `account_id` parameter.
    async fn check_targets(
        &self,
        scope: &OwnerScope,
        base: &postit_core::Post,
        overrides: &BTreeMap<AccountId, PostOverride>,
        input: &PublishInput,
        at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<Vec<Target>, PublishingError> {
        let mut violations = Vec::new();
        let mut resolved = Vec::with_capacity(input.targets.len());
        for account in &input.targets {
            let bound = self.bind(scope.owner, *account).await?;
            match resolve_variant(base, &bound.account, overrides.get(account)) {
                Ok(variant) => resolved.push((bound, variant)),
                Err(found) => violations.extend(tagged(found, *account)),
            }
        }
        let ids: Vec<MediaAssetId> = resolved
            .iter()
            .flat_map(|(_, variant)| media_ids(variant))
            .collect();
        let assets = self.known_assets(scope, &ids).await?;
        let mut targets = Vec::with_capacity(resolved.len());
        for (bound, variant) in resolved {
            let account = bound.account.account;
            let descriptor = &bound.descriptor;
            let operation = if descriptor.kind == AgentKind::Direct {
                Operation::Direct
            } else if variant.kind == PostKind::Comment {
                Operation::Comment
            } else {
                Operation::Publish
            };
            let mut checked = variant.clone();
            if operation == Operation::Direct
                && let Err(entry) = direct::take_audience(&mut checked)
            {
                let mut violation = Violation::new(ViolationCode::OptionType);
                if let Some(path) = entry.get("path").and_then(Value::as_str) {
                    violation = violation.at(path);
                }
                violations.extend(tagged(vec![violation], account));
            }
            let schedule = match at {
                Some(at) => Schedule::At {
                    at,
                    native: input.native_schedule,
                },
                None => Schedule::Now,
            };
            violations.extend(tagged(
                validate_target(&ValidationInput {
                    descriptor,
                    options_schema: &descriptor.options_schema,
                    variant: &checked,
                    media: &assets,
                    schedule,
                    now,
                }),
                account,
            ));
            targets.push(Target {
                account,
                native: input.native_schedule
                    && at.is_some()
                    && descriptor.supports_native_schedule(),
                variant,
                operation,
            });
        }
        if violations.is_empty() {
            Ok(targets)
        } else {
            Err(PublishingError::ValidationFailed(violations))
        }
    }

    /// Validates the snapshot's variant for `delivery` again, with its media as they are now.
    async fn revalidate(
        &self,
        scope: &OwnerScope,
        delivery: &Delivery,
        snapshot: &PublishSnapshot,
    ) -> Result<(), PublishingError> {
        let deps = &self.deps;
        let bound = self.bind(scope.owner, delivery.account_id).await?;
        let mut assembled =
            match assemble(snapshot, delivery.account_id, &deps.media, scope.owner).await {
                Ok(assembled) => assembled,
                Err(AssembleError::VariantMissing) => return Err(PublishingError::NotFound),
                Err(AssembleError::MediaMissing(ids)) => {
                    let media = snapshot
                        .variant_for(delivery.account_id)
                        .map(|variant| variant.media.as_slice())
                        .unwrap_or_default();
                    return Err(PublishingError::ValidationFailed(missing_media(
                        media, &ids,
                    )));
                }
                Err(AssembleError::Data(error)) => return Err(error.into()),
            };
        let mut violations = Vec::new();
        if delivery.operation == Operation::Direct
            && let Err(entry) = direct::take_audience(&mut assembled.variant)
        {
            let mut violation = Violation::new(ViolationCode::OptionType);
            if let Some(path) = entry.get("path").and_then(Value::as_str) {
                violation = violation.at(path);
            }
            violations.push(violation);
        }
        let descriptor = &bound.descriptor;
        violations.extend(validate_target(&ValidationInput {
            descriptor,
            options_schema: &descriptor.options_schema,
            variant: &assembled.variant,
            media: &assembled.assets,
            schedule: Schedule::Now,
            now: deps.clock.now(),
        }));
        if violations.is_empty() {
            Ok(())
        } else {
            Err(PublishingError::ValidationFailed(violations))
        }
    }

    /// The accounts of `job` whose agents hold a natively scheduled post until its time, for a
    /// job that schedules natively; empty otherwise.
    async fn native_targets(
        &self,
        scope: &OwnerScope,
        job: &PublishJob,
    ) -> Result<Vec<AccountId>, PublishingError> {
        if !job.native_schedule || job.dry_run {
            return Ok(Vec::new());
        }
        let deliveries = {
            let mut conn = self.deps.pool.acquire().await?;
            DeliveryRepo::list_for_job(&mut conn, scope, job.id).await?
        };
        let mut accounts = Vec::new();
        for account in deliveries.iter().map(|d| d.account_id) {
            if accounts.contains(&account) {
                continue;
            }
            let native = self
                .deps
                .accounts
                .bound_client(scope.owner, account)
                .await
                .is_ok_and(|bound| bound.descriptor.supports_native_schedule());
            if native {
                accounts.push(account);
            }
        }
        Ok(accounts)
    }

    /// Moves a natively scheduled upload to `at` through its agent, then records the new time
    /// in its own transaction under the delivery's fence. `false` when the agent cannot or did
    /// not move it, or the delivery changed meanwhile.
    async fn move_native(
        &self,
        owner: postit_core::UserId,
        delivery: &Delivery,
        at: DateTime<Utc>,
    ) -> Result<bool, PublishingError> {
        let deps = &self.deps;
        let Ok(bound) = deps.accounts.bound_client(owner, delivery.account_id).await else {
            return Ok(false);
        };
        let Some(scheduler) = bound.client.native_scheduler() else {
            return Ok(false);
        };
        let moved = match scheduler
            .reschedule(&bound.account, &receipt_of(delivery), at)
            .await
        {
            Ok(receipt) => receipt.remote_publish_at.unwrap_or(at),
            Err(error) => {
                tracing::warn!(
                    delivery = %delivery.id,
                    class = ?error.retry_class(),
                    "the platform did not move a natively scheduled upload"
                );
                return Ok(false);
            }
        };
        let mut tx = deps.pool.begin().await?;
        let written = DeliveryRepo::set_remote_publish_at(
            &mut tx,
            Fence {
                id: delivery.id,
                dispatch_gen: delivery.dispatch_gen,
                expected: DeliveryState::Processing,
            },
            moved,
            deps.clock.now(),
        )
        .await?;
        if written.is_none() {
            return Ok(false);
        }
        // The waiting poll is due at the old time. Queue the chain's current step at the new
        // one too: whichever runs first takes the step and the other exits, so an upload moved
        // earlier is seen going live on time.
        if let Some(target) = DeliveryRepo::check_target(&mut tx, delivery.id).await? {
            let poll = PollDeliveryStatus {
                delivery: delivery.id.as_uuid(),
                dispatch_gen: delivery.dispatch_gen,
                poll: target.check_seq,
            };
            deps.queue.enqueue_in(&mut tx, &poll, Some(moved)).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Deletes a natively scheduled upload through its agent, then cancels the delivery in its
    /// own transaction under its fence, and files it in `report`. A failure is this delivery's
    /// alone: it is filed as not cancellable and the caller goes on with the next one. (When
    /// the platform delete landed but the cancel could not be written, the delivery stays
    /// `processing` and its poll finds the upload gone.)
    async fn cancel_native(
        &self,
        scope: &OwnerScope,
        job: &PublishJob,
        delivery: &Delivery,
        report: &mut CancelReport,
    ) {
        match self
            .delete_native(scope.owner, Some(scope.actor), job, delivery)
            .await
        {
            Ok(NativeDelete::Cancelled) => report.cancelled.push(delivery.id),
            Ok(NativeDelete::Kept | NativeDelete::Transient) => {
                report.not_cancellable.push(delivery.id);
            }
            Err(_) => {
                tracing::warn!(
                    delivery = %delivery.id,
                    "could not record the cancel of a natively scheduled upload"
                );
                report.not_cancellable.push(delivery.id);
            }
        }
    }

    /// Deletes the natively scheduled upload `delivery` of `owner` through its agent, with no
    /// connection held, then cancels the delivery in its own transaction under its fence
    /// (`actor` is `None` for a system cancel). See [`NativeDelete`] for what it reports.
    pub(crate) async fn delete_native(
        &self,
        owner: UserId,
        actor: Option<UserId>,
        job: &PublishJob,
        delivery: &Delivery,
    ) -> Result<NativeDelete, PublishingError> {
        let deps = &self.deps;
        let bound = match deps.accounts.bound_client(owner, delivery.account_id).await {
            Ok(bound) => bound,
            // A database failure says nothing about the upload: try again later.
            Err(AccountError::Data(_)) => return Ok(NativeDelete::Transient),
            // Disconnected, switched off, or no longer in this build: nothing can delete it.
            Err(_) => return Ok(NativeDelete::Kept),
        };
        let Some(scheduler) = bound.client.native_scheduler() else {
            return Ok(NativeDelete::Kept);
        };
        if let Err(error) = scheduler
            .delete(&bound.account, &receipt_of(delivery))
            .await
        {
            let class = error.retry_class();
            tracing::warn!(
                delivery = %delivery.id,
                class = class.as_str(),
                "the platform did not delete a natively scheduled upload"
            );
            return Ok(match class {
                RetryClass::Retryable
                | RetryClass::RetryAfter(_)
                | RetryClass::QuotaExceeded
                | RetryClass::Ambiguous => NativeDelete::Transient,
                RetryClass::Permanent | RetryClass::NeedsReconnect | RetryClass::Cancelled => {
                    NativeDelete::Kept
                }
            });
        }
        let mut tx = deps.pool.begin().await?;
        let applied = DeliveryRepo::apply(
            &mut tx,
            Fence {
                id: delivery.id,
                dispatch_gen: delivery.dispatch_gen,
                expected: DeliveryState::Processing,
            },
            DeliveryEvent::Cancel,
            TransitionData {
                actor,
                ..TransitionData::default()
            },
            deps.clock.now(),
        )
        .await?;
        let outcome = match applied {
            Applied::Done { job: change, .. } => {
                self.job_partial(&mut tx, job, owner, change).await?;
                NativeDelete::Cancelled
            }
            Applied::Stale => NativeDelete::Kept,
        };
        tx.commit().await?;
        Ok(outcome)
    }

    /// Deletes every natively scheduled upload of `owner` (of `account` alone when given)
    /// through its agent and cancels it, one at a time with no connection held across a
    /// platform call, and counts each [`NativeDelete`] outcome.
    pub(crate) async fn delete_natives(
        &self,
        owner: UserId,
        account: Option<AccountId>,
    ) -> Result<NativeTally, PublishingError> {
        let deps = &self.deps;
        let scope = OwnerScope::own(owner);
        let natives = {
            let mut conn = deps.pool.acquire().await?;
            let deliveries =
                DeliveryRepo::native_scheduled(&mut conn, owner, account, deps.clock.now()).await?;
            let mut natives = Vec::with_capacity(deliveries.len());
            for delivery in deliveries {
                let job = JobRepo::get(&mut conn, &scope, delivery.job_id).await?;
                natives.push((job, delivery));
            }
            natives
        };
        let mut tally = NativeTally::default();
        for (job, delivery) in &natives {
            match self.delete_native(owner, None, job, delivery).await? {
                NativeDelete::Cancelled => tally.deleted += 1,
                NativeDelete::Kept => tally.kept += 1,
                NativeDelete::Transient => tally.transient += 1,
            }
        }
        Ok(tally)
    }

    /// Records `job_partial` for each live job a system cancel in `results` first made
    /// `partial`, in the caller's transaction.
    pub(crate) async fn record_cancel_partials(
        &self,
        tx: &mut PgConnection,
        owner: UserId,
        results: &[CancelResult],
    ) -> Result<(), PublishingError> {
        let scope = OwnerScope::own(owner);
        for result in results {
            let Some(change) = result.change.filter(JobStatusChange::became_partial) else {
                continue;
            };
            let job = JobRepo::get(&mut *tx, &scope, result.job).await?;
            self.job_partial(tx, &job, owner, change).await?;
        }
        Ok(())
    }

    /// Records `job_partial` when `change` is the write that made a live job `partial`.
    async fn job_partial(
        &self,
        tx: &mut PgConnection,
        job: &PublishJob,
        owner: UserId,
        change: JobStatusChange,
    ) -> Result<(), PublishingError> {
        if job.dry_run || !change.became_partial() {
            return Ok(());
        }
        notices::record_job_partial(&self.deps.notifier, tx, job.id, job.post_id, owner)
            .await
            .map_err(PublishingError::from)
    }

    /// A client bound to `account` of `owner`, its errors mapped.
    async fn bind(
        &self,
        owner: postit_core::UserId,
        account: AccountId,
    ) -> Result<BoundClient, PublishingError> {
        self.deps
            .accounts
            .bound_client(owner, account)
            .await
            .map_err(|error| match error {
                AccountError::NotFound | AccountError::AccountDisconnected => {
                    PublishingError::NotFound
                }
                AccountError::PlatformDisabled => PublishingError::PlatformDisabled,
                AccountError::Data(error) => error.into(),
                AccountError::Scope(error) => PublishingError::Scope(error),
                _ => PublishingError::PlatformUnavailable,
            })
    }

    /// The assets among `ids` that the owner still has; validation reports the rest missing.
    async fn known_assets(
        &self,
        scope: &OwnerScope,
        ids: &[MediaAssetId],
    ) -> Result<Vec<MediaAsset>, PublishingError> {
        let mut seen = HashSet::new();
        let ids: Vec<MediaAssetId> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let owner = OwnerScope::own(scope.owner);
        let media = &self.deps.media;
        match media.assets(&owner, &ids).await {
            Ok(assets) => return Ok(assets),
            Err(MediaServiceError::NotFound) => {}
            Err(other) => return Err(media_error(other)),
        }
        let mut found = Vec::with_capacity(ids.len());
        for id in ids {
            match media.assets(&owner, &[id]).await {
                Ok(assets) => found.extend(assets),
                Err(MediaServiceError::NotFound) => {}
                Err(other) => return Err(media_error(other)),
            }
        }
        Ok(found)
    }

    /// Records an audit event by the caller, carrying IDs and counts only.
    async fn audit(
        &self,
        tx: &mut PgConnection,
        kind: AuditEventKind,
        scope: &OwnerScope,
        audit: AuditContext,
        details: &[(&str, Value)],
    ) -> Result<(), PublishingError> {
        let mut event = AuditEvent::new(kind)
            .actor(scope.actor)
            .owner(scope.owner)
            .context(audit);
        for (key, value) in details {
            event = event
                .detail(key, value.clone())
                .map_err(|error| PublishingError::Corrupt(error.to_string()))?;
        }
        AuditLog::record(tx, AuditEventId::from(self.deps.ids.generate()), event).await?;
        Ok(())
    }
}

/// When a job runs: `None` for now, `Some(at)` for a time ahead. A time up to a minute in the
/// past is now; an earlier one is a `schedule_in_past` violation.
fn due_time(
    at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, PublishingError> {
    let Some(at) = at else {
        return Ok(None);
    };
    if at > now {
        return Ok(Some(at));
    }
    let earliest = now.checked_sub_signed(SCHEDULE_GRACE).unwrap_or(now);
    if at < earliest {
        return Err(PublishingError::ValidationFailed(vec![
            Violation::new(ViolationCode::ScheduleInPast).at("schedule"),
        ]));
    }
    Ok(None)
}

/// The receipt an agent's `NativeScheduler` reads for `delivery`.
fn receipt_of(delivery: &Delivery) -> PublishReceipt {
    PublishReceipt {
        remote_id: delivery.remote_id.clone(),
        remote_url: delivery
            .remote_url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok()),
        container_ref: None,
        remote_publish_at: delivery.remote_publish_at,
    }
}

/// A stored override as core reads it: its patch with its options beside it.
fn override_of(row: PostOverrideRow) -> Result<PostOverride, PublishingError> {
    let mut over: PostOverride =
        serde_json::from_value(row.patch).map_err(|_| corrupt("post_override_invalid"))?;
    if let Value::Object(options) = row.options {
        over.options = options;
    }
    Ok(over)
}

/// The media a variant references: its attachments and an email's images.
fn media_ids(variant: &Variant) -> Vec<MediaAssetId> {
    let mut ids: Vec<MediaAssetId> = variant.media.iter().map(|item| item.asset).collect();
    if let Some(email) = &variant.email {
        ids.extend(email.media_ids());
    }
    ids
}

/// `violations` with the account they are about, as an `account_id` parameter.
fn tagged(violations: Vec<Violation>, account: AccountId) -> Vec<Violation> {
    violations
        .into_iter()
        .map(|violation| violation.param("account_id", account.to_string()))
        .collect()
}

/// One `media_missing` violation per missing asset, at its place in the variant's media.
fn missing_media(media: &[MediaItem], ids: &[MediaAssetId]) -> Vec<Violation> {
    if ids.is_empty() {
        return vec![Violation::new(ViolationCode::MediaMissing)];
    }
    ids.iter()
        .map(|id| {
            let violation = Violation::new(ViolationCode::MediaMissing).param("id", id.to_string());
            match media.iter().position(|item| item.asset == *id) {
                Some(index) => violation.at(format!("media[{index}]")),
                None => violation,
            }
        })
        .collect()
}

fn corrupt(code: &str) -> PublishingError {
    PublishingError::Corrupt(code.to_owned())
}

fn media_error(error: MediaServiceError) -> PublishingError {
    match error {
        MediaServiceError::Data(error) => error.into(),
        other => PublishingError::Corrupt(other.to_string()),
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_time_up_to_a_minute_past_is_now() {
        let now = Utc::now();
        assert!(matches!(due_time(None, now), Ok(None)));
        assert!(matches!(
            due_time(Some(now - TimeDelta::seconds(59)), now),
            Ok(None)
        ));
        let ahead = now + TimeDelta::minutes(5);
        assert!(matches!(due_time(Some(ahead), now), Ok(Some(at)) if at == ahead));
        let Err(PublishingError::ValidationFailed(found)) =
            due_time(Some(now - TimeDelta::seconds(61)), now)
        else {
            panic!("a time more than a minute past is a violation");
        };
        assert_eq!(found[0].code, ViolationCode::ScheduleInPast);
    }
}
