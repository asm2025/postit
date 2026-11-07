//! Wire types of the publish, job, and delivery routes. A delivery's last error carries a code
//! and a retry class only, never a platform's message.

use chrono::{DateTime, Utc};
use postit_core::{DeliveryState, JobStatus, ParkReason, ResolvedOutcome, ScheduleInput};
use postit_data::publishing::direct::DirectCounts;
use postit_data::publishing::{CalendarJob, Delivery, Operation, PublishJob};
use postit_publishing::{CancelReport, RescheduleOutcome};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// What a delivery does on its platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationDto {
    /// Publishes a post.
    Publish,
    /// Publishes a comment on someone's post.
    Comment,
    /// Sends to each contact of an audience.
    Direct,
}

impl From<Operation> for OperationDto {
    fn from(operation: Operation) -> Self {
        match operation {
            Operation::Publish => Self::Publish,
            Operation::Comment => Self::Comment,
            Operation::Direct => Self::Direct,
        }
    }
}

/// Where a delivery is in its life. A parked delivery names why in `parked_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStateDto {
    /// Waiting for its time or a worker.
    Pending,
    /// A worker holds the lease and has not called the platform yet.
    Claimed,
    /// A platform call is under way.
    InFlight,
    /// The platform accepted the post and is still processing it.
    Processing,
    /// A call may have changed the platform and its effect is unknown.
    OutcomeUnknown,
    /// Waiting for something outside the engine; see `parked_reason`.
    Parked,
    /// Live on the platform.
    Published,
    /// Failed for good.
    Failed,
    /// Retries ran out.
    DeadLetter,
    /// Cancelled by the user.
    Cancelled,
    /// A dry run completed.
    Simulated,
}

impl From<DeliveryState> for DeliveryStateDto {
    fn from(state: DeliveryState) -> Self {
        match state {
            DeliveryState::Pending => Self::Pending,
            DeliveryState::Claimed => Self::Claimed,
            DeliveryState::InFlight => Self::InFlight,
            DeliveryState::Processing => Self::Processing,
            DeliveryState::OutcomeUnknown => Self::OutcomeUnknown,
            DeliveryState::Parked(_) => Self::Parked,
            DeliveryState::Published => Self::Published,
            DeliveryState::Failed => Self::Failed,
            DeliveryState::DeadLetter => Self::DeadLetter,
            DeliveryState::Cancelled => Self::Cancelled,
            DeliveryState::Simulated => Self::Simulated,
        }
    }
}

/// The classified error behind a failed or parked delivery.
#[derive(Debug, Serialize, ToSchema)]
pub struct LastErrorDto {
    /// The error's code, such as `rate_limited`.
    pub code: String,
    /// How the engine treated it, such as `retryable`.
    pub class: String,
}

/// How far a direct (email) delivery has got.
#[derive(Debug, Serialize, ToSchema)]
pub struct DirectCountsDto {
    /// Recipients the delivery was materialized for.
    pub audience: i64,
    /// Recipients still waiting.
    pub queued: i64,
    /// Recipients the provider accepted.
    pub sent: i64,
    /// Recipients whose send failed, including cancelled ones.
    pub failed: i64,
    /// Recipients skipped because the address is suppressed.
    pub skipped: i64,
    /// Recipients whose address bounced after the provider accepted it.
    pub bounced: i64,
}

impl From<DirectCounts> for DirectCountsDto {
    fn from(counts: DirectCounts) -> Self {
        Self {
            audience: counts.audience,
            queued: counts.queued,
            sent: counts.sent,
            failed: counts.failed,
            skipped: counts.suppressed,
            bounced: counts.bounced,
        }
    }
}

/// One post going to one account.
#[derive(Debug, Serialize, ToSchema)]
pub struct DeliveryDto {
    /// The delivery.
    pub id: Uuid,
    /// The job it belongs to.
    pub job_id: Uuid,
    /// The post it publishes.
    pub post_id: Uuid,
    /// The account it goes to.
    pub account_id: Uuid,
    /// What it does on the platform.
    pub operation: OperationDto,
    /// Where it is.
    pub state: DeliveryStateDto,
    /// Why it is parked, while `state` is `parked`.
    pub parked_reason: Option<ParkReason>,
    /// Attempts counted so far.
    pub attempt_count: i32,
    /// When it is next due.
    pub next_attempt_at: Option<DateTime<Utc>>,
    /// The platform's ID for the post.
    pub remote_id: Option<String>,
    /// The post's public URL.
    pub remote_url: Option<String>,
    /// When a natively scheduled post goes live.
    pub remote_publish_at: Option<DateTime<Utc>>,
    /// The classified error behind the last failure or park.
    pub last_error: Option<LastErrorDto>,
    /// Progress of a direct send.
    pub direct: Option<DirectCountsDto>,
    /// Who created the job it belongs to (a user ID).
    pub created_by: Option<Uuid>,
}

impl DeliveryDto {
    /// The delivery as clients see it, with the progress of a direct send.
    #[must_use]
    pub fn new(delivery: Delivery, direct: Option<DirectCounts>) -> Self {
        let parked_reason = match delivery.state {
            DeliveryState::Parked(reason) => Some(reason),
            _ => None,
        };
        let last_error = delivery.last_error.as_ref().and_then(|value| {
            Some(LastErrorDto {
                code: value.get("code")?.as_str()?.to_owned(),
                class: value.get("class")?.as_str()?.to_owned(),
            })
        });
        Self {
            id: delivery.id.as_uuid(),
            job_id: delivery.job_id.as_uuid(),
            post_id: delivery.post_id.as_uuid(),
            account_id: delivery.account_id.as_uuid(),
            operation: delivery.operation.into(),
            state: delivery.state.into(),
            parked_reason,
            attempt_count: delivery.attempt_count,
            next_attempt_at: delivery.next_attempt_at,
            remote_id: delivery.remote_id,
            remote_url: delivery.remote_url,
            remote_publish_at: delivery.remote_publish_at,
            last_error,
            direct: direct.map(Into::into),
            created_by: delivery.created_by.map(|u| u.as_uuid()),
        }
    }
}

/// A publish job without its deliveries: an item of the job list.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublishJobSummaryDto {
    /// The job.
    pub id: Uuid,
    /// The post it publishes.
    pub post_id: Uuid,
    /// The post version it froze.
    pub post_version: i64,
    /// When its deliveries become due.
    pub scheduled_at: DateTime<Utc>,
    /// Whether platforms hold the post until its time.
    pub native_schedule: bool,
    /// Whether it only simulates.
    pub dry_run: bool,
    /// Where it is, derived from its deliveries.
    pub status: JobStatus,
    /// Bumped on every delivery change; the `ETag` is `"v{version}"`.
    pub version: i64,
    /// When it was cancelled.
    pub cancelled_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// Who created it (a user ID).
    pub created_by: Option<Uuid>,
}

impl From<PublishJob> for PublishJobSummaryDto {
    fn from(job: PublishJob) -> Self {
        Self {
            id: job.id.as_uuid(),
            post_id: job.post_id.as_uuid(),
            post_version: job.post_version,
            scheduled_at: job.scheduled_at,
            native_schedule: job.native_schedule,
            dry_run: job.dry_run,
            status: job.status,
            version: job.version,
            cancelled_at: job.cancelled_at,
            created_at: job.created_at,
            created_by: job.created_by.map(|u| u.as_uuid()),
        }
    }
}

/// A publish job with the deliveries the caller can see.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublishJobDto {
    /// The job itself.
    #[serde(flatten)]
    pub job: PublishJobSummaryDto,
    /// Its deliveries inside the caller's account limit.
    pub deliveries: Vec<DeliveryDto>,
}

/// What to publish a saved post as. Needs `If-Match` with the post's `ETag`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishRequest {
    /// The accounts to publish to; a client expands target groups itself.
    pub targets: Vec<Uuid>,
    /// When to publish; now when absent.
    #[serde(default)]
    pub at: Option<ScheduleInput>,
    /// Whether platforms that can hold the post until `at` do so.
    #[serde(default)]
    pub native_schedule: bool,
    /// Whether the job only simulates.
    #[serde(default)]
    pub dry_run: bool,
    /// The caller's confirmation of a live publish.
    #[serde(default)]
    pub confirm: bool,
}

/// A new time for a job that has not started. Needs `If-Match` with the job's `ETag`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RescheduleRequest {
    /// When to publish.
    pub at: ScheduleInput,
}

/// What a reschedule moved.
#[derive(Debug, Serialize, ToSchema)]
pub struct RescheduleOutcomeDto {
    /// Deliveries now at the new time.
    pub moved: Vec<Uuid>,
    /// Natively scheduled uploads the platform did not move; they keep their old time.
    pub failed: Vec<Uuid>,
}

impl From<RescheduleOutcome> for RescheduleOutcomeDto {
    fn from(outcome: RescheduleOutcome) -> Self {
        Self {
            moved: outcome.moved.into_iter().map(|d| d.as_uuid()).collect(),
            failed: outcome.failed.into_iter().map(|d| d.as_uuid()).collect(),
        }
    }
}

/// What a cancel did to each delivery that had not ended.
#[derive(Debug, Serialize, ToSchema)]
pub struct CancelReportDto {
    /// Now cancelled.
    pub cancelled: Vec<Uuid>,
    /// Running: the worker stops at its next safe checkpoint.
    pub requested: Vec<Uuid>,
    /// Left as they were: an unknown outcome (resolve it instead), a platform still
    /// processing, or a native upload the agent could not delete.
    pub not_cancellable: Vec<Uuid>,
}

impl From<CancelReport> for CancelReportDto {
    fn from(report: CancelReport) -> Self {
        let ids =
            |ids: Vec<postit_core::DeliveryId>| ids.into_iter().map(|d| d.as_uuid()).collect();
        Self {
            cancelled: ids(report.cancelled),
            requested: ids(report.requested),
            not_cancellable: ids(report.not_cancellable),
        }
    }
}

/// Sending a failed or stuck delivery again.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryRequest {
    /// The caller's confirmation.
    #[serde(default)]
    pub confirm: bool,
    /// For a delivery whose outcome is unknown: the caller accepts that the post may already
    /// be live.
    #[serde(default)]
    pub acknowledge_outcome_unknown: bool,
}

/// Settling a delivery whose outcome nobody can verify.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveRequest {
    /// Whether the post is live (`published`) or not and will not be retried (`failed`).
    pub outcome: ResolvedOutcome,
}

/// Flat on purpose (see `UserListQuery`).
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct JobListQuery {
    /// Only jobs in this status.
    pub status: Option<JobStatus>,
    /// Only jobs of this post.
    pub post_id: Option<Uuid>,
    /// Only jobs of posts in this campaign.
    pub campaign_id: Option<Uuid>,
    /// Only dry runs (`true`) or only live jobs (`false`).
    pub dry_run: Option<bool>,
    /// Only jobs scheduled at or after this time (RFC 3339).
    pub from: Option<DateTime<Utc>>,
    /// Only jobs scheduled before this time (RFC 3339).
    pub to: Option<DateTime<Utc>>,
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

/// What a calendar entry is. Recurring posts add `occurrence` later, so clients should
/// ignore kinds they do not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CalendarKindDto {
    /// A scheduled publish job.
    Job,
}

/// The longest first line of a post a calendar cell shows, in graphemes.
const TEXT_HEAD_MAX: usize = 80;

/// One scheduled job on the calendar.
#[derive(Debug, Serialize, ToSchema)]
pub struct CalendarItemDto {
    /// What the entry is.
    pub kind: CalendarKindDto,
    /// The job.
    pub job_id: Uuid,
    /// The post it publishes.
    pub post_id: Uuid,
    /// When it is due.
    pub scheduled_at: DateTime<Utc>,
    /// Where it is.
    pub status: JobStatus,
    /// The post's campaign.
    pub campaign_id: Option<Uuid>,
    /// The campaign's display color.
    pub campaign_color: Option<String>,
    /// The first line of the post's text, cut to 80 graphemes.
    pub text_head: String,
    /// The accounts it goes to that the caller can see.
    pub account_ids: Vec<Uuid>,
}

impl From<CalendarJob> for CalendarItemDto {
    fn from(job: CalendarJob) -> Self {
        use unicode_segmentation::UnicodeSegmentation as _;

        Self {
            kind: CalendarKindDto::Job,
            job_id: job.job_id.as_uuid(),
            post_id: job.post_id.as_uuid(),
            scheduled_at: job.scheduled_at,
            status: job.status,
            campaign_id: job.campaign_id.map(|id| id.as_uuid()),
            campaign_color: job.campaign_color,
            text_head: job.text_head.graphemes(true).take(TEXT_HEAD_MAX).collect(),
            account_ids: job.account_ids.into_iter().map(|a| a.as_uuid()).collect(),
        }
    }
}

/// The calendar of a range.
#[derive(Debug, Serialize, ToSchema)]
pub struct CalendarDto {
    /// Live jobs due in the range, soonest first.
    pub items: Vec<CalendarItemDto>,
}

/// The range of a calendar: `[from, to)`, at most 62 days.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct CalendarQuery {
    /// The first instant of the range, RFC 3339.
    pub from: DateTime<Utc>,
    /// The first instant after the range, RFC 3339; later than `from` by at most 62 days.
    pub to: DateTime<Utc>,
}
